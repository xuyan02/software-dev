use async_trait::async_trait;
use foundry_agent_runtime::{
    AgentRunRequest, AgentRuntime, Tool, ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSpec,
};
use graph_agent::ScriptedModel;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct RecordTool {
    name: &'static str,
    fail: bool,
    calls: Calls,
}

#[async_trait]
impl Tool for RecordTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name.to_owned(),
            description: "Records calls.".to_owned(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        self.calls
            .lock()
            .expect("lock poisoned")
            .push((self.name.to_owned(), arguments));
        if self.fail {
            return Err(ToolError::Failed("spawn failed".to_owned()));
        }
        Ok(ToolOutput::success("{}"))
    }
}

fn runtime(fail_spawn: bool) -> (AgentRuntime, Calls) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(RecordTool {
        name: "task_spawn_children",
        fail: fail_spawn,
        calls: Arc::clone(&calls),
    }));
    tools.register(Arc::new(RecordTool {
        name: "task_operation",
        fail: false,
        calls: Arc::clone(&calls),
    }));
    let model = Arc::new(ScriptedModel::new(Vec::new()));
    (AgentRuntime::new_daily_check(model, Arc::new(tools)), calls)
}

#[tokio::test]
async fn creates_the_inspection_child_and_reports_dispatched() {
    let (runtime, calls) = runtime(false);
    let mut request = AgentRunRequest::new("daily-check", "task-1", "", "");
    request.task_input = json!({ "scope": "src/lib.rs", "trigger_id": "trigger-1" });

    runtime.run(request).await.unwrap();

    let calls = calls.lock().expect("lock poisoned");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "task_spawn_children");
    assert_eq!(calls[0].1["task_id"], "task-1");
    assert_eq!(calls[0].1["tasks"][0]["workflow"], "design_inspection");
    assert_eq!(
        calls[0].1["tasks"][0]["input"],
        json!({ "scope": "src/lib.rs" })
    );
    assert_eq!(calls[1].0, "task_operation");
    assert_eq!(calls[1].1["operation"], "dispatch:dispatched");
}

#[tokio::test]
async fn reports_failed_when_the_child_cannot_be_created() {
    let (runtime, calls) = runtime(true);
    let request = AgentRunRequest::new("daily-check", "task-2", "", "");

    runtime.run(request).await.unwrap();

    let calls = calls.lock().expect("lock poisoned");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "task_spawn_children");
    assert_eq!(calls[1].0, "task_operation");
    assert_eq!(calls[1].1["operation"], "dispatch:failed");
}

#[tokio::test]
async fn omits_scope_when_the_task_has_none() {
    let (runtime, calls) = runtime(false);
    let request = AgentRunRequest::new("daily-check", "task-3", "", "");

    runtime.run(request).await.unwrap();

    let calls = calls.lock().expect("lock poisoned");
    assert_eq!(calls[0].1["tasks"][0]["input"], json!({}));
}
