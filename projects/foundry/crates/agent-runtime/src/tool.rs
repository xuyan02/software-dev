//! Tool contract, registry and execution.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::{ToolError, ToolSpec};

/// A tool that marks the run as waiting for human input after its output is
/// recorded.
///
/// The model keeps ownership of the current turn: it may continue with more
/// tool calls or finish. When it finishes, the worker persists the checkpoint
/// and resumes when new human input arrives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PauseRequest {
    pub issue_id: Uuid,
    pub last_comment_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolContext {
    pub agent_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError>;

    /// Record that the run will wait for human input after this call. The
    /// model may continue its turn; the wait starts when the model finishes.
    /// The default is to continue normally.
    fn pause_after_call(&self, _arguments: &Value, _output: &ToolOutput) -> Option<PauseRequest> {
        None
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> &mut Self {
        let name = tool.spec().name;
        self.tools.insert(name, tool);
        self
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// Execute a tool. Tool-level failures are returned as an error output so
    /// the model can see them and decide what to do; only unknown tools are
    /// reported without invoking anything.
    pub async fn call(&self, name: &str, arguments: Value, context: &ToolContext) -> ToolOutput {
        let Some(tool) = self.get(name) else {
            return ToolOutput::error(format!("unknown tool: {name}"));
        };
        match tool.call(arguments, context).await {
            Ok(output) => output,
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }
}
