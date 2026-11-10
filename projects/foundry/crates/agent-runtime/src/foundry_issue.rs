//! Issue and TaskLink tools for the clarification agent.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct TaskLinksListTool {
    client: Arc<FoundryClient>,
}

impl TaskLinksListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskLinksListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_links_list".to_string(),
            description: "List the links attached to a Task, including its clarification Issue."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id"],
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "task_id": { "type": "string", "format": "uuid" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let task_id = parse_uuid(&arguments, "task_id")?;
        let project = arguments
            .get("project_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let links = self
            .client
            .list_task_links_scoped(project.as_deref(), task_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(links)
    }
}

pub struct IssueGetTool {
    client: Arc<FoundryClient>,
}

impl IssueGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for IssueGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "issue_get".to_string(),
            description: "Read an Issue and its comments.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["issue_id"],
                "properties": { "issue_id": { "type": "string", "format": "uuid" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let issue_id = parse_uuid(&arguments, "issue_id")?;
        let issue = self
            .client
            .get_issue(issue_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(issue)
    }
}

pub struct IssueCommentTool {
    client: Arc<FoundryClient>,
}

impl IssueCommentTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for IssueCommentTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "issue_comment".to_string(),
            description: "Comment on an Issue. After a question that needs a human reply, finish \
                          your turn; a later human comment activates the agent again. A status \
                          or handoff comment can be followed by more tool calls."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["issue_id", "body"],
                "properties": {
                    "issue_id": { "type": "string", "format": "uuid" },
                    "body": { "type": "string" },
                    "internal": { "type": "boolean", "default": false }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let issue_id = parse_uuid(&arguments, "issue_id")?;
        let body = arguments
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing body".to_string()))?;
        let internal = arguments
            .get("internal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let issue = self
            .client
            .comment_issue(issue_id, body, internal)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(issue)
    }
}

fn parse_uuid(arguments: &Value, key: &str) -> Result<Uuid, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))?
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("invalid {key}")))
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}
