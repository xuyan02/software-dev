//! Dispatcher tools: list agents/templates, create and assign agents.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct AgentsListTool {
    client: Arc<FoundryClient>,
}

impl AgentsListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AgentsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agents_list".to_string(),
            description: "List platform agents with their role, enabled flag and process status."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let agents = self
            .client
            .dispatch_agents()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(agents)
    }
}

pub struct AgentTemplatesListTool {
    client: Arc<FoundryClient>,
}

impl AgentTemplatesListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AgentTemplatesListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent_templates_list".to_string(),
            description: "List platform agent templates for a workflow role.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["role"],
                "properties": { "role": { "type": "string" } }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let role = parse_string(&arguments, "role")?;
        let templates = self
            .client
            .dispatch_templates(&role)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(templates)
    }
}

pub struct AgentCreateForRoleTool {
    client: Arc<FoundryClient>,
}

impl AgentCreateForRoleTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AgentCreateForRoleTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent_create_for_role".to_string(),
            description: "Create a new platform agent from a template for an unassigned task role."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "role", "template_id"],
                "properties": {
                    "task_id": { "type": "string", "format": "uuid" },
                    "role": { "type": "string" },
                    "template_id": { "type": "string" }
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
        let role = parse_string(&arguments, "role")?;
        let template_id = parse_string(&arguments, "template_id")?;
        let agent = self
            .client
            .create_agent_from_template(task_id, &role, &template_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(agent)
    }
}

pub struct AgentAssignTool {
    client: Arc<FoundryClient>,
}

impl AgentAssignTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for AgentAssignTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent_assign".to_string(),
            description: "Assign an agent to an unassigned task role.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "role", "agent_definition_id"],
                "properties": {
                    "task_id": { "type": "string", "format": "uuid" },
                    "role": { "type": "string" },
                    "agent_definition_id": { "type": "string", "format": "uuid" }
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
        let role = parse_string(&arguments, "role")?;
        let agent_definition_id = parse_uuid(&arguments, "agent_definition_id")?;
        let result = self
            .client
            .assign_agent(task_id, &role, agent_definition_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(result)
    }
}

fn parse_string(arguments: &Value, key: &str) -> Result<String, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn parse_uuid(arguments: &Value, key: &str) -> Result<Uuid, ToolError> {
    parse_string(arguments, key)?
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("invalid {key}")))
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}
