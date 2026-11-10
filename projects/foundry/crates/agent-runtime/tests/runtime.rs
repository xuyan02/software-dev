use async_trait::async_trait;
use foundry_agent_runtime::{
    AgentRunRequest, AgentRuntime, Answer, Budget, Cancellation, EchoTool, InMemoryTrajectory,
    ModelResponse, PauseRequest, RunOutcome, ScriptedModel, StepKind, StopReason, Tool,
    ToolContext, ToolError, ToolOutput, ToolRegistry, ToolSpec, Usage,
};
use serde_json::{Value, json};
use std::sync::Arc;

struct FailingTool;

#[async_trait]
impl Tool for FailingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fail".to_string(),
            description: "Always fails.".to_string(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        Err(ToolError::Failed("boom".to_string()))
    }
}

fn runtime(
    responses: Vec<ModelResponse>,
    tools: ToolRegistry,
    budget: Budget,
) -> (AgentRuntime, Arc<ScriptedModel>, Arc<InMemoryTrajectory>) {
    let model = Arc::new(ScriptedModel::new(responses));
    let trajectory = Arc::new(InMemoryTrajectory::new());
    let runtime = AgentRuntime::new(model.clone(), Arc::new(tools), trajectory.clone(), budget);
    (runtime, model, trajectory)
}

#[tokio::test]
async fn tool_call_then_final_answer() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    let (runtime, _model, trajectory) = runtime(
        vec![
            ModelResponse::tool_call("echo", json!({ "text": "hello" })),
            ModelResponse::final_answer("done"),
        ],
        tools,
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.stop_reason, StopReason::Completed);
    assert_eq!(result.final_message.as_deref(), Some("done"));
    assert_eq!(result.steps, 3);

    let steps = trajectory.steps();
    assert_eq!(steps.len(), 4);
    assert_eq!(steps[0].kind, StepKind::Model);
    assert_eq!(steps[1].kind, StepKind::Tool);
    assert_eq!(steps[1].name.as_deref(), Some("echo"));
    assert_eq!(steps[1].output.as_deref(), Some("hello"));
    assert_eq!(steps[1].error, None);
    assert_eq!(steps[2].kind, StepKind::Model);
    assert_eq!(steps[3].kind, StepKind::Final);
}

#[tokio::test]
async fn tool_errors_are_fed_back_to_the_model() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(FailingTool));
    let (runtime, _model, trajectory) = runtime(
        vec![
            ModelResponse::tool_call("fail", json!({})),
            ModelResponse::final_answer("recovered"),
        ],
        tools,
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.final_message.as_deref(), Some("recovered"));
    let tool_step = trajectory
        .steps()
        .into_iter()
        .find(|step| step.kind == StepKind::Tool)
        .expect("tool step");
    assert_eq!(tool_step.output.as_deref(), Some("tool failed: boom"));
    assert_eq!(tool_step.error.as_deref(), Some("tool failed: boom"));
}

#[tokio::test]
async fn unknown_tools_are_reported_to_the_model() {
    let (runtime, _model, trajectory) = runtime(
        vec![
            ModelResponse::tool_call("missing", json!({})),
            ModelResponse::final_answer("handled"),
        ],
        ToolRegistry::new(),
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.final_message.as_deref(), Some("handled"));
    let tool_step = trajectory
        .steps()
        .into_iter()
        .find(|step| step.kind == StepKind::Tool)
        .expect("tool step");
    assert_eq!(tool_step.output.as_deref(), Some("unknown tool: missing"));
    assert!(tool_step.error.is_some());
}

#[tokio::test]
async fn max_steps_stops_the_loop() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    let budget = Budget {
        max_steps: 2,
        ..Budget::default()
    };
    let (runtime, _model, _trajectory) = runtime(
        vec![ModelResponse::tool_call("echo", json!({ "text": "again" }))],
        tools,
        budget,
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.stop_reason, StopReason::MaxSteps);
    assert_eq!(result.steps, 2);
}

#[tokio::test]
async fn token_budget_stops_the_loop() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(EchoTool));
    let budget = Budget {
        max_input_tokens: 10,
        max_steps: 10,
        ..Budget::default()
    };
    let (runtime, _model, _trajectory) = runtime(
        vec![
            ModelResponse::tool_call("echo", json!({ "text": "x" })).with_usage(Usage::new(100, 0)),
            ModelResponse::final_answer("never reached"),
        ],
        tools,
        budget,
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.stop_reason, StopReason::BudgetExceeded);
}

#[tokio::test]
async fn cancellation_before_the_run_skips_the_model() {
    let (cancellation, sender) = Cancellation::new();
    sender.send(true).unwrap();
    let (runtime, model, _trajectory) = runtime(
        vec![ModelResponse::final_answer("should not run")],
        ToolRegistry::new(),
        Budget::default(),
    );

    let mut request = AgentRunRequest::new("agent", "task", "system", "user");
    request.cancellation = Some(cancellation);
    let result = runtime.run_to_completion(request).await.unwrap();

    assert_eq!(result.stop_reason, StopReason::Cancelled);
    assert_eq!(result.steps, 0);
    assert!(model.requests().is_empty());
}

#[tokio::test]
async fn context_is_rendered_into_the_user_message() {
    let (runtime, model, _trajectory) = runtime(
        vec![ModelResponse::final_answer("ok")],
        ToolRegistry::new(),
        Budget::default(),
    );
    let mut request = AgentRunRequest::new("agent", "task", "system", "user request");
    request
        .context
        .push("requirement", "req-1", "users must be able to search");

    runtime.run_to_completion(request).await.unwrap();

    let requests = model.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].messages[0].content, "system");
    let user = &requests[0].messages[1].content;
    assert!(user.contains("[requirement] req-1"));
    assert!(user.ends_with("user request"));
}

#[tokio::test]
async fn final_answer_without_tools_completes_in_one_step() {
    let (runtime, _model, trajectory) = runtime(
        vec![ModelResponse::final_answer("done")],
        ToolRegistry::new(),
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(result.stop_reason, StopReason::Completed);
    assert_eq!(result.steps, 1);
    assert_eq!(trajectory.steps().len(), 2);
}

struct FixedInteraction {
    answer: Answer,
    questions: std::sync::Mutex<Vec<foundry_agent_runtime::Question>>,
}

#[async_trait]
impl foundry_agent_runtime::HumanInteraction for FixedInteraction {
    async fn ask(
        &self,
        question: foundry_agent_runtime::Question,
    ) -> Result<foundry_agent_runtime::Answer, foundry_agent_runtime::InteractionError> {
        self.questions.lock().unwrap().push(question);
        Ok(self.answer.clone())
    }
}

#[tokio::test]
async fn human_ask_tool_runs_inside_the_loop() {
    let interaction = Arc::new(FixedInteraction {
        answer: foundry_agent_runtime::Answer::text("700 件/月"),
        questions: std::sync::Mutex::new(Vec::new()),
    });
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(foundry_agent_runtime::HumanAskTool::new(
        interaction.clone(),
    )));
    let (runtime, _model, trajectory) = runtime(
        vec![
            ModelResponse::tool_call("human_ask", json!({ "prompt": "成功指标的目标值是多少？" })),
            ModelResponse::final_answer("目标值已确认：700 件/月"),
        ],
        tools,
        Budget::default(),
    );

    let result = runtime
        .run_to_completion(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();

    assert_eq!(
        result.final_message.as_deref(),
        Some("目标值已确认：700 件/月")
    );
    let tool_step = trajectory
        .steps()
        .into_iter()
        .find(|step| step.kind == StepKind::Tool)
        .expect("tool step");
    assert_eq!(tool_step.name.as_deref(), Some("human_ask"));
    assert_eq!(tool_step.output.as_deref(), Some("700 件/月"));
    let questions = interaction.questions.lock().unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].prompt, "成功指标的目标值是多少？");
}

struct PausingTool;

#[async_trait]
impl Tool for PausingTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "pause".to_string(),
            description: "Post a question and pause.".to_string(),
            input_schema: json!({ "type": "object" }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::success("posted"))
    }

    fn pause_after_call(&self, _arguments: &Value, _output: &ToolOutput) -> Option<PauseRequest> {
        Some(PauseRequest {
            issue_id: uuid::Uuid::nil(),
            last_comment_id: None,
        })
    }
}

#[tokio::test]
async fn pauses_and_resumes_with_a_human_comment() {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(PausingTool));
    let (runtime, model, _trajectory) = runtime(
        vec![
            ModelResponse::tool_call("pause", json!({})),
            ModelResponse::final_answer("asked"),
            ModelResponse::final_answer("resumed"),
        ],
        tools,
        Budget::default(),
    );

    let outcome = runtime
        .run(AgentRunRequest::new("agent", "task", "system", "user"))
        .await
        .unwrap();
    let RunOutcome::WaitingForIssueComment { checkpoint, .. } = outcome else {
        panic!("expected the run to pause");
    };

    let outcome = runtime
        .resume(checkpoint, "human reply", None)
        .await
        .unwrap();
    let RunOutcome::Finished(result) = outcome else {
        panic!("expected the run to finish");
    };
    assert_eq!(result.final_message.as_deref(), Some("resumed"));

    let requests = model.requests();
    assert!(requests.iter().any(|request| {
        request
            .messages
            .iter()
            .any(|message| message.content.contains("human reply"))
    }));
}
