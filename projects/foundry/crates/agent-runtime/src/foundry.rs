//! Foundry HTTP client.
//!
//! Phase 1 uses Foundry's REST API directly. The runtime still treats these as
//! ordinary tools; the transport can later be replaced by MCP without changing
//! the agent loop.

use serde_json::Value;
use uuid::Uuid;

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

    pub async fn get_project(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/projects/{}",
            self.base_url.trim_end_matches('/'),
            self.project_id
        ))
        .await
    }

    pub async fn list_task_links(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links",
                self.base_url.trim_end_matches('/')
            ),
            None,
        )
        .await
    }

    pub async fn list_issues(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/issues",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn issue_triage_batch(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/issues/triage-batch",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn list_triggers(&self) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/projects/{}/triggers",
            self.base_url.trim_end_matches('/'),
            self.project_id
        ))
        .await
    }

    pub async fn create_trigger(&self, body: Value) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/projects/{}/triggers",
                self.base_url.trim_end_matches('/'),
                self.project_id
            ),
            body,
            None,
        )
        .await
    }

    pub async fn set_issue_triage(
        &self,
        issue_id: Uuid,
        body: Value,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/issues/{issue_id}/triage",
                self.base_url.trim_end_matches('/')
            ),
            body,
            None,
        )
        .await
    }

    pub async fn create_issue(&self, body: Value) -> Result<Value, FoundryError> {
        self.post(
            &format!("{}/api/v1/issues", self.base_url.trim_end_matches('/')),
            body,
            None,
        )
        .await
    }

    pub async fn transition_issue(
        &self,
        issue_id: Uuid,
        to: &str,
        resolution: Option<&str>,
        duplicate_of: Option<Uuid>,
        pool: Option<&str>,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/issues/{issue_id}/transitions",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({
                "to": to,
                "resolution": resolution,
                "duplicate_of": duplicate_of,
                "pool": pool,
            }),
            None,
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

    pub async fn dispatch_agents(&self, project_id: Option<Uuid>) -> Result<Value, FoundryError> {
        let mut url = format!(
            "{}/api/v1/agent-dispatch/agents",
            self.base_url.trim_end_matches('/')
        );
        if let Some(project_id) = project_id {
            url.push_str(&format!("?project_id={project_id}"));
        }
        self.get(&url).await
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

    pub async fn ready_tasks(&self, project_id: Option<Uuid>) -> Result<Value, FoundryError> {
        let mut url = format!(
            "{}/api/v1/agent-dispatch/tasks",
            self.base_url.trim_end_matches('/')
        );
        if let Some(project_id) = project_id {
            url.push_str(&format!("?project_id={project_id}"));
        }
        self.get(&url).await
    }

    pub async fn start_task(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/start",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({}),
            None,
            None,
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

    pub async fn get_task(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}",
                self.base_url.trim_end_matches('/')
            ),
            None,
        )
        .await
    }

    pub async fn task_allowed_operations(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/allowed-operations",
                self.base_url.trim_end_matches('/')
            ),
            None,
        )
        .await
    }

    pub async fn agent_credentials(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/agent/credentials",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "task_id": task_id }),
            None,
            None,
        )
        .await
    }

    /// The agent account's Arche SSH key. It belongs to the agent, not to a
    /// task, and is installed once when the shell starts.
    pub async fn agent_arche_credential(&self, definition_id: Uuid) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/agent/arche-credential",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "definition_id": definition_id }),
            None,
            None,
        )
        .await
    }

    pub async fn send_task_operation(
        &self,
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
            None,
        )
        .await
    }

    pub async fn comment_task(&self, task_id: Uuid, body: &str) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/comments",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "body": body }),
            None,
            None,
        )
        .await
    }

    pub async fn bind_task_actor(
        &self,
        task_id: Uuid,
        role: &str,
        account_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/tasks/{task_id}/actors",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "role": role, "account_id": account_id }),
            None,
        )
        .await
    }

    pub async fn spawn_task_children(
        &self,
        task_id: Uuid,
        tasks: Value,
        sequential: bool,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/tasks/{task_id}/children",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({ "tasks": tasks, "sequential": sequential }),
            None,
        )
        .await
    }

    pub async fn retry_task(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/tasks/{task_id}/retry",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({}),
            None,
        )
        .await
    }

    pub async fn create_follow_up_task(
        &self,
        task_id: Uuid,
        issue_id: Uuid,
        workflow: &str,
        title: &str,
        description: &str,
        input: Value,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/tasks/{task_id}/follow-up",
                self.base_url.trim_end_matches('/')
            ),
            serde_json::json!({
                "issue_id": issue_id,
                "workflow": workflow,
                "title": title,
                "description": description,
                "input": input,
            }),
            None,
        )
        .await
    }

    pub async fn create_merge_request_for_task(
        &self,
        task_id: Uuid,
        body: Value,
    ) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/tasks/{task_id}/merge-requests",
                self.base_url.trim_end_matches('/')
            ),
            body,
            None,
        )
        .await
    }

    pub async fn get_merge_request_for_task(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.get(&format!(
            "{}/api/v1/tasks/{task_id}/merge-request",
            self.base_url.trim_end_matches('/')
        ))
        .await
    }

    pub async fn merge_merge_request(&self, merge_request_id: Uuid) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/projects/{}/merge-requests/{merge_request_id}/merge",
                self.base_url.trim_end_matches('/'),
                self.project_id
            ),
            Value::Object(Default::default()),
            None,
        )
        .await
    }

    pub async fn delete_design_space_branch(&self, branch: &str) -> Result<Value, FoundryError> {
        self.post(
            &format!(
                "{}/api/v1/projects/{}/design-space/branches/delete",
                self.base_url.trim_end_matches('/'),
                self.project_id
            ),
            serde_json::json!({ "branch": branch }),
            None,
        )
        .await
    }

    pub async fn task_history(&self, task_id: Uuid) -> Result<Value, FoundryError> {
        self.get_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/history",
                self.base_url.trim_end_matches('/')
            ),
            None,
        )
        .await
    }

    pub async fn add_task_link(&self, task_id: Uuid, body: Value) -> Result<Value, FoundryError> {
        self.post_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links",
                self.base_url.trim_end_matches('/')
            ),
            body,
            None,
            None,
        )
        .await
    }

    pub async fn remove_task_link(
        &self,
        task_id: Uuid,
        link_id: Uuid,
    ) -> Result<Value, FoundryError> {
        self.delete_scoped(
            &format!(
                "{}/api/v1/tasks/{task_id}/links/{link_id}",
                self.base_url.trim_end_matches('/')
            ),
            None,
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
