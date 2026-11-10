//! Built-in tools for Phase 0.

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

/// Lists the skill documents available to `load_skill`.
pub struct ListSkillsTool;

#[async_trait]
impl Tool for ListSkillsTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "list_skills".to_string(),
            description: "List the skill documents that can be loaded with load_skill."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let entries: Vec<Value> = crate::skills()
            .iter()
            .map(|skill| {
                json!({
                    "name": skill.name,
                    "title": skill.title,
                    "description": skill.description,
                })
            })
            .collect();
        let content = serde_json::to_string_pretty(&entries)
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(content))
    }
}

/// Loads one skill document by name.
pub struct LoadSkillTool;

#[async_trait]
impl Tool for LoadSkillTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "load_skill".to_string(),
            description: "Load a skill document by name. Use list_skills to discover names."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = arguments
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing name".to_string()))?;
        let Some(skill) = crate::skill(name) else {
            let available: Vec<&str> = crate::skills()
                .iter()
                .map(|entry| entry.name)
                .collect();
            return Err(ToolError::InvalidArguments(format!(
                "unknown skill {name:?}; available: {}",
                available.join(", ")
            )));
        };
        Ok(ToolOutput::success(format!(
            "# {}\n\n{}\n\n{}",
            skill.title, skill.description, skill.content
        )))
    }
}

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
