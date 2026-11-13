use crate::NodeId;
use crate::variable::{
    Variable, VariableError, VariablePath, VariableSource, VariableType, is_identifier, traverse,
};
use crate::{LlmConfig, LlmRuntime};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// State shared by all nodes in one graph run.
///
/// Variables are always resolved through the context. A context can aggregate:
///
/// - local variables set with [`Context::set_var`];
/// - node outputs exposed under the reserved `nodes` variable;
/// - external [`VariableSource`] implementations registered with
///   [`Context::register_source`].
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Context {
    vars: BTreeMap<String, Variable>,
    outputs: BTreeMap<NodeId, Variable>,
    #[serde(skip)]
    llm_configs: BTreeMap<NodeId, LlmConfig>,
    #[serde(skip)]
    llm_runtime: Option<Arc<LlmRuntime>>,
    #[serde(skip)]
    sources: Vec<Arc<dyn VariableSource>>,
    #[serde(skip)]
    current_node: Option<NodeId>,
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("vars", &self.vars)
            .field("outputs", &self.outputs)
            .field("llm_configs", &self.llm_configs)
            .field("llm_runtime", &self.llm_runtime.is_some())
            .field("sources", &self.sources.len())
            .field("current_node", &self.current_node)
            .finish()
    }
}

impl Context {
    pub const NODES_VAR: &'static str = "nodes";

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
        if name == Self::NODES_VAR {
            return Err(ContextError::ReservedVarName(name));
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
        if name == Self::NODES_VAR {
            return Err(ContextError::ReservedVarName(name));
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

        if root == Self::NODES_VAR {
            return traverse(&self.node_outputs_variable(), rest, &path.to_string());
        }

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

    /// Store the output of the node currently being executed.
    pub fn set_output<T: Serialize>(&mut self, value: T) -> Result<(), ContextError> {
        let node = self
            .current_node
            .clone()
            .ok_or(ContextError::NoCurrentNode)?;
        let value = serde_json::to_value(value)?;
        self.outputs
            .insert(node.clone(), Variable::new(node.to_string(), value)?);
        Ok(())
    }

    /// Read the output of the node currently being executed.
    pub fn current_output<T: DeserializeOwned>(&self) -> Result<T, ContextError> {
        let node = self
            .current_node
            .as_ref()
            .ok_or(ContextError::NoCurrentNode)?;
        let variable = self
            .outputs
            .get(node)
            .ok_or_else(|| ContextError::MissingOutput(node.to_string()))?;
        Ok(serde_json::from_value(variable.value().clone())?)
    }

    pub fn node_output<T: DeserializeOwned>(&self, node: &str) -> Result<T, ContextError> {
        let variable = self
            .outputs
            .get(node)
            .ok_or_else(|| ContextError::MissingOutput(node.to_owned()))?;
        Ok(serde_json::from_value(variable.value().clone())?)
    }

    fn node_outputs_variable(&self) -> Variable {
        let mut fields = BTreeMap::new();
        let mut values = Map::new();

        for (id, output) in &self.outputs {
            fields.insert(
                id.as_str().to_owned(),
                VariableType::Object(BTreeMap::from([("output".to_owned(), output.ty().clone())])),
            );

            let mut wrapper = Map::new();
            wrapper.insert("output".to_owned(), output.value().clone());
            values.insert(id.as_str().to_owned(), Value::Object(wrapper));
        }

        Variable::typed(
            Self::NODES_VAR,
            VariableType::Object(fields),
            Value::Object(values),
        )
        .expect("synthetic nodes variable is valid")
    }

    pub(crate) fn set_current_node(&mut self, node: Option<NodeId>) {
        self.current_node = node;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("missing variable: {0}")]
    MissingVar(String),

    #[error("missing output for node: {0}")]
    MissingOutput(String),

    #[error("missing LLM config for node: {0}")]
    MissingLlmConfig(NodeId),

    #[error("LLM runtime is not installed in the context")]
    MissingLlmRuntime,

    #[error("no current node is set")]
    NoCurrentNode,

    #[error("invalid variable name: {0}")]
    InvalidVarName(String),

    #[error("variable name is reserved: {0}")]
    ReservedVarName(String),

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
    fn resolves_current_node_output_and_nodes_path() {
        let mut ctx = Context::new();
        ctx.set_current_node(Some(NodeId::new("load")));
        ctx.set_output(json!({"requirement": "auth"})).unwrap();

        let output: Value = ctx.current_output().unwrap();
        assert_eq!(output["requirement"], "auth");

        let requirement: String = ctx.resolve("nodes.load.output.requirement").unwrap();
        assert_eq!(requirement, "auth");
    }

    #[test]
    fn rejects_invalid_and_reserved_variable_names() {
        let mut ctx = Context::new();
        assert!(matches!(
            ctx.set_var("bad-name", 1),
            Err(ContextError::InvalidVarName(_))
        ));
        assert!(matches!(
            ctx.set_var("nodes", 1),
            Err(ContextError::ReservedVarName(_))
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
