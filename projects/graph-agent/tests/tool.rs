use async_trait::async_trait;
use graph_agent::{
    Context, Graph, Node, NodeError, Tool, ToolContext, ToolDefinition, ToolError, ToolOutput,
    ToolRegistry, TransitionIndex, codes,
};
use serde_json::{Value, json};
use std::sync::Arc;

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_owned(),
            description: "Echo text.".to_owned(),
            input_schema: json!({
                "type": "object",
                "required": ["text"],
                "properties": { "text": { "type": "string" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let text = arguments
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::new(codes::INVALID_ARGUMENTS, "missing text"))?;
        Ok(ToolOutput::Text(text.to_owned()))
    }
}

struct JsonTool;

#[async_trait]
impl Tool for JsonTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "json".to_owned(),
            description: "Return structured data.".to_owned(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::Json(json!({ "ok": true })))
    }
}

fn registry() -> Arc<ToolRegistry> {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool)).unwrap();
    registry.register(Arc::new(JsonTool)).unwrap();
    Arc::new(registry)
}

#[tokio::test]
async fn registers_definitions_and_calls_tools() {
    let registry = registry();
    let context = ToolContext {
        run_id: "run".to_owned(),
        agent_id: Some("agent".to_owned()),
        task_id: Some("task".to_owned()),
        project_id: None,
    };

    assert_eq!(registry.names(), vec!["echo", "json"]);
    assert_eq!(
        registry
            .definitions(&["echo".to_owned(), "echo".to_owned(), "json".to_owned()])
            .unwrap()
            .iter()
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>(),
        vec!["echo", "json"]
    );

    let output = registry
        .call("echo", json!({ "text": "hello" }), &context)
        .await
        .unwrap();
    assert_eq!(output, ToolOutput::Text("hello".to_owned()));

    let output = registry.call("json", json!({}), &context).await.unwrap();
    assert_eq!(output, ToolOutput::Json(json!({ "ok": true })));

    let error = registry
        .call("missing", json!({}), &context)
        .await
        .unwrap_err();
    assert_eq!(error.code, codes::UNKNOWN_TOOL);
}

#[tokio::test]
async fn rejects_duplicate_and_invalid_calls() {
    let mut registry = ToolRegistry::new();
    registry.register(Arc::new(EchoTool)).unwrap();
    assert!(registry.register(Arc::new(EchoTool)).is_err());

    let error = registry
        .call("echo", json!({}), &ToolContext::default())
        .await
        .unwrap_err();
    assert_eq!(error.code, codes::INVALID_ARGUMENTS);
    assert_eq!(error.message, "missing text");
}

struct CallTool;

#[async_trait]
impl Node for CallTool {
    async fn run(
        &self,
        _input: serde_json::Value,
        context: &mut Context,
    ) -> Result<serde_json::Value, NodeError> {
        let output = context
            .tool_registry()?
            .call(
                "echo",
                json!({ "text": "from-node" }),
                context.tool_context(),
            )
            .await
            .map_err(|error| NodeError::failed(error.to_string()))?;
        serde_json::to_value(output).map_err(|error| NodeError::failed(error.to_string()))
    }

    fn to(&self, _output: &Value, _context: &Context) -> Result<TransitionIndex, NodeError> {
        Ok(TransitionIndex(0))
    }

    fn transition_count(&self) -> usize {
        1
    }
}

#[tokio::test]
async fn deterministic_nodes_call_the_same_registry() {
    let graph = Graph::builder("tool-node")
        .entry("call")
        .exit("done")
        .node("call", CallTool)
        .edge("call", 0, "done")
        .build()
        .unwrap();
    let mut context = Context::new();
    context.set_tool_registry(registry());
    context.set_tool_context(ToolContext {
        run_id: "run".to_owned(),
        ..ToolContext::default()
    });

    let output = graph
        .run(&mut context, serde_json::json!(null))
        .await
        .unwrap();
    assert_eq!(output.exit.as_str(), "done");
    let result: ToolOutput = serde_json::from_value(output.output).unwrap();
    assert_eq!(result, ToolOutput::Text("from-node".to_owned()));
}
