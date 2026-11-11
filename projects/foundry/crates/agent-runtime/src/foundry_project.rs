//! Read the current project's user-provided name and description.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct ProjectGetTool {
    client: Arc<FoundryClient>,
}

impl ProjectGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for ProjectGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project_get".to_string(),
            description: "Read the current project's name and description.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let project = self
            .client
            .get_project()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        let content = serde_json::to_string_pretty(&project)
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(content))
    }
}
