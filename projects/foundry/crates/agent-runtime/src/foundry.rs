//! Foundry HTTP client and requirements-document tools.
//!
//! Phase 1 uses Foundry's REST API directly. The runtime still treats these as
//! ordinary tools; the transport can later be replaced by MCP without changing
//! the agent loop.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::{ContextBundle, Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

#[derive(Debug, thiserror::Error)]
pub enum FoundryError {
    #[error("http error: {0}")]
    Http(String),
    #[error("foundry returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid json: {0}")]
    Json(String),
}

pub struct FoundryClient {
    base_url: String,
    token: String,
    project_id: String,
    http: reqwest::Client,
}

impl FoundryClient {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        project_id: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            token: token.into(),
            project_id: project_id.into(),
            http: reqwest::Client::new(),
        }
    }

    fn document_url(&self) -> String {
        format!(
            "{}/api/v1/projects/{}/requirements-document",
            self.base_url.trim_end_matches('/'),
            self.project_id
        )
    }

    pub async fn get_document(&self) -> Result<Value, FoundryError> {
        self.get(&self.document_url()).await
    }

    pub async fn get_draft(&self) -> Result<Value, FoundryError> {
        self.get(&format!("{}/draft", self.document_url())).await
    }

    pub async fn validate(&self) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/draft/validate", self.document_url()),
            Value::Object(Default::default()),
            None,
        )
        .await
    }

    pub async fn apply_operations(
        &self,
        expected_revision: u64,
        operations: Value,
    ) -> Result<Value, FoundryError> {
        let body = serde_json::json!({ "operations": operations });
        self.post(
            &format!("{}/draft/operations", self.document_url()),
            body,
            Some(expected_revision),
        )
        .await
    }

    pub async fn get_project(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/projects/{}",
            self.base_url.trim_end_matches('/'),
            self.project_id
        ))
        .await
    }

    fn anchor_document_url(&self, kind: &str) -> String {
        format!(
            "{}/api/v1/projects/{}/documents/{}",
            self.base_url.trim_end_matches('/'),
            self.project_id,
            kind
        )
    }

    pub async fn get_anchor_document_toc(
        &self,
        kind: &str,
        anchor: Option<&str>,
    ) -> Result<String, FoundryError> {
        match anchor {
            Some(anchor) => {
                self.get_text(&format!(
                    "{}?anchor={anchor}",
                    self.anchor_document_url(kind)
                ))
                .await
            }
            None => self.get_text(&self.anchor_document_url(kind)).await,
        }
    }

    pub async fn open_anchor_document_draft(&self, kind: &str) -> Result<Value, FoundryError> {
        self.get(&format!("{}/draft", self.anchor_document_url(kind)))
            .await
    }

    pub async fn create_anchor_document_draft(&self, kind: &str) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/draft", self.anchor_document_url(kind)),
            Value::Object(Default::default()),
            None,
        )
        .await
    }

    pub async fn get_anchor_document_content(
        &self,
        kind: &str,
        anchor: Option<&str>,
    ) -> Result<Value, FoundryError> {
        match anchor {
            Some(anchor) => {
                self.get(&format!(
                    "{}/content?anchor={anchor}",
                    self.anchor_document_url(kind)
                ))
                .await
            }
            None => {
                self.get(&format!("{}/content", self.anchor_document_url(kind)))
                    .await
            }
        }
    }

    pub async fn validate_anchor_document(&self, kind: &str) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/draft/validate", self.anchor_document_url(kind)),
            Value::Object(Default::default()),
            None,
        )
        .await
    }

    pub async fn apply_anchor_document_operations(
        &self,
        kind: &str,
        expected_revision: u64,
        operations: Value,
    ) -> Result<Value, FoundryError> {
        let body = serde_json::json!({ "operations": operations });
        self.post(
            &format!("{}/draft/operations", self.anchor_document_url(kind)),
            body,
            Some(expected_revision),
        )
        .await
    }

    pub async fn list_anchor_annotations(
        &self,
        kind: &str,
        include_resolved: bool,
    ) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/annotations?include_resolved={include_resolved}",
            self.anchor_document_url(kind)
        ))
        .await
    }

    pub async fn annotate_anchor_document(
        &self,
        kind: &str,
        body: Value,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/annotations", self.anchor_document_url(kind)),
            body,
            None,
        )
        .await
    }

    pub async fn resolve_anchor_annotation(
        &self,
        kind: &str,
        annotation_id: &str,
        resolution: &str,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/annotations/{annotation_id}/resolve",
                self.anchor_document_url(kind)
            ),
            serde_json::json!({ "resolution": resolution }),
            None,
        )
        .await
    }

    pub async fn list_anchor_approvals(&self, kind: &str) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/draft/approvals",
            self.anchor_document_url(kind)
        ))
        .await
    }

    pub async fn list_anchor_versions(&self, kind: &str) -> Result<Value, FoundryError> {
        self.get(&format!("{}/versions", self.anchor_document_url(kind)))
            .await
    }

    pub async fn decide_anchor_document(
        &self,
        kind: &str,
        expected_revision: u64,
        decision: &str,
        comment: Option<&str>,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/draft/approvals", self.anchor_document_url(kind)),
            serde_json::json!({
                "decision": decision,
                "comment": comment,
            }),
            Some(expected_revision),
        )
        .await
    }

    pub async fn list_annotations(&self, include_resolved: bool) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/annotations?include_resolved={include_resolved}",
            self.document_url()
        ))
        .await
    }

    pub async fn annotate(
        &self,
        target_type: &str,
        target_id: Option<&str>,
        field: Option<&str>,
        body: &str,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/annotations", self.document_url()),
            serde_json::json!({
                "target_type": target_type,
                "target_id": target_id,
                "field": field,
                "body": body,
            }),
            None,
        )
        .await
    }

    pub async fn resolve_annotation(
        &self,
        annotation_id: Uuid,
        resolution: &str,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/annotations/{annotation_id}/resolve",
                self.document_url()
            ),
            serde_json::json!({ "resolution": resolution }),
            None,
        )
        .await
    }

    pub async fn requirements_context(&self) -> Result<ContextBundle, FoundryError> {
        let document = self.get_document().await?;
        let content = serde_json::to_string_pretty(&document)
            .map_err(|error| FoundryError::Json(error.to_string()))?;
        let mut context = ContextBundle::new();
        context.push("requirements_document", self.project_id.clone(), content);
        Ok(context)
    }

    pub async fn list_task_links(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.list_task_links_scoped(None, task_id).await
    }

    pub async fn list_task_links_scoped(
        &self,
        project: Option<&str>,
        task_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links",
                self.base_url.trim_end_matches('/')
            ),
            project,
        )
        .await
    }

    pub async fn get_issue(&self, issue_id: Uuid) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/issues/{issue_id}",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn comment_issue(
        &self,
        issue_id: Uuid,
        body: impl Into<String>,
        internal: bool,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/issues/{issue_id}/comments",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "body": body.into(), "internal": internal }),
            None,
        )
        .await
    }

    pub async fn dispatch_agents(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/agent-dispatch/agents",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn dispatch_templates(&self, role: &str) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/agent-dispatch/templates?role={role}",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn create_agent_from_template(
        &self,
        task_id: Uuid,
        role: &str,
        template_id: &str,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/agent-dispatch/create",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({
                "task_id": task_id,
                "role": role,
                "template_id": template_id,
            }),
            None,
        )
        .await
    }

    pub async fn assign_agent(
        &self,
        task_id: Uuid,
        role: &str,
        agent_definition_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/agent-dispatch/assign",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({
                "task_id": task_id,
                "role": role,
                "agent_definition_id": agent_definition_id,
            }),
            None,
        )
        .await
    }

    pub async fn ready_tasks(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/agent-dispatch/tasks",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn start_task(&self, project_id: &str, task_id: Uuid) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/start",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({}),
            None,
            Some(project_id),
        )
        .await
    }

    pub async fn list_tasks(
        &self,
        project: Option<&str>,
        query: &str,
    ) -> Result<Value, FoundryError> {
        let mut url = format!("{}/api/v1/tasks", self.base_url.trim_end_matches('/'));
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        self.get_scoped(&url, project).await
    }

    pub async fn get_task(
        &self,
        project: Option<&str>,
        task_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}",
                self.base_url.trim_end_matches('/')
            ),
            project,
        )
        .await
    }

    pub async fn task_allowed_operations(
        &self,
        project: Option<&str>,
        task_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/allowed-operations",
                self.base_url.trim_end_matches('/')
            ),
            project,
        )
        .await
    }

    pub async fn send_task_operation(
        &self,
        project: Option<&str>,
        task_id: Uuid,
        operation: &str,
        idempotency_key: &str,
    ) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/operations",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({
                "operation": operation,
                "idempotency_key": idempotency_key,
            }),
            None,
            project,
        )
        .await
    }

    pub async fn comment_task(
        &self,
        project: Option<&str>,
        task_id: Uuid,
        body: &str,
    ) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/comments",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "body": body }),
            None,
            project,
        )
        .await
    }

    pub async fn task_history(
        &self,
        project: Option<&str>,
        task_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/history",
                self.base_url.trim_end_matches('/')
            ),
            project,
        )
        .await
    }

    pub async fn add_task_link(
        &self,
        project: Option<&str>,
        task_id: Uuid,
        body: Value,
    ) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links",
                self.base_url.trim_end_matches('/')
            ),
            body,
            None,
            project,
        )
        .await
    }

    pub async fn remove_task_link(
        &self,
        project: Option<&str>,
        task_id: Uuid,
        link_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.delete_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links/{link_id}",
                self.base_url.trim_end_matches('/')
            ),
            project,
        )
        .await
    }

    async fn get(&self, url: &str) -> Result<Value, FoundryError> {
        self.get_scoped(url, None).await
    }

    async fn get_scoped(&self, url: &str, project: Option<&str>) -> Result<Value, FoundryError> {
        let mut request = self.http.get(url).bearer_auth(&self.token);
        let project = project.unwrap_or(self.project_id.as_str());
        if !project.is_empty() {
            request = request.header("X-Foundry-Project", project);
        }
        let response = request
            .send()
            .await
            .map_err(|error| FoundryError::Http(error.to_string()))?;
        parse_response(response).await
    }

    async fn get_text(&self, url: &str) -> Result<String, FoundryError> {
        let mut request = self.http.get(url).bearer_auth(&self.token);
        if !self.project_id.is_empty() {
            request = request.header("X-Foundry-Project", self.project_id.as_str());
        }
        let response = request
            .send()
            .await
            .map_err(|error| FoundryError::Http(error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| FoundryError::Http(error.to_string()))?;
        if !status.is_success() {
            return Err(FoundryError::Status {
                status: status.as_u16(),
                body: text,
            });
        }
        Ok(text)
    }

    async fn post(
        &self,
        url: &str,
        body: Value,
        expected_revision: Option<u64>,
    ) -> Result<Value, FoundryError> {
        self.post_scoped(url, body, expected_revision, None).await
    }

    async fn post_scoped(
        &self,
        url: &str,
        body: Value,
        expected_revision: Option<u64>,
        project: Option<&str>,
    ) -> Result<Value, FoundryError> {
        let mut request = self.http.post(url).bearer_auth(&self.token).json(&body);
        let project = project.unwrap_or(self.project_id.as_str());
        if !project.is_empty() {
            request = request.header("X-Foundry-Project", project);
        }
        if let Some(revision) = expected_revision {
            request = request.header("If-Match", format!("\"{revision}\""));
        }
        let response = request
            .send()
            .await
            .map_err(|error| FoundryError::Http(error.to_string()))?;
        parse_response(response).await
    }

    async fn delete_scoped(&self, url: &str, project: Option<&str>) -> Result<Value, FoundryError> {
        let mut request = self.http.delete(url).bearer_auth(&self.token);
        let project = project.unwrap_or(self.project_id.as_str());
        if !project.is_empty() {
            request = request.header("X-Foundry-Project", project);
        }
        let response = request
            .send()
            .await
            .map_err(|error| FoundryError::Http(error.to_string()))?;
        parse_response(response).await
    }
}

async fn parse_response(response: reqwest::Response) -> Result<Value, FoundryError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| FoundryError::Http(error.to_string()))?;
    if !status.is_success() {
        return Err(FoundryError::Status {
            status: status.as_u16(),
            body: text,
        });
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|error| FoundryError::Json(error.to_string()))
}

pub struct FoundryRequirementsGetTool {
    client: Arc<FoundryClient>,
}

impl FoundryRequirementsGetTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for FoundryRequirementsGetTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_get".to_string(),
            description: "Read the project's structured requirements document.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let document = self
            .client
            .get_document()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(document)
    }
}

pub struct FoundryRequirementsDraftTool {
    client: Arc<FoundryClient>,
}

impl FoundryRequirementsDraftTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for FoundryRequirementsDraftTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_draft".to_string(),
            description: "Open the shared requirements draft and return its revision for edits."
                .to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let draft = self
            .client
            .get_draft()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(draft)
    }
}

pub struct FoundryRequirementsApplyTool {
    client: Arc<FoundryClient>,
}

impl FoundryRequirementsApplyTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for FoundryRequirementsApplyTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_apply_operations".to_string(),
            description: "Apply structured operations to the shared requirements draft. \
                          Requires the draft revision and returns the new revision."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["expected_revision", "operations"],
                "properties": {
                    "expected_revision": { "type": "integer" },
                    "operations": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["op_id", "type"],
                            "additionalProperties": false,
                            "properties": {
                                "op_id": { "type": "string", "format": "uuid" },
                                "type": {
                                    "type": "string",
                                    "enum": [
                                        "user_add", "user_update", "user_remove",
                                        "function_add", "function_update", "function_remove",
                                        "operation_add", "operation_update", "operation_remove",
                                        "story_add", "story_update", "story_remove",
                                        "story_set_operations"
                                    ]
                                },
                                "function_id": { "type": "string" },
                                "user_id": { "type": "string" },
                                "operation_id": { "type": "string" },
                                "story_id": { "type": "string" },
                                "operation_ids": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                },
                                "user": {
                                    "type": "object",
                                    "required": ["id", "name"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "id": { "type": "string" },
                                        "name": { "type": "string" },
                                        "age": { "type": ["integer", "null"] },
                                        "gender": { "type": ["string", "null"] },
                                        "occupation": { "type": ["string", "null"] },
                                        "habits": { "type": "array", "items": { "type": "string" } },
                                        "goals": { "type": "array", "items": { "type": "string" } },
                                        "pain_points": { "type": "array", "items": { "type": "string" } },
                                        "permissions": { "type": "array", "items": { "type": "string" } },
                                        "scenarios": { "type": "array", "items": { "type": "string" } },
                                        "devices": { "type": "array", "items": { "type": "string" } },
                                        "accessibility": { "type": "array", "items": { "type": "string" } },
                                        "evidence": { "type": "array", "items": { "type": "string" } },
                                        "confidence": { "type": "string", "enum": ["low", "medium", "high"] },
                                        "status": { "type": "string", "enum": ["draft", "active", "deprecated", "retired"] }
                                    }
                                },
                                "function": {
                                    "type": "object",
                                    "required": ["id", "name"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "id": { "type": "string" },
                                        "name": { "type": "string" },
                                        "description": { "type": ["string", "null"] },
                                        "owner": { "type": ["string", "null"] },
                                        "status": { "type": "string", "enum": ["draft", "active", "deprecated", "retired"] }
                                    }
                                },
                                "operation": {
                                    "type": "object",
                                    "required": ["id", "name"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "id": { "type": "string" },
                                        "name": { "type": "string" },
                                        "description": { "type": ["string", "null"] },
                                        "actor_user_ids": { "type": "array", "items": { "type": "string" } },
                                        "trigger": { "type": ["string", "null"] },
                                        "preconditions": { "type": "array", "items": { "type": "string" } },
                                        "inputs": { "type": "array", "items": { "type": "string" } },
                                        "outputs": { "type": "array", "items": { "type": "string" } },
                                        "states": { "type": "array", "items": { "type": "string" } },
                                        "exceptions": { "type": "array", "items": { "type": "string" } },
                                        "permissions": { "type": "array", "items": { "type": "string" } },
                                        "side_effects": { "type": "array", "items": { "type": "string" } },
                                        "audit": { "type": "array", "items": { "type": "string" } },
                                        "interaction_id": { "type": ["string", "null"] },
                                        "status": { "type": "string", "enum": ["draft", "active", "deprecated", "retired"] }
                                    }
                                },
                                "story": {
                                    "type": "object",
                                    "required": ["id", "title"],
                                    "additionalProperties": false,
                                    "properties": {
                                        "id": { "type": "string" },
                                        "title": { "type": "string" },
                                        "user_ids": { "type": "array", "items": { "type": "string" } },
                                        "scenario": { "type": "string" },
                                        "goal": { "type": "string" },
                                        "preconditions": { "type": "array", "items": { "type": "string" } },
                                        "postconditions": { "type": "array", "items": { "type": "string" } },
                                        "operation_ids": { "type": "array", "items": { "type": "string" } },
                                        "interaction_flow_id": { "type": ["string", "null"] },
                                        "screen_ids": { "type": "array", "items": { "type": "string" } },
                                        "priority": { "type": "string", "enum": ["must", "should", "could"] },
                                        "source_issue_ids": { "type": "array", "items": { "type": "string" } },
                                        "acceptance_notes": { "type": "array", "items": { "type": "string" } },
                                        "status": { "type": "string", "enum": ["draft", "active", "deprecated", "retired"] }
                                    }
                                }
                            }
                        }
                    }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let expected_revision = arguments
            .get("expected_revision")
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::InvalidArguments("missing expected_revision".to_string()))?;
        let operations = arguments
            .get("operations")
            .cloned()
            .ok_or_else(|| ToolError::InvalidArguments("missing operations".to_string()))?;
        let result = self
            .client
            .apply_operations(expected_revision, operations)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(result)
    }
}

pub struct FoundryRequirementsValidateTool {
    client: Arc<FoundryClient>,
}

impl FoundryRequirementsValidateTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for FoundryRequirementsValidateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_validate".to_string(),
            description: "Validate the current requirements draft.".to_string(),
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let validation = self
            .client
            .validate()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(validation)
    }
}

pub struct RequirementsAnnotationsListTool {
    client: Arc<FoundryClient>,
}

impl RequirementsAnnotationsListTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for RequirementsAnnotationsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_annotations_list".to_string(),
            description: "List annotations on the requirements draft. Open annotations are \
                          review feedback that must be addressed or resolved."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "include_resolved": { "type": "boolean", "default": false }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let include_resolved = arguments
            .get("include_resolved")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let annotations = self
            .client
            .list_annotations(include_resolved)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotations)
    }
}

pub struct RequirementsAnnotateTool {
    client: Arc<FoundryClient>,
}

impl RequirementsAnnotateTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for RequirementsAnnotateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_annotate".to_string(),
            description: "Add an annotation to a requirements object or field, for example a \
                          specific story goal that is ambiguous."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["target_type", "body"],
                "properties": {
                    "target_type": {
                        "type": "string",
                        "enum": ["document", "user", "function", "story", "operation"]
                    },
                    "target_id": { "type": "string" },
                    "field": { "type": "string" },
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
        let target_type = arguments
            .get("target_type")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing target_type".to_string()))?;
        let body = arguments
            .get("body")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing body".to_string()))?;
        let target_id = arguments.get("target_id").and_then(Value::as_str);
        let field = arguments.get("field").and_then(Value::as_str);
        let annotation = self
            .client
            .annotate(target_type, target_id, field, body)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotation)
    }
}

pub struct RequirementsResolveAnnotationTool {
    client: Arc<FoundryClient>,
}

impl RequirementsResolveAnnotationTool {
    pub fn new(client: Arc<FoundryClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Tool for RequirementsResolveAnnotationTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "requirements_resolve_annotation".to_string(),
            description: "Mark a requirements annotation resolved after applying the requested \
                          change. Include a short resolution note."
                .to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["annotation_id", "resolution"],
                "properties": {
                    "annotation_id": { "type": "string", "format": "uuid" },
                    "resolution": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let annotation_id = arguments
            .get("annotation_id")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<Uuid>().ok())
            .ok_or_else(|| ToolError::InvalidArguments("missing annotation_id".to_string()))?;
        let resolution = arguments
            .get("resolution")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing resolution".to_string()))?;
        let annotation = self
            .client
            .resolve_annotation(annotation_id, resolution)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        pretty(annotation)
    }
}

fn pretty(value: Value) -> Result<ToolOutput, ToolError> {
    let content = serde_json::to_string_pretty(&value)
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    Ok(ToolOutput::success(content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::Json;
    use axum::http::HeaderMap;
    use axum::routing::{get, post};
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn reads_the_document() {
        let app = Router::new().route(
            "/api/v1/projects/project-1/requirements-document",
            get(|| async { Json(serde_json::json!({ "users": [], "functions": [] })) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let client = Arc::new(FoundryClient::new(
            format!("http://{addr}"),
            "token",
            "project-1",
        ));
        let tool = FoundryRequirementsGetTool::new(client);
        let output = tool
            .call(
                Value::Object(Default::default()),
                &ToolContext {
                    agent_id: "a".to_string(),
                    task_id: "t".to_string(),
                },
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("\"users\""));
    }

    #[tokio::test]
    async fn applies_operations_with_if_match() {
        let captured: Arc<Mutex<Option<(String, Value)>>> = Arc::new(Mutex::new(None));
        let state = captured.clone();
        let app = Router::new().route(
            "/api/v1/projects/project-1/requirements-document/draft/operations",
            post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let state = state.clone();
                async move {
                    let if_match = headers
                        .get("if-match")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or_default()
                        .to_string();
                    *state.lock().expect("lock poisoned") = Some((if_match, body));
                    Json(serde_json::json!({ "revision": 2 }))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let client = Arc::new(FoundryClient::new(
            format!("http://{addr}"),
            "token",
            "project-1",
        ));
        let tool = FoundryRequirementsApplyTool::new(client);
        let output = tool
            .call(
                serde_json::json!({
                    "expected_revision": 1,
                    "operations": [{ "op_id": "x", "type": "user_add" }]
                }),
                &ToolContext {
                    agent_id: "a".to_string(),
                    task_id: "t".to_string(),
                },
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        let (if_match, body) = captured.lock().unwrap().clone().unwrap();
        assert_eq!(if_match, "\"1\"");
        assert_eq!(body["operations"][0]["type"], "user_add");
    }
}
