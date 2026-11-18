use crate::NodeId;
use crate::message::LlmMessage;
use crate::tool::{ToolContext, ToolRegistry};
use crate::variable::{
    Variable, VariableError, VariablePath, VariableSource, VariableType, is_identifier, traverse,
};
use crate::{LlmConfig, LlmRuntime};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Shared conversation history for one agent run.
///
/// Only LLM nodes append to this history: the user message built from their
/// input and output format, assistant tool calls, tool results and the final
/// assistant reply. Fixed nodes never append tool traces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    messages: Vec<LlmMessage>,
}

impl History {
    pub fn messages(&self) -> &[LlmMessage] {
        &self.messages
    }

    pub fn push(&mut self, message: LlmMessage) {
        self.messages.push(message);
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Replace the messages before `keep_from` with one summary message.
    pub fn replace_with_summary(&mut self, keep_from: usize, summary: String) {
        let tail = self.messages.split_off(keep_from);
        self.messages.clear();
        self.messages.push(LlmMessage::user(format!(
            "[earlier conversation summary]\n{summary}"
        )));
        self.messages.extend(tail);
    }
}

/// State shared by all nodes in one graph run.
///
/// Variables are always resolved through the context. A context can aggregate:
///
/// - local variables set with [`Context::set_var`];
/// - external [`VariableSource`] implementations registered with
///   [`Context::register_source`].
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Context {
    vars: BTreeMap<String, Variable>,
    #[serde(skip)]
    llm_configs: BTreeMap<NodeId, LlmConfig>,
    #[serde(skip)]
    llm_runtime: Option<Arc<LlmRuntime>>,
    #[serde(skip)]
    tool_registry: Option<Arc<ToolRegistry>>,
    #[serde(skip)]
    tool_context: ToolContext,
    #[serde(default)]
    history: History,
    #[serde(skip)]
    sources: Vec<Arc<dyn VariableSource>>,
    #[serde(skip)]
    current_node: Option<NodeId>,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("vars", &self.vars)
            .field("llm_configs", &self.llm_configs)
            .field("llm_runtime", &self.llm_runtime.is_some())
            .field("tool_registry", &self.tool_registry.is_some())
            .field("tool_context", &self.tool_context)
            .field("sources", &self.sources.len())
            .field("current_node", &self.current_node)
            .finish()
    }
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_var<T: Serialize>(
        &mut self,
        name: impl Into<String>,
        value: T,
    ) -> Result<(), ContextError> {
        let name = name.into();
        if !is_identifier(&name) {
            return Err(ContextError::InvalidVarName(name));
        }

        let value = serde_json::to_value(value)?;
        self.vars.insert(name.clone(), Variable::new(name, value)?);
        Ok(())
    }

    pub fn set_typed_var(
        &mut self,
        name: impl Into<String>,
        ty: VariableType,
        value: Value,
    ) -> Result<(), ContextError> {
        let name = name.into();
        if !is_identifier(&name) {
            return Err(ContextError::InvalidVarName(name));
        }

        self.vars
            .insert(name.clone(), Variable::typed(name, ty, value)?);
        Ok(())
    }

    pub fn var<T: DeserializeOwned>(&self, name: &str) -> Result<T, ContextError> {
        let variable = self
            .vars
            .get(name)
            .ok_or_else(|| ContextError::MissingVar(name.to_owned()))?;
        Ok(serde_json::from_value(variable.value().clone())?)
    }

    /// Register an external variable source.
    ///
    /// Sources are checked in registration order after local variables and the
    /// reserved `nodes` variable. The first source that provides a root variable
    /// wins.
    pub fn register_source(&mut self, source: Arc<dyn VariableSource>) {
        self.sources.push(source);
    }

    /// Configure the LLM settings for one graph node.
    pub fn set_llm_config(&mut self, node: impl Into<NodeId>, config: LlmConfig) {
        self.llm_configs.insert(node.into(), config);
    }

    /// Install engine-level LLM capabilities for this run.
    pub fn set_llm_runtime(&mut self, runtime: Arc<LlmRuntime>) {
        self.llm_runtime = Some(runtime);
    }

    /// LLM capabilities used by LLM nodes during this run.
    pub fn llm_runtime(&self) -> Result<&Arc<LlmRuntime>, ContextError> {
        self.llm_runtime
            .as_ref()
            .ok_or(ContextError::MissingLlmRuntime)
    }

    /// Install the tool registry used by LLM and deterministic nodes.
    pub fn set_tool_registry(&mut self, registry: Arc<ToolRegistry>) {
        self.tool_registry = Some(registry);
    }

    pub fn tool_registry(&self) -> Result<&Arc<ToolRegistry>, ContextError> {
        self.tool_registry
            .as_ref()
            .ok_or(ContextError::MissingToolRegistry)
    }

    pub fn set_tool_context(&mut self, context: ToolContext) {
        self.tool_context = context;
    }

    pub fn tool_context(&self) -> &ToolContext {
        &self.tool_context
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub(crate) fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// LLM settings for the node currently being executed.
    pub fn current_llm_config(&self) -> Result<&LlmConfig, ContextError> {
        let node = self
            .current_node
            .as_ref()
            .ok_or(ContextError::NoCurrentNode)?;

        self.llm_configs
            .get(node)
            .ok_or_else(|| ContextError::MissingLlmConfig(node.clone()))
    }

    /// Resolve a parsed path to a typed variable.
    pub fn resolve_variable(&self, path: &VariablePath) -> Result<Variable, VariableError> {
        let segments = path.segments();
        let (root, rest) = segments
            .split_first()
            .ok_or_else(|| VariableError::InvalidPath(path.to_string()))?;

        if let Some(variable) = self.vars.get(root) {
            return traverse(variable, rest, &path.to_string());
        }

        for source in &self.sources {
            if let Some(variable) = source.get(root)? {
                return traverse(&variable, rest, &path.to_string());
            }
        }

        Err(VariableError::Unknown(path.to_string()))
    }

    /// Resolve a typed variable by path.
    pub fn resolve<T: DeserializeOwned>(&self, path: &str) -> Result<T, ContextError> {
        let path = VariablePath::parse(path)?;
        let variable = self.resolve_variable(&path)?;
        Ok(serde_json::from_value(variable.value().clone())?)
    }

    pub fn resolve_value(&self, path: &VariablePath) -> Result<Value, ContextError> {
        Ok(self.resolve_variable(path)?.value().clone())
    }

    pub(crate) fn set_current_node(&mut self, node: Option<NodeId>) {
        self.current_node = node;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("missing variable: {0}")]
    MissingVar(String),

    #[error("missing LLM config for node: {0}")]
    MissingLlmConfig(NodeId),

    #[error("LLM runtime is not installed in the context")]
    MissingLlmRuntime,

    #[error("tool registry is not installed in the context")]
    MissingToolRegistry,

    #[error("no current node is set")]
    NoCurrentNode,

    #[error("invalid variable name: {0}")]
    InvalidVarName(String),

    #[error(transparent)]
    Variable(#[from] VariableError),

    #[error("context json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MapSource;
    use serde_json::json;

    fn project_source() -> Arc<dyn VariableSource> {
        Arc::new(
            MapSource::from_json([
                (
                    "project",
                    json!({"name": "foundry", "config": {"retries": 3}}),
                ),
                ("owner", json!("team")),
            ])
            .unwrap(),
        )
    }

    #[test]
    fn resolves_local_vars_nested_values_and_sources() {
        let mut ctx = Context::new();
        ctx.set_var("task", json!({"title": "login"})).unwrap();
        ctx.register_source(project_source());

        assert_eq!(ctx.resolve::<String>("task.title").unwrap(), "login");
        assert_eq!(ctx.resolve::<String>("project.name").unwrap(), "foundry");
        assert_eq!(ctx.resolve::<u32>("project.config.retries").unwrap(), 3);
        assert_eq!(ctx.resolve::<String>("owner").unwrap(), "team");
    }

    #[test]
    fn rejects_invalid_and_reserved_variable_names() {
        let mut ctx = Context::new();
        assert!(matches!(
            ctx.set_var("bad-name", 1),
            Err(ContextError::InvalidVarName(_))
        ));
    }

    #[test]
    fn preserves_types_when_resolving() {
        let mut ctx = Context::new();
        ctx.set_var("count", 3).unwrap();
        let path = VariablePath::parse("count").unwrap();
        let variable = ctx.resolve_variable(&path).unwrap();

        assert_eq!(variable.ty(), &VariableType::Int);
    }
}
