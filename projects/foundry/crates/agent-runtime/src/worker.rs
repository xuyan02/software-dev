//! A minimal worker for system-managed runs.
//!
//! Runs are queued by the system, not by a human. When a run pauses on an
//! Issue comment, the worker stores the checkpoint and resumes it when the
//! comment source reports new human comments. Production replaces the
//! in-memory queue and the HTTP comment source with durable storage and the
//! Foundry event bus.

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

use crate::{
    AgentCheckpoint, AgentRunRequest, AgentRuntime, FoundryClient, RunError, RunOutcome, RunResult,
};

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error(transparent)]
    Run(#[from] RunError),
    #[error("comment source error: {0}")]
    Source(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentInfo {
    pub id: Uuid,
    pub author_kind: String,
    pub body: String,
}

#[async_trait]
pub trait IssueCommentSource: Send + Sync {
    async fn human_comments_after(
        &self,
        issue_id: Uuid,
        after: Option<Uuid>,
    ) -> Result<Vec<CommentInfo>, String>;
}

pub struct FoundryIssueCommentSource {
    client: Arc<FoundryClient>,
}

impl FoundryIssueCommentSource {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl IssueCommentSource for FoundryIssueCommentSource {
    async fn human_comments_after(
        &self,
        issue_id: Uuid,
        after: Option<Uuid>,
    ) -> Result<Vec<CommentInfo>, String> {
        let issue = self
            .client
            .get_issue(issue_id)
            .await
            .map_err(|error| error.to_string())?;
        let comments = issue
            .get("comments")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let after_text = after.map(|id| id.to_string());
        let start = after_text
            .as_deref()
            .and_then(|after| {
                comments.iter().position(|comment| {
                    comment.get("id").and_then(serde_json::Value::as_str) == Some(after)
                })
            })
            .map(|index| index + 1)
            .unwrap_or(0);
        Ok(comments
            .into_iter()
            .skip(start)
            .filter(|comment| {
                comment
                    .get("author")
                    .and_then(|author| author.get("kind"))
                    .and_then(|kind| kind.as_str())
                    == Some("human")
            })
            .filter_map(|comment| {
                Some(CommentInfo {
                    id: comment.get("id")?.as_str()?.parse().ok()?,
                    author_kind: "human".to_string(),
                    body: comment.get("body")?.as_str()?.to_string(),
                })
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct WorkerResult {
    pub agent_id: String,
    pub task_id: String,
    pub result: RunResult,
}

struct WaitingRun {
    checkpoint: AgentCheckpoint,
    last_comment_id: Option<Uuid>,
}

pub struct AgentWorker {
    runtime: Arc<AgentRuntime>,
    source: Option<Arc<dyn IssueCommentSource>>,
    queued: VecDeque<AgentRunRequest>,
    waiting: HashMap<Uuid, Vec<WaitingRun>>,
    results: Vec<WorkerResult>,
}

impl AgentWorker {
    pub fn new(runtime: Arc<AgentRuntime>, source: Option<Arc<dyn IssueCommentSource>>) -> Self {
        Self {
            runtime,
            source,
            queued: VecDeque::new(),
            waiting: HashMap::new(),
            results: Vec::new(),
        }
    }

    /// The system enqueues work; humans do not call this in production.
    pub fn enqueue(&mut self, request: AgentRunRequest) {
        self.queued.push_back(request);
    }

    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }

    pub fn waiting_len(&self) -> usize {
        self.waiting.values().map(Vec::len).sum()
    }

    pub fn results(&self) -> &[WorkerResult] {
        &self.results
    }

    /// Execute one queued run. Returns false when the queue is empty.
    pub async fn run_once(&mut self) -> Result<bool, WorkerError> {
        let Some(request) = self.queued.pop_front() else {
            return Ok(false);
        };
        let agent_id = request.agent_id.clone();
        let task_id = request.task_id.clone();
        let outcome = self.runtime.run(request).await?;
        self.handle_outcome(outcome, agent_id, task_id);
        Ok(true)
    }

    /// Resume waiting runs when new human comments arrive.
    pub async fn poll_comments(&mut self) -> Result<usize, WorkerError> {
        let Some(source) = &self.source else {
            return Ok(0);
        };
        let issue_ids: Vec<Uuid> = self.waiting.keys().copied().collect();
        let mut resumed = 0;
        for issue_id in issue_ids {
            let runs = self.waiting.remove(&issue_id).unwrap_or_default();
            let mut keep = Vec::new();
            for run in runs {
                let comments = source
                    .human_comments_after(issue_id, run.last_comment_id)
                    .await
                    .map_err(WorkerError::Source)?;
                if comments.is_empty() {
                    keep.push(run);
                    continue;
                }
                let last_comment_id = comments.last().map(|comment| comment.id);
                let text = comments
                    .iter()
                    .map(|comment| comment.body.clone())
                    .collect::<Vec<_>>()
                    .join("\n---\n");
                let agent_id = run.checkpoint.agent_id.clone();
                let task_id = run.checkpoint.task_id.clone();
                let outcome = self.runtime.resume(run.checkpoint, text, None).await?;
                resumed += 1;
                match outcome {
                    RunOutcome::Finished(result) => {
                        self.results.push(WorkerResult {
                            agent_id,
                            task_id,
                            result,
                        });
                    }
                    RunOutcome::WaitingForIssueComment { pause, checkpoint } => {
                        keep.push(WaitingRun {
                            checkpoint,
                            last_comment_id: last_comment_id.or(pause.last_comment_id),
                        });
                    }
                }
            }
            if !keep.is_empty() {
                self.waiting.insert(issue_id, keep);
            }
        }
        Ok(resumed)
    }

    /// Reference loop: run queued work, poll for comments, sleep.
    pub async fn run_forever(&mut self, interval: Duration) -> Result<(), WorkerError> {
        loop {
            while self.run_once().await? {}
            self.poll_comments().await?;
            tokio::time::sleep(interval).await;
        }
    }

    fn handle_outcome(&mut self, outcome: RunOutcome, agent_id: String, task_id: String) {
        match outcome {
            RunOutcome::Finished(result) => self.results.push(WorkerResult {
                agent_id,
                task_id,
                result,
            }),
            RunOutcome::WaitingForIssueComment { pause, checkpoint } => {
                self.waiting
                    .entry(pause.issue_id)
                    .or_default()
                    .push(WaitingRun {
                        checkpoint,
                        last_comment_id: pause.last_comment_id,
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Budget, InMemoryTrajectory, ModelResponse, PauseRequest, ScriptedModel, Tool, ToolContext,
        ToolError, ToolOutput, ToolRegistry, ToolSpec,
    };
    use serde_json::json;
    use std::sync::Mutex;

    struct PausingTool;

    #[async_trait]
    impl Tool for PausingTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "pause".to_string(),
                description: "Pause".to_string(),
                input_schema: json!({ "type": "object" }),
            }
        }

        async fn call(
            &self,
            _arguments: serde_json::Value,
            _context: &ToolContext,
        ) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::success("posted"))
        }

        fn pause_after_call(
            &self,
            _arguments: &serde_json::Value,
            _output: &ToolOutput,
        ) -> Option<PauseRequest> {
            Some(PauseRequest {
                issue_id: Uuid::nil(),
                last_comment_id: None,
            })
        }
    }

    struct MockSource {
        comments: Mutex<Vec<CommentInfo>>,
    }

    #[async_trait]
    impl IssueCommentSource for MockSource {
        async fn human_comments_after(
            &self,
            _issue_id: Uuid,
            _after: Option<Uuid>,
        ) -> Result<Vec<CommentInfo>, String> {
            let mut comments = self.comments.lock().unwrap();
            Ok(std::mem::take(&mut *comments))
        }
    }

    #[tokio::test]
    async fn worker_pauses_and_resumes_on_a_human_comment() {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(PausingTool));
        let model = Arc::new(ScriptedModel::new(vec![
            ModelResponse::tool_call("pause", json!({})),
            ModelResponse::final_answer("done"),
            ModelResponse::final_answer("answered"),
        ]));
        let runtime = Arc::new(AgentRuntime::new(
            model.clone(),
            Arc::new(tools),
            Arc::new(InMemoryTrajectory::new()),
            Budget::default(),
        ));
        let source = Arc::new(MockSource {
            comments: Mutex::new(Vec::new()),
        });
        let mut worker = AgentWorker::new(runtime, Some(source.clone()));
        worker.enqueue(AgentRunRequest::new("agent", "task", "system", "user"));

        assert!(worker.run_once().await.unwrap());
        assert_eq!(worker.waiting_len(), 1);
        assert!(worker.results().is_empty());

        source.comments.lock().unwrap().push(CommentInfo {
            id: Uuid::now_v7(),
            author_kind: "human".to_string(),
            body: "here is the answer".to_string(),
        });
        assert_eq!(worker.poll_comments().await.unwrap(), 1);
        assert_eq!(worker.waiting_len(), 0);
        assert_eq!(
            worker.results()[0].result.final_message.as_deref(),
            Some("answered")
        );
        assert!(model.requests().iter().any(|request| {
            request
                .messages
                .iter()
                .any(|message| message.content.contains("here is the answer"))
        }));
    }
}
