//! Tools for design-space merge requests.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct MergeRequestTool {
    client: Arc<FoundryClient>,
}

impl MergeRequestTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for MergeRequestTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "merge_request".to_string(),
            description: "Create, read, merge or clean up a design-space merge request. create_for_task attaches the MR to the current task and reuses the existing MR on retry."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create_for_task", "get_for_task", "merge", "delete_branch"]
                    },
                    "task_id": { "type": "string", "format": "uuid" },
                    "merge_request_id": { "type": "string", "format": "uuid" },
                    "source_branch": { "type": "string" },
                    "target_branch": { "type": "string" },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "branch": { "type": "string" }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing action".to_string()))?;
        let result = match action {
            "create_for_task" => {
                let task_id = parse_uuid(&arguments, "task_id")?;
                let body = json!({
                    "title": required(&arguments, "title")?,
                    "description": arguments.get("description").and_then(Value::as_str).unwrap_or(""),
                    "source_branch": required(&arguments, "source_branch")?,
                    "target_branch": required(&arguments, "target_branch")?,
                });
                self.client
                    .create_merge_request_for_task(task_id, body)
                    .await
            }
            "get_for_task" => {
                let task_id = parse_uuid(&arguments, "task_id")?;
                self.client.get_merge_request_for_task(task_id).await
            }
            "merge" => {
                let merge_request_id = parse_uuid(&arguments, "merge_request_id")?;
                self.client.merge_merge_request(merge_request_id).await
            }
            "delete_branch" => {
                let _project = context.project_id.as_deref().ok_or_else(|| {
                    ToolError::Failed("merge_request requires a project".to_string())
                })?;
                let branch = required(&arguments, "branch")?;
                self.client.delete_design_space_branch(branch).await
            }
            other => {
                return Err(ToolError::InvalidArguments(format!(
                    "unknown merge_request action: {other}"
                )));
            }
        };
        let value = result.map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(value)
    }
}

fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn parse_uuid(arguments: &Value, key: &str) -> Result<Uuid, ToolError> {
    required(arguments, key)?
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("invalid {key}")))
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}
