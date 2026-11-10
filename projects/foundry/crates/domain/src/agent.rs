//! Agent definitions, bindings and process status.
//!
//! A definition is the managed configuration of one agent: the account it acts
//! as, the model it calls, its prompt and its tool whitelist. One process per
//! definition is supervised by the control plane. Bindings attach a definition
//! to a project workflow role.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: Uuid,
    pub name: String,
    pub account_id: Uuid,
    pub account_handle: String,
    /// Workflow role this agent can fill, as `workflow_id:role`; `dispatcher`
    /// for the platform PM agent.
    pub role: String,
    pub model_base_url: String,
    pub model_name: String,
    /// Name of the environment variable holding the model API key. The key
    /// itself is never stored in the database.
    pub api_key_env: String,
    pub system_prompt: String,
    /// Tool names the process may register. Empty means the default set.
    pub tools: Vec<String>,
    pub enabled: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBinding {
    pub id: Uuid,
    pub project_id: Uuid,
    pub project_name: String,
    pub workflow_id: String,
    pub role: String,
    pub agent_definition_id: Uuid,
    pub agent_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProcess {
    pub agent_definition_id: Uuid,
    pub agent_name: String,
    pub status: String,
    pub pid: Option<i32>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub heartbeat_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub started_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub stopped_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentOverview {
    pub definitions: Vec<AgentDefinition>,
    pub bindings: Vec<AgentBinding>,
    pub processes: Vec<AgentProcess>,
}

#[derive(Debug, Clone)]
pub struct CreateAgentDefinition {
    pub name: String,
    pub account_handle: String,
    pub role: String,
    pub model_base_url: String,
    pub model_name: String,
    pub api_key_env: String,
    pub system_prompt: String,
    pub tools: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplateView {
    pub id: String,
    pub name: String,
    pub role: String,
    pub description: String,
}

/// A task with the agent roles that must be bound before it starts.
///
/// The PM sees one entry per task. It assigns agents to every agent role that
/// has no enabled agent yet, including roles used only in later stages, then
/// starts the task. Tasks with no agent role never enter this list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyTaskView {
    pub project_id: Uuid,
    pub project_name: String,
    pub task_id: Uuid,
    pub title: String,
    pub workflow: String,
    pub stage: String,
    pub roles: Vec<ReadyTaskRoleView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadyTaskRoleView {
    pub name: String,
    /// `workflow:role`, the key accepted by assignment APIs.
    pub task_role: String,
    pub kind: crate::RoleKind,
    pub min_actors: u32,
    pub max_actors: Option<u32>,
    /// The agent currently assigned to this role, if any.
    pub agent: Option<AssignedAgentView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignedAgentView {
    pub agent_definition_id: Uuid,
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct CreateAgentFromTemplate {
    pub task_id: Uuid,
    /// Workflow role expected by the task, in `workflow_id:role` form.
    pub role: String,
    pub template_id: String,
}

#[derive(Debug, Clone)]
pub struct AssignAgentToTask {
    pub task_id: Uuid,
    pub role: String,
    pub agent_definition_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct CreateAgentBinding {
    pub project_id: Uuid,
    pub workflow_id: String,
    pub role: String,
    pub agent_definition_id: Uuid,
}
