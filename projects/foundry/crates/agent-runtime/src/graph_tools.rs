//! Adapt Foundry's tool registry to graph-agent's tool system.
//!
//! Foundry tools remain the single implementation. They are wrapped so fixed
//! graph nodes and LLM nodes can use the same registry. Foundry tool output is
//! currently text, so it maps to [`graph_agent::ToolOutput::Text`].

use crate::{
    Tool as FoundryTool, ToolContext as FoundryToolContext, ToolError as FoundryToolError,
    ToolRegistry as FoundryToolRegistry,
};
use async_trait::async_trait;
use graph_agent::{
    Tool as GraphTool, ToolContext as GraphToolContext, ToolDefinition,
    ToolError as GraphToolError, ToolOutput as GraphToolOutput, ToolRegistry as GraphToolRegistry,
    codes,
};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;

/// First tool-specific error code used by the Foundry adapter. Foundry tools
/// do not expose their own numeric codes yet.
pub const FOUNDRY_TOOL_FAILED: u32 = 1000;

/// One Foundry tool exposed through graph-agent.
pub struct GraphToolAdapter {
    inner: Arc<dyn FoundryTool>,
}

#[async_trait]
impl GraphTool for GraphToolAdapter {
    fn definition(&self) -> ToolDefinition {
        let spec = self.inner.spec();
        ToolDefinition {
            name: spec.name,
            description: spec.description,
            input_schema: spec.input_schema,
        }
    }

    async fn call(
        &self,
        arguments: Value,
        context: &GraphToolContext,
    ) -> Result<GraphToolOutput, GraphToolError> {
        let name = self.inner.spec().name;
        let started = Instant::now();
        tracing::info!(
            tool = %name,
            arguments = %loggable(&arguments),
            "tool call started"
        );
        let context = FoundryToolContext {
            agent_id: context.agent_id.clone().unwrap_or_default(),
            task_id: context.task_id.clone().unwrap_or_default(),
            project_id: context.project_id.clone(),
        };
        match self.inner.call(arguments, &context).await {
            Ok(output) if output.is_error => {
                tracing::warn!(
                    tool = %name,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %output.content,
                    "tool call failed"
                );
                Err(GraphToolError::new(FOUNDRY_TOOL_FAILED, output.content))
            }
            Ok(output) => {
                tracing::info!(
                    tool = %name,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    output_bytes = output.content.len(),
                    "tool call finished"
                );
                Ok(GraphToolOutput::Text(output.content))
            }
            Err(error) => {
                tracing::warn!(
                    tool = %name,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    error = %error,
                    "tool call failed"
                );
                Err(map_error(error))
            }
        }
    }
}

/// Wrap every tool in a Foundry registry as a graph-agent registry.
pub fn adapt_registry(
    registry: &FoundryToolRegistry,
) -> Result<GraphToolRegistry, graph_agent::RegistryError> {
    let mut adapted = GraphToolRegistry::new();
    for name in registry.names() {
        if let Some(tool) = registry.get(&name) {
            adapted.register(Arc::new(GraphToolAdapter { inner: tool }))?;
        }
    }
    Ok(adapted)
}

fn map_error(error: FoundryToolError) -> GraphToolError {
    match error {
        FoundryToolError::InvalidArguments(message) => {
            GraphToolError::new(codes::INVALID_ARGUMENTS, message)
        }
        FoundryToolError::Unknown(name) => {
            GraphToolError::new(codes::UNKNOWN_TOOL, format!("unknown tool: {name}"))
        }
        FoundryToolError::Failed(message) => GraphToolError::new(FOUNDRY_TOOL_FAILED, message),
    }
}

/// Log arguments without URL userinfo, which may carry a repository token.
fn loggable(arguments: &Value) -> String {
    let text = arguments.to_string();
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(index) = rest.find("://") {
        let (before, after) = rest.split_at(index + 3);
        out.push_str(before);
        let stop = after
            .find(['@', '/', '"', ' ', '\n']);
        match stop {
            Some(at) if after.as_bytes()[at] == b'@' => {
                out.push_str("***@");
                rest = &after[at + 1..];
            }
            _ => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tool, ToolContext as FoundryContext, ToolError, ToolOutput, ToolSpec};
    use graph_agent::ToolContext;
    use serde_json::json;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "echo".to_owned(),
                description: "Echo text.".to_owned(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn call(
            &self,
            arguments: Value,
            context: &FoundryContext,
        ) -> Result<ToolOutput, ToolError> {
            if context.agent_id != "agent" || context.task_id != "task" {
                return Err(ToolError::InvalidArguments("bad context".to_owned()));
            }
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::Failed("missing text".to_owned()))?;
            Ok(ToolOutput::success(text))
        }
    }

    struct ErrorOutputTool;

    #[async_trait]
    impl Tool for ErrorOutputTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "error".to_owned(),
                description: "Return an error output.".to_owned(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn call(
            &self,
            _arguments: Value,
            _context: &FoundryContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::error("boom"))
        }
    }

    fn foundry_registry() -> FoundryToolRegistry {
        let mut registry = FoundryToolRegistry::new();
        registry.register(Arc::new(EchoTool));
        registry.register(Arc::new(ErrorOutputTool));
        registry
    }

    fn context() -> ToolContext {
        ToolContext {
            run_id: "run".to_owned(),
            agent_id: Some("agent".to_owned()),
            task_id: Some("task".to_owned()),
            project_id: Some("project".to_owned()),
        }
    }

    #[tokio::test]
    async fn adapts_definitions_and_calls() {
        let adapted = adapt_registry(&foundry_registry()).unwrap();
        assert_eq!(adapted.names(), vec!["echo", "error"]);
        assert_eq!(adapted.definition("echo").unwrap().name, "echo");

        let output = adapted
            .call("echo", json!({ "text": "hello" }), &context())
            .await
            .unwrap();
        assert_eq!(output, GraphToolOutput::Text("hello".to_owned()));
    }

    #[tokio::test]
    async fn maps_tool_errors_and_error_outputs() {
        let adapted = adapt_registry(&foundry_registry()).unwrap();

        let error = adapted
            .call("echo", json!({}), &context())
            .await
            .unwrap_err();
        assert_eq!(error.code, FOUNDRY_TOOL_FAILED);

        let error = adapted
            .call("error", json!({}), &context())
            .await
            .unwrap_err();
        assert_eq!(error.code, FOUNDRY_TOOL_FAILED);
        assert_eq!(error.message, "boom");

        let error = adapted
            .call("missing", json!({}), &context())
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::UNKNOWN_TOOL);
    }
}
