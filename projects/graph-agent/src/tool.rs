//! Tool definitions, execution and lookup.
//!
//! A tool is a named asynchronous operation with a JSON input schema. The
//! registry is assembled once per run and shared by LLM nodes and
//! deterministic nodes. Tool failures use a stable numeric code plus a
//! human-readable message; the tool name and code together identify an error.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Common tool-framework error codes. Tool-specific codes start at 1000 and
/// are only meaningful together with the tool name.
pub mod codes {
    pub const UNKNOWN_TOOL: u32 = 1;
    pub const INVALID_ARGUMENTS: u32 = 2;
    pub const DENIED: u32 = 3;
    pub const TIMEOUT: u32 = 4;
    pub const CANCELLED: u32 = 5;
    pub const NOT_FOUND: u32 = 6;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolOutput {
    Text(String),
    Json(Value),
    /// Text plus one or more images. The runtime appends the text as the
    /// tool result and sends the images in a follow-up user message so
    /// vision-capable models can inspect them.
    Multimodal {
        text: String,
        images: Vec<crate::message::LlmImage>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("tool error {code}: {message}")]
pub struct ToolError {
    /// Stable machine-readable code. Common codes are 1..=999; tool-specific
    /// codes start at 1000.
    pub code: u32,
    /// Human/agent-readable explanation.
    pub message: String,
}

impl ToolError {
    pub fn new(code: u32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolContext {
    pub run_id: String,
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub project_id: Option<String>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError>;
}

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("tool already registered: {0}")]
    DuplicateTool(String),
}

#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) -> Result<(), RegistryError> {
        let name = tool.definition().name;
        if self.tools.contains_key(&name) {
            return Err(RegistryError::DuplicateTool(name));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn definition(&self, name: &str) -> Result<ToolDefinition, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::new(codes::UNKNOWN_TOOL, format!("unknown tool: {name}")))?;
        Ok(tool.definition())
    }

    /// Return definitions in request order, skipping duplicate names.
    pub fn definitions(&self, names: &[String]) -> Result<Vec<ToolDefinition>, ToolError> {
        let mut seen = std::collections::BTreeSet::new();
        let mut definitions = Vec::with_capacity(names.len());
        for name in names {
            if !seen.insert(name.clone()) {
                continue;
            }
            definitions.push(self.definition(name)?);
        }
        Ok(definitions)
    }

    pub async fn call(
        &self,
        name: &str,
        arguments: Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .get(name)
            .ok_or_else(|| ToolError::new(codes::UNKNOWN_TOOL, format!("unknown tool: {name}")))?;
        tool.call(arguments, context).await
    }
}
