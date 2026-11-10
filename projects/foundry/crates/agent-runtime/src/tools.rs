//! Built-in tools for Phase 0.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "echo".to_string(),
            description: "Echo the provided text. Useful for smoke tests.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let text = arguments
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing text".to_string()))?;
        Ok(ToolOutput::success(text))
    }
}

/// Phase 0 read-only requirements tool. Phase 1 replaces the static document
/// with a Foundry MCP/HTTP call.
pub struct RequirementsGetTool {
    document: Value,
}

impl RequirementsGetTool {
    pub fn new(document: Value) -> Self {
        Self { document }
    }
}

#[async_trait]
impl Tool for RequirementsGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_get".to_string(),
            description: "Read the project's structured requirements document.".to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let content = serde_json::to_string_pretty(&self.document)
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(content))
    }
}
