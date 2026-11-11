//! Full task tools for agents.
//!
//! Agents discover and operate the task pool through these tools. Project is
//! optional: execution agents default to the project of their run; the PM
//! dispatcher passes the project explicitly.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{FoundryClient, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct TasksReadyListTool {
    client: Arc<FoundryClient>,
}

impl TasksReadyListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TasksReadyListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "tasks_ready_list".to_string(),
            description:
                "List Ready tasks with their active roles and the agent assigned to each role."
                    .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let tasks = self
            .client
            .ready_tasks()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        tracing::info!(
            count = tasks.as_array().map(|entries| entries.len()).unwrap_or(0),
            "ready tasks listed"
        );
        if let Some(entries) = tasks.as_array() {
            for entry in entries {
                let task_id = entry.get("task_id").and_then(Value::as_str).unwrap_or("");
                let title = entry.get("title").and_then(Value::as_str).unwrap_or("");
                let workflow = entry.get("workflow").and_then(Value::as_str).unwrap_or("");
                let stage = entry.get("stage").and_then(Value::as_str).unwrap_or("");
                tracing::info!(
                    task_id = %task_id,
                    title = %title,
                    workflow = %workflow,
                    stage = %stage,
                    "ready task"
                );
                if let Some(roles) = entry.get("roles").and_then(Value::as_array) {
                    for role in roles {
                        let name = role.get("name").and_then(Value::as_str).unwrap_or("");
                        let task_role = role.get("task_role").and_then(Value::as_str).unwrap_or("");
                        let kind = role.get("kind").and_then(Value::as_str).unwrap_or("");
                        let min_actors =
                            role.get("min_actors").and_then(Value::as_u64).unwrap_or(0);
                        let agent = role.get("agent");
                        let agent_name = agent
                            .and_then(|agent| agent.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let enabled = agent
                            .and_then(|agent| agent.get("enabled"))
                            .and_then(Value::as_bool);
                        tracing::info!(
                            task_id = %task_id,
                            role = %name,
                            task_role = %task_role,
                            kind = %kind,
                            min_actors = min_actors,
                            agent = %agent_name,
                            enabled = ?enabled,
                            "ready task role"
                        );
                    }
                }
            }
        }
        if tasks
            .as_array()
            .map(|entries| entries.is_empty())
            .unwrap_or(true)
        {
            match self.client.list_tasks(None, "").await {
                Ok(pool) => {
                    if let Some(entries) = pool.as_array() {
                        tracing::info!(count = entries.len(), "task pool");
                        for entry in entries.iter().take(20) {
                            let task_id = entry.get("id").and_then(Value::as_str).unwrap_or("");
                            let title = entry.get("title").and_then(Value::as_str).unwrap_or("");
                            let workflow =
                                entry.get("workflow").and_then(Value::as_str).unwrap_or("");
                            let version = entry
                                .get("workflow_version")
                                .and_then(Value::as_u64)
                                .unwrap_or(0);
                            let state = entry.get("state").and_then(Value::as_str).unwrap_or("");
                            let stage = entry.get("stage").and_then(Value::as_str).unwrap_or("");
                            let dispatch = entry
                                .get("dispatch_state")
                                .and_then(Value::as_str)
                                .unwrap_or("");
                            let parent =
                                entry.get("parent_id").and_then(Value::as_str).unwrap_or("");
                            let bound = entry
                                .get("actors")
                                .and_then(Value::as_array)
                                .map(|actors| actors.len())
                                .unwrap_or(0);
                            tracing::info!(
                                task_id = %task_id,
                                title = %title,
                                workflow = %workflow,
                                version = version,
                                state = %state,
                                stage = %stage,
                                dispatch = %dispatch,
                                parent = %parent,
                                bound_actors = bound,
                                "task"
                            );
                        }
                    }
                }
                Err(error) => tracing::warn!(error = %error, "cannot list task pool"),
            }
        }
        pretty(tasks)
    }
}

pub struct TaskStartTool {
    client: Arc<FoundryClient>,
}

impl TaskStartTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskStartTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_start".to_string(),
            description:
                "Start a Ready task after its agent roles are assigned: Pending becomes InProgress."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["project_id", "task_id"],
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
        let project_id = parse_string(&arguments, "project_id")?;
        let task_id = parse_uuid(&arguments, "task_id")?;
        self.client
            .start_task(&project_id, task_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(format!("task {task_id} started")))
    }
}

pub struct TasksListTool {
    client: Arc<FoundryClient>,
}

impl TasksListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TasksListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "tasks_list".to_string(),
            description:
                "List tasks, optionally filtered. Pass project_id when acting across projects."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "state": { "type": "string" },
                    "stage": { "type": "string" },
                    "dispatch_state": { "type": "string" },
                    "role": { "type": "string" },
                    "text": { "type": "string" },
                    "limit": { "type": "integer" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let project = optional_string(&arguments, "project_id");
        let mut pairs = Vec::new();
        for key in ["state", "stage", "dispatch_state", "role", "text"] {
            if let Some(value) = optional_string(&arguments, key) {
                pairs.push(format!("{key}={}", encode(&value)));
            }
        }
        if let Some(limit) = arguments.get("limit").and_then(Value::as_u64) {
            pairs.push(format!("limit={limit}"));
        }
        let tasks = self
            .client
            .list_tasks(project.as_deref(), &pairs.join("&"))
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(tasks)
    }
}

pub struct TaskGetTool {
    client: Arc<FoundryClient>,
}

impl TaskGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_get".to_string(),
            description: "Read one task with actors, role specs, operations and comments."
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
        let project = optional_string(&arguments, "project_id");
        let task = self
            .client
            .get_task(project.as_deref(), task_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(task)
    }
}

pub struct TaskAllowedOperationsTool {
    client: Arc<FoundryClient>,
}

impl TaskAllowedOperationsTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskAllowedOperationsTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_allowed_operations".to_string(),
            description: "List the operations the current account may send on a task.".to_string(),
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
        let project = optional_string(&arguments, "project_id");
        let operations = self
            .client
            .task_allowed_operations(project.as_deref(), task_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(operations)
    }
}

pub struct TaskOperationTool {
    client: Arc<FoundryClient>,
}

impl TaskOperationTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskOperationTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_operation".to_string(),
            description: "Send a task operation such as started, completed or approved."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "operation"],
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "task_id": { "type": "string", "format": "uuid" },
                    "operation": { "type": "string" },
                    "idempotency_key": { "type": "string" }
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
        let operation = parse_string(&arguments, "operation")?;
        let project = optional_string(&arguments, "project_id");
        let key = optional_string(&arguments, "idempotency_key")
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let result = self
            .client
            .send_task_operation(project.as_deref(), task_id, &operation, &key)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(result)
    }
}

pub struct TaskCommentTool {
    client: Arc<FoundryClient>,
}

impl TaskCommentTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskCommentTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_comment".to_string(),
            description: "Add a comment to a task.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "body"],
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "task_id": { "type": "string", "format": "uuid" },
                    "body": { "type": "string" }
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
        let body = parse_string(&arguments, "body")?;
        let project = optional_string(&arguments, "project_id");
        let task = self
            .client
            .comment_task(project.as_deref(), task_id, &body)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(task)
    }
}

pub struct TaskHistoryTool {
    client: Arc<FoundryClient>,
}

impl TaskHistoryTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskHistoryTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_history".to_string(),
            description: "Read a task's operation history.".to_string(),
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
        let project = optional_string(&arguments, "project_id");
        let history = self
            .client
            .task_history(project.as_deref(), task_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(history)
    }
}

pub struct TaskAddLinkTool {
    client: Arc<FoundryClient>,
}

impl TaskAddLinkTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskAddLinkTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_add_link".to_string(),
            description: "Add a link from a task to an Issue, document or URL.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "kind", "title", "relation"],
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "task_id": { "type": "string", "format": "uuid" },
                    "kind": { "type": "string" },
                    "target_id": { "type": "string", "format": "uuid" },
                    "url": { "type": "string" },
                    "title": { "type": "string" },
                    "relation": { "type": "string" },
                    "metadata": { "type": "object" }
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
        let project = optional_string(&arguments, "project_id");
        let mut body = arguments.clone();
        if let Some(object) = body.as_object_mut() {
            object.remove("project_id");
            object.remove("task_id");
        }
        let link = self
            .client
            .add_task_link(project.as_deref(), task_id, body)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(link)
    }
}

pub struct TaskRemoveLinkTool {
    client: Arc<FoundryClient>,
}

impl TaskRemoveLinkTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskRemoveLinkTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_remove_link".to_string(),
            description: "Remove a link from a task.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "link_id"],
                "properties": {
                    "project_id": { "type": "string", "format": "uuid" },
                    "task_id": { "type": "string", "format": "uuid" },
                    "link_id": { "type": "string", "format": "uuid" }
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
        let link_id = parse_uuid(&arguments, "link_id")?;
        let project = optional_string(&arguments, "project_id");
        let result = self
            .client
            .remove_task_link(project.as_deref(), task_id, link_id)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        ))
    }
}

pub struct TaskAssignAgentTool {
    client: Arc<FoundryClient>,
}

impl TaskAssignAgentTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for TaskAssignAgentTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "task_assign_agent".to_string(),
            description: "Assign an agent to a task role.".to_string(),
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

fn optional_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_string(arguments: &Value, key: &str) -> Result<String, ToolError> {
    optional_string(arguments, key)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn parse_uuid(arguments: &Value, key: &str) -> Result<Uuid, ToolError> {
    parse_string(arguments, key)?
        .parse()
        .map_err(|_| ToolError::InvalidArguments(format!("invalid {key}")))
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}
