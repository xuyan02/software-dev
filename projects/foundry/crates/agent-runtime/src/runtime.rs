//! The agent loop: model → tool calls → observations → final answer.
//!
//! The loop can pause after a tool that needs human input. The worker persists
//! `AgentCheckpoint`, releases its lease, and resumes when a human comment
//! arrives. The loop itself never owns a thread while waiting.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;

use crate::{
    Budget, ContextBundle, Message, ModelGateway, ModelRequest, ModelResponse, PauseRequest,
    Pricing, RunError, StepKind, StepRecord, ToolCall, ToolContext, ToolOutput, ToolRegistry,
    TrajectorySink, Usage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    Completed,
    MaxSteps,
    BudgetExceeded,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct AgentRunRequest {
    pub agent_id: String,
    pub task_id: String,
    pub system_prompt: String,
    pub user_message: String,
    pub context: ContextBundle,
    pub cancellation: Option<Cancellation>,
}

impl AgentRunRequest {
    pub fn new(
        agent_id: impl Into<String>,
        task_id: impl Into<String>,
        system_prompt: impl Into<String>,
        user_message: impl Into<String>,
    ) -> Self {
        Self {
            agent_id: agent_id.into(),
            task_id: task_id.into(),
            system_prompt: system_prompt.into(),
            user_message: user_message.into(),
            context: ContextBundle::default(),
            cancellation: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RunResult {
    pub final_message: Option<String>,
    pub steps: u32,
    pub usage: Usage,
    pub stop_reason: StopReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// What a run returns: a terminal result or a checkpoint waiting for a human
/// comment on an Issue.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum RunOutcome {
    Finished(RunResult),
    WaitingForIssueComment {
        pause: PauseRequest,
        checkpoint: AgentCheckpoint,
    },
}

/// Everything needed to continue a paused run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCheckpoint {
    pub messages: Vec<Message>,
    pub usage: Usage,
    pub steps: u32,
    pub tool_calls: u32,
    pub remaining_calls: Vec<ToolCall>,
    pub agent_id: String,
    pub task_id: String,
    pub elapsed_ms: u64,
    pub pause: PauseRequest,
}

struct RuntimeState {
    messages: Vec<Message>,
    usage: Usage,
    steps: u32,
    tool_calls: u32,
    agent_id: String,
    task_id: String,
    started: Instant,
    prior_elapsed_ms: u64,
    /// A tool asked for human input, but the model still owns the turn. The
    /// run waits only after the model finishes its current turn.
    pause: Option<PauseRequest>,
}

#[derive(Debug, Clone)]
pub struct Cancellation {
    receiver: watch::Receiver<bool>,
}

impl Cancellation {
    pub fn new() -> (Self, watch::Sender<bool>) {
        let (sender, receiver) = watch::channel(false);
        (Self { receiver }, sender)
    }

    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    /// Resolve after the sender flips the flag. Returns immediately when the
    /// flag is already set.
    pub async fn cancelled(&mut self) {
        if *self.receiver.borrow() {
            return;
        }
        let _ = self.receiver.changed().await;
    }
}

pub struct AgentRuntime {
    model: Arc<dyn ModelGateway>,
    tools: Arc<ToolRegistry>,
    trajectory: Arc<dyn TrajectorySink>,
    budget: Budget,
    pricing: Option<Pricing>,
}

impl AgentRuntime {
    pub fn new(
        model: Arc<dyn ModelGateway>,
        tools: Arc<ToolRegistry>,
        trajectory: Arc<dyn TrajectorySink>,
        budget: Budget,
    ) -> Self {
        Self {
            model,
            tools,
            trajectory,
            budget,
            pricing: None,
        }
    }

    pub fn with_pricing(mut self, pricing: Pricing) -> Self {
        self.pricing = Some(pricing);
        self
    }

    pub async fn run(&self, request: AgentRunRequest) -> Result<RunOutcome, RunError> {
        let AgentRunRequest {
            agent_id,
            task_id,
            system_prompt,
            user_message,
            context,
            cancellation,
        } = request;
        let mut messages = vec![Message::system(system_prompt)];
        let rendered_context = context.render();
        let user_message = if rendered_context.is_empty() {
            user_message
        } else {
            format!("{rendered_context}\n\n{user_message}")
        };
        messages.push(Message::user(user_message));
        let state = RuntimeState {
            messages,
            usage: Usage::default(),
            steps: 0,
            tool_calls: 0,
            agent_id,
            task_id,
            started: Instant::now(),
            prior_elapsed_ms: 0,
            pause: None,
        };
        self.run_loop(state, Vec::new(), cancellation).await
    }

    /// Convenience for callers that do not expect a pause.
    pub async fn run_to_completion(&self, request: AgentRunRequest) -> Result<RunResult, RunError> {
        match self.run(request).await? {
            RunOutcome::Finished(result) => Ok(result),
            RunOutcome::WaitingForIssueComment { .. } => Err(RunError::UnexpectedPause),
        }
    }

    /// Resume a paused run with the new human comments on its Issue.
    pub async fn resume(
        &self,
        checkpoint: AgentCheckpoint,
        human_comments: impl Into<String>,
        cancellation: Option<Cancellation>,
    ) -> Result<RunOutcome, RunError> {
        let AgentCheckpoint {
            mut messages,
            usage,
            steps,
            tool_calls,
            remaining_calls,
            agent_id,
            task_id,
            elapsed_ms,
            ..
        } = checkpoint;
        messages.push(Message::user(format!(
            "New human comments on the clarification Issue:\n{}",
            human_comments.into()
        )));
        let state = RuntimeState {
            messages,
            usage,
            steps,
            tool_calls,
            agent_id,
            task_id,
            started: Instant::now(),
            prior_elapsed_ms: elapsed_ms,
            pause: None,
        };
        self.run_loop(state, remaining_calls, cancellation).await
    }

    async fn run_loop(
        &self,
        mut state: RuntimeState,
        mut pending_calls: Vec<ToolCall>,
        mut cancellation: Option<Cancellation>,
    ) -> Result<RunOutcome, RunError> {
        loop {
            if let Some(outcome) = self.check_stop(&state, cancellation.as_ref()) {
                return Ok(outcome);
            }
            if !pending_calls.is_empty() {
                let calls = std::mem::take(&mut pending_calls);
                if let Some(outcome) = self
                    .execute_calls(&mut state, calls, &mut cancellation)
                    .await?
                {
                    return Ok(outcome);
                }
                continue;
            }

            let model_request = ModelRequest {
                messages: state.messages.clone(),
                tools: self.tools.specs(),
                max_tokens: None,
                temperature: None,
            };
            let response = match cancellation.as_mut() {
                Some(cancellation) => {
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            return Ok(self.stop_outcome(StopReason::Cancelled, &state));
                        }
                        response = self.model.complete(model_request) => response?,
                    }
                }
                None => self.model.complete(model_request).await?,
            };
            let ModelResponse {
                content,
                reasoning_content,
                tool_calls,
                usage: response_usage,
                ..
            } = response;
            state.usage += response_usage;
            state.steps += 1;
            self.record(StepRecord {
                index: state.steps,
                kind: StepKind::Model,
                name: None,
                input: None,
                output: content.clone(),
                reasoning: reasoning_content.clone(),
                usage: response_usage,
                elapsed_ms: elapsed_ms(&state),
                error: None,
            })
            .await?;

            if tool_calls.is_empty() {
                let final_message = content.unwrap_or_default();
                self.record(StepRecord {
                    index: state.steps,
                    kind: StepKind::Final,
                    name: None,
                    input: None,
                    output: Some(final_message.clone()),
                    reasoning: reasoning_content.clone(),
                    usage: response_usage,
                    elapsed_ms: elapsed_ms(&state),
                    error: None,
                })
                .await?;
                if let Some(pause) = state.pause.clone() {
                    let elapsed = elapsed_ms(&state);
                    state.messages.push(Message::assistant_with_reasoning(
                        final_message,
                        Vec::new(),
                        reasoning_content,
                    ));
                    let checkpoint = AgentCheckpoint {
                        messages: state.messages,
                        usage: state.usage,
                        steps: state.steps,
                        tool_calls: state.tool_calls,
                        remaining_calls: Vec::new(),
                        agent_id: state.agent_id,
                        task_id: state.task_id,
                        elapsed_ms: elapsed,
                        pause: pause.clone(),
                    };
                    return Ok(RunOutcome::WaitingForIssueComment { pause, checkpoint });
                }
                return Ok(RunOutcome::Finished(RunResult {
                    final_message: Some(final_message),
                    steps: state.steps,
                    usage: state.usage,
                    stop_reason: StopReason::Completed,
                    cost_usd: self.pricing.map(|pricing| state.usage.cost_usd(&pricing)),
                }));
            }

            state.messages.push(Message::assistant_with_reasoning(
                content.unwrap_or_default(),
                tool_calls.clone(),
                reasoning_content,
            ));
            pending_calls = tool_calls;
        }
    }

    async fn execute_calls(
        &self,
        state: &mut RuntimeState,
        calls: Vec<ToolCall>,
        cancellation: &mut Option<Cancellation>,
    ) -> Result<Option<RunOutcome>, RunError> {
        for call in &calls {
            if let Some(outcome) = self.check_stop(state, cancellation.as_ref()) {
                return Ok(Some(outcome));
            }
            if self
                .budget
                .max_tool_calls
                .is_some_and(|max_calls| state.tool_calls >= max_calls)
            {
                return Ok(Some(self.stop_outcome(StopReason::BudgetExceeded, state)));
            }
            state.tool_calls += 1;
            state.steps += 1;
            let tool_started = Instant::now();
            let output = self
                .tools
                .call(
                    &call.name,
                    call.arguments.clone(),
                    &ToolContext {
                        agent_id: state.agent_id.clone(),
                        task_id: state.task_id.clone(),
                    },
                )
                .await;
            self.log_tool_call(call, &output, tool_started.elapsed());
            self.record(StepRecord {
                index: state.steps,
                kind: StepKind::Tool,
                name: Some(call.name.clone()),
                input: Some(call.arguments.clone()),
                output: Some(output.content.clone()),
                reasoning: None,
                usage: Usage::default(),
                elapsed_ms: elapsed_ms(state),
                error: output.is_error.then(|| output.content.clone()),
            })
            .await?;
            state
                .messages
                .push(Message::tool(call.id.clone(), output.content.clone()));

            let pause = self
                .tools
                .get(&call.name)
                .and_then(|tool| tool.pause_after_call(&call.arguments, &output));
            if let Some(pause) = pause {
                // Do not stop the model here. Record the interaction request;
                // the run waits only after the model finishes its turn.
                state.pause = Some(pause);
            }
        }
        Ok(None)
    }

    fn check_stop(
        &self,
        state: &RuntimeState,
        cancellation: Option<&Cancellation>,
    ) -> Option<RunOutcome> {
        if cancellation.is_some_and(Cancellation::is_cancelled) {
            return Some(self.stop_outcome(StopReason::Cancelled, state));
        }
        if self
            .budget
            .max_steps
            .is_some_and(|max_steps| state.steps >= max_steps)
        {
            return Some(self.stop_outcome(StopReason::MaxSteps, state));
        }
        if self
            .budget
            .max_duration
            .is_some_and(|max| elapsed_ms(state) > max.as_millis() as u64)
        {
            return Some(self.stop_outcome(StopReason::BudgetExceeded, state));
        }
        if self.budget.tokens_exceeded(&state.usage) {
            return Some(self.stop_outcome(StopReason::BudgetExceeded, state));
        }
        None
    }

    fn stop_outcome(&self, stop_reason: StopReason, state: &RuntimeState) -> RunOutcome {
        RunOutcome::Finished(RunResult {
            final_message: None,
            steps: state.steps,
            usage: state.usage,
            stop_reason,
            cost_usd: self.pricing.map(|pricing| state.usage.cost_usd(&pricing)),
        })
    }

    async fn record(&self, step: StepRecord) -> Result<(), RunError> {
        self.trajectory
            .record(&step)
            .await
            .map_err(|error| RunError::Trajectory(error.to_string()))
    }

    fn log_tool_call(&self, call: &ToolCall, output: &ToolOutput, elapsed: std::time::Duration) {
        let arguments = truncate(&call.arguments.to_string(), 2000);
        let content = truncate(&output.content, 4000);
        if output.is_error {
            tracing::warn!(
                tool = %call.name,
                elapsed_ms = elapsed.as_millis() as u64,
                arguments = %arguments,
                output = %content,
                "tool call failed"
            );
        } else {
            tracing::info!(
                tool = %call.name,
                elapsed_ms = elapsed.as_millis() as u64,
                arguments = %arguments,
                output = %content,
                "tool call"
            );
        }
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

fn elapsed_ms(state: &RuntimeState) -> u64 {
    state.prior_elapsed_ms + state.started.elapsed().as_millis() as u64
}
