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
        let links = self
            .client
            .list_task_links(task_id)
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
            description: "Comment on an Issue. Use it for questions; finish your turn after \
                          posting. A later human comment on the same Issue activates the agent \
                          again."
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

pub struct IssueCreateTool {
    client: Arc<FoundryClient>,
}

impl IssueCreateTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for IssueCreateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "issue_create".to_string(),
            description: "Create a project Issue to report a defect, design problem, requirement problem or other finding."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["title", "description", "kind"],
                "properties": {
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "kind": { "type": "string", "enum": ["feedback", "bug", "question", "idea", "task_request", "incident"] },
                    "severity": { "type": "string", "enum": ["s1", "s2", "s3", "s4"] },
                    "area": { "type": "string" },
                    "labels": { "type": "array", "items": { "type": "string" } },
                    "environment": { "type": "string" },
                    "affected_versions": { "type": "array", "items": { "type": "string" } },
                    "reproduction": { "type": "string" },
                    "expected": { "type": "string" },
                    "actual": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let mut body = arguments;
        if let Some(object) = body.as_object_mut() {
            object.retain(|_, value| !value.is_null());
        }
        let issue = self
            .client
            .create_issue(body)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(issue)
    }
}

pub struct IssueTransitionTool {
    client: Arc<FoundryClient>,
}

impl IssueTransitionTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for IssueTransitionTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "issue_transition".to_string(),
            description: "Move an Issue to another status. Agents may start work or resolve an Issue; rejection, duplicate and closure are human dispositions."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["issue_id", "to"],
                "properties": {
                    "issue_id": { "type": "string", "format": "uuid" },
                    "to": { "type": "string", "enum": ["in_progress", "resolved", "rejected", "duplicate", "closed", "open"] },
                    "resolution": { "type": "string" },
                    "duplicate_of": { "type": "string", "format": "uuid" }
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
        let to = arguments
            .get("to")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing to".to_string()))?;
        let resolution = arguments.get("resolution").and_then(Value::as_str);
        let duplicate_of = arguments
            .get("duplicate_of")
            .and_then(Value::as_str)
            .map(|value| value.parse::<Uuid>())
            .transpose()
            .map_err(|_| ToolError::InvalidArguments("invalid duplicate_of".to_string()))?;
        let issue = self
            .client
            .transition_issue(issue_id, to, resolution, duplicate_of)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(issue)
    }
}


pub struct IssueListTool {
    client: Arc<FoundryClient>,
}

impl IssueListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for IssueListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "issue_list".to_string(),
            description: "List project Issues. Use it to check for duplicates before creating a new Issue."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let issues = self
            .client
            .list_issues()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(issues)
    }
}
