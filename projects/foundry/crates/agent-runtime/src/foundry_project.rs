//! Read the current project.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct ProjectTool {
    client: Arc<FoundryClient>,
}

impl ProjectTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project".to_string(),
            description: "Read project information. action=get returns the current project, \
                          including its git repository, git branch, design-space URL and \
                          design-space branch."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": { "type": "string", "enum": ["get"] }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let action = arguments
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing action".to_string()))?;
        match action {
            "get" => {
                let detail = self
                    .client
                    .get_project()
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                // `/api/v1/projects/{id}` returns `{project, members}`; the
                // tool exposes the project itself so callers see its fields
                // at the top level.
                let project = detail.get("project").cloned().unwrap_or(detail);
                let content = serde_json::to_string_pretty(&project)
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                Ok(ToolOutput::success(content))
            }
            other => Err(ToolError::InvalidArguments(format!(
                "unknown project action: {other}"
            ))),
        }
    }
}
