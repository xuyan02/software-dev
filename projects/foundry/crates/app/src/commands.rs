use foundry_domain::{
    AccountKind, AccountRole, AccountStatus, Actor, IssueKind, IssueSeverity, IssueSource,
    IssueStatus, ProjectRole, ProjectStatus, WorkflowBody, WorkflowId, WorkflowLayout,
    WorkflowVersionRef,
};

#[derive(Debug, Clone)]
pub struct CreateAccount {
    pub handle: String,
    pub display_name: String,
    pub kind: AccountKind,
    pub role: AccountRole,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct UpdateAccount {
    pub id: uuid::Uuid,
    pub display_name: Option<String>,
    pub email: Option<Option<String>>,
    pub role: Option<AccountRole>,
    pub status: Option<AccountStatus>,
    pub new_password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateProject {
    pub name: String,
    pub description: Option<String>,
    pub root_workflow_id: WorkflowId,
}

/// Where a new workflow definition lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowScope {
    Account,
    Platform,
}

#[derive(Debug, Clone)]
pub struct CreateWorkflow {
    pub title: String,
    pub scope: WorkflowScope,
    /// A published platform version copied verbatim into the new draft.
    pub copy_from: Option<WorkflowVersionRef>,
}

#[derive(Debug, Clone)]
pub struct RenameWorkflow {
    pub id: WorkflowId,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct SaveWorkflowDraft {
    pub id: WorkflowId,
    pub body: WorkflowBody,
    pub expected_revision: u64,
    pub client_save_id: uuid::Uuid,
}

#[derive(Debug, Clone)]
pub struct PublishWorkflow {
    pub id: WorkflowId,
    pub expected_revision: u64,
}

#[derive(Debug, Clone)]
pub struct SaveWorkflowLayout {
    pub id: WorkflowId,
    pub layout: WorkflowLayout,
}

#[derive(Debug, Clone)]
pub struct UpdateProject {
    pub id: uuid::Uuid,
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<ProjectStatus>,
}

#[derive(Debug, Clone)]
pub struct AddProjectMember {
    pub project_id: uuid::Uuid,
    pub account_id: uuid::Uuid,
    pub role: ProjectRole,
}

#[derive(Debug, Clone)]
pub struct AddProjectMemberByHandle {
    pub project_id: uuid::Uuid,
    pub handle: String,
    pub role: ProjectRole,
}

#[derive(Debug, Clone)]
pub struct SetProjectMemberRole {
    pub project_id: uuid::Uuid,
    pub account_id: uuid::Uuid,
    pub role: ProjectRole,
}

#[derive(Debug, Clone)]
pub struct RemoveProjectMember {
    pub project_id: uuid::Uuid,
    pub account_id: uuid::Uuid,
}

/// The reporter describes what happened and classifies it at intake.
#[derive(Debug, Clone)]
pub struct CreateIssue {
    pub project_id: uuid::Uuid,
    pub title: String,
    pub description: String,
    pub kind: IssueKind,
    pub source: IssueSource,
    pub severity: Option<IssueSeverity>,
    pub area: Option<String>,
    pub labels: Vec<String>,
    pub environment: Option<String>,
    pub affected_versions: Vec<String>,
    pub reproduction: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommentIssue {
    pub project_id: uuid::Uuid,
    pub id: uuid::Uuid,
    pub actor: Actor,
    pub body: String,
    pub internal: bool,
    pub expected_version: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TransitionIssue {
    pub project_id: uuid::Uuid,
    pub id: uuid::Uuid,
    pub actor: Actor,
    pub to: IssueStatus,
    pub expected_version: Option<u32>,
    pub resolution: Option<String>,
    pub duplicate_of: Option<uuid::Uuid>,
}
