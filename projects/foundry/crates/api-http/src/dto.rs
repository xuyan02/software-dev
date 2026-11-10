use foundry_app::{
    AddProjectMemberByHandle, CommentIssue, CreateAccount, CreateIssue, CreateProject,
    CreateRequirement, CreateWorkflow, RenameWorkflow, SaveWorkflowDraft, SetProjectMemberRole,
    TaskFilter, TransitionIssue, TransitionRequirement, UpdateAccount, UpdateProject,
    UpdateRequirement, WorkflowScope,
};
use foundry_domain::{
    AcceptanceScenario, AccountKind, AccountRole, AccountStatus, Actor, IssueKind,
    IssueSeverity, IssueSource, IssueSourceKind, IssueStatus, Priority, ProjectBriefBody,
    ProjectRole, ProjectStatus, RequirementStatus, Risk, RiskClass, RiskLevel,
    RoleKey, StageKey, TaskDispatchState, TaskState, Term, WorkflowBody, WorkflowId,
    WorkflowVersionRef,
};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub handle: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: time::OffsetDateTime,
    pub account: foundry_domain::Account,
}

#[derive(Debug, Deserialize, Default)]
pub struct LoginQuery {
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LoginForm {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub next: String,
}

/// Step one of first-run setup: sign in with the initial identity configured in
/// the config file. This identity is only an authorization gate; it is never
/// stored as an account.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SetupLoginForm {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub password: String,
}

/// Step two of first-run setup: the operator creates the real administrator
/// account after the initial identity has been verified.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SetupAdminForm {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub confirm_password: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct SelectProjectForm {
    #[serde(default)]
    pub project: String,
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateAccountRequest {
    pub handle: String,
    pub display_name: String,
    #[serde(default)]
    pub kind: AccountKind,
    #[serde(default)]
    pub role: AccountRole,
    #[serde(default)]
    pub email: Option<String>,
    pub password: String,
}

impl CreateAccountRequest {
    pub fn into_command(self) -> CreateAccount {
        CreateAccount {
            handle: self.handle,
            display_name: self.display_name,
            kind: self.kind,
            role: self.role,
            email: self.email,
            password: self.password,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateAccountRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<Option<String>>,
    #[serde(default)]
    pub role: Option<AccountRole>,
    #[serde(default)]
    pub status: Option<AccountStatus>,
    #[serde(default)]
    pub new_password: Option<String>,
}

impl UpdateAccountRequest {
    pub fn into_command(self, id: uuid::Uuid) -> UpdateAccount {
        UpdateAccount {
            id,
            display_name: self.display_name,
            email: self.email,
            role: self.role,
            status: self.status,
            new_password: self.new_password,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NewAccountForm {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub password: String,
}

impl NewAccountForm {
    pub fn into_command(self) -> CreateAccount {
        CreateAccount {
            handle: self.handle.trim().to_string(),
            display_name: self.display_name.trim().to_string(),
            kind: parse_account_kind(&self.kind),
            role: parse_account_role(&self.role),
            email: optional_text(&self.email),
            password: self.password,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AccountUpdateForm {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub new_password: String,
}

impl AccountUpdateForm {
    pub fn into_command(self, id: uuid::Uuid) -> UpdateAccount {
        UpdateAccount {
            id,
            display_name: (!self.display_name.trim().is_empty())
                .then(|| self.display_name.trim().to_string()),
            email: Some(optional_text(&self.email)),
            role: Some(parse_account_role(&self.role)),
            status: Some(parse_account_status(&self.status)),
            new_password: optional_text(&self.new_password),
        }
    }
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub root_workflow_id: String,
}

impl CreateProjectRequest {
    pub fn into_command(self) -> CreateProject {
        CreateProject {
            name: self.name,
            description: self.description,
            root_workflow_id: WorkflowId::new(self.root_workflow_id),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateProjectRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub status: Option<ProjectStatus>,
}

impl UpdateProjectRequest {
    pub fn into_command(self, id: uuid::Uuid) -> UpdateProject {
        UpdateProject {
            id,
            name: self.name,
            description: self.description,
            status: self.status,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub handle: String,
    pub role: ProjectRole,
}

impl AddMemberRequest {
    pub fn into_command(self, project_id: uuid::Uuid) -> AddProjectMemberByHandle {
        AddProjectMemberByHandle {
            project_id,
            handle: self.handle,
            role: self.role,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetMemberRoleRequest {
    pub role: ProjectRole,
}

impl SetMemberRoleRequest {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        account_id: uuid::Uuid,
    ) -> SetProjectMemberRole {
        SetProjectMemberRole {
            project_id,
            account_id,
            role: self.role,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct NewProjectForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub root_workflow_id: String,
}

impl NewProjectForm {
    pub fn into_command(self) -> CreateProject {
        CreateProject {
            name: self.name.trim().to_string(),
            description: optional_text(&self.description),
            root_workflow_id: WorkflowId::new(self.root_workflow_id.trim()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProjectUpdateForm {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub status: String,
}

impl ProjectUpdateForm {
    pub fn into_command(self, id: uuid::Uuid) -> UpdateProject {
        UpdateProject {
            id,
            name: (!self.name.trim().is_empty()).then(|| self.name.trim().to_string()),
            description: Some(optional_text(&self.description)),
            status: if self.status.is_empty() {
                None
            } else {
                Some(parse_project_status(&self.status))
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct AddMemberForm {
    #[serde(default)]
    pub handle: String,
    #[serde(default)]
    pub role: String,
}

impl AddMemberForm {
    pub fn into_command(self, project_id: uuid::Uuid) -> AddProjectMemberByHandle {
        AddProjectMemberByHandle {
            project_id,
            handle: self.handle.trim().to_string(),
            role: parse_project_role(&self.role),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MemberRoleForm {
    #[serde(default)]
    pub role: String,
}

impl MemberRoleForm {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        account_id: uuid::Uuid,
    ) -> SetProjectMemberRole {
        SetProjectMemberRole {
            project_id,
            account_id,
            role: parse_project_role(&self.role),
        }
    }
}

// ---------------------------------------------------------------------------
// Requirements
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateRequirementRequest {
    pub title: String,
    pub problem: String,
    #[serde(default)]
    pub users: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<AcceptanceScenario>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub risk: RiskClass,
}

impl CreateRequirementRequest {
    pub fn into_command(self, project_id: uuid::Uuid) -> CreateRequirement {
        CreateRequirement {
            project_id,
            title: self.title,
            problem: self.problem,
            users: self.users,
            acceptance: self.acceptance,
            non_goals: self.non_goals,
            constraints: self.constraints,
            priority: self.priority,
            risk: self.risk,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TransitionRequirementRequest {
    pub to: RequirementStatus,
    #[serde(default)]
    pub expected_version: Option<u32>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequirementRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub problem: Option<String>,
    #[serde(default)]
    pub users: Option<Vec<String>>,
    #[serde(default)]
    pub acceptance: Option<Vec<AcceptanceScenario>>,
    #[serde(default)]
    pub non_goals: Option<Vec<String>>,
    #[serde(default)]
    pub constraints: Option<Vec<String>>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub risk: Option<RiskClass>,
    #[serde(default)]
    pub expected_version: Option<u32>,
}

impl UpdateRequirementRequest {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        id: uuid::Uuid,
        actor: Actor,
    ) -> UpdateRequirement {
        UpdateRequirement {
            project_id,
            id,
            actor,
            title: self.title,
            problem: self.problem,
            users: self.users,
            acceptance: self.acceptance,
            non_goals: self.non_goals,
            constraints: self.constraints,
            priority: self.priority,
            risk: self.risk,
            expected_version: self.expected_version,
        }
    }
}

pub fn transition_command(
    project_id: uuid::Uuid,
    id: uuid::Uuid,
    actor: Actor,
    request: TransitionRequirementRequest,
) -> TransitionRequirement {
    TransitionRequirement {
        project_id,
        id,
        to: request.to,
        actor,
        expected_version: request.expected_version,
        reason: request.reason,
    }
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub status: Option<RequirementStatus>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

/// HTML form submission for creating a requirement from the web console.
///
/// Everything is a string so a validation failure can re-render the form with
/// the user's input intact instead of returning a bare 400.
#[derive(Debug, Clone, Deserialize)]
pub struct NewRequirementForm {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub users: String,
    #[serde(default)]
    pub scenario_title: String,
    #[serde(default)]
    pub given: String,
    #[serde(default)]
    pub when: String,
    #[serde(default)]
    pub then: String,
    #[serde(default)]
    pub non_goals: String,
    #[serde(default)]
    pub constraints: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub risk: String,
}

impl NewRequirementForm {
    pub fn values(&self) -> foundry_web::RequirementFormValues {
        foundry_web::RequirementFormValues {
            title: self.title.clone(),
            problem: self.problem.clone(),
            users: self.users.clone(),
            scenario_title: self.scenario_title.clone(),
            given: self.given.clone(),
            when: self.when.clone(),
            then: self.then.clone(),
            non_goals: self.non_goals.clone(),
            constraints: self.constraints.clone(),
            priority: if self.priority.is_empty() {
                "p3".to_string()
            } else {
                self.priority.clone()
            },
            risk: if self.risk.is_empty() {
                "medium".to_string()
            } else {
                self.risk.clone()
            },
        }
    }

    pub fn into_command(self, project_id: uuid::Uuid) -> CreateRequirement {
        let acceptance = if self.scenario_title.trim().is_empty()
            && self.given.trim().is_empty()
            && self.when.trim().is_empty()
            && self.then.trim().is_empty()
        {
            Vec::new()
        } else {
            vec![AcceptanceScenario {
                id: "S-1".to_string(),
                title: self.scenario_title.trim().to_string(),
                given: lines(&self.given),
                when: lines(&self.when),
                then: lines(&self.then),
            }]
        };

        CreateRequirement {
            project_id,
            title: self.title.trim().to_string(),
            problem: self.problem.trim().to_string(),
            users: lines(&self.users),
            acceptance,
            non_goals: lines(&self.non_goals),
            constraints: lines(&self.constraints),
            priority: match self.priority.as_str() {
                "p0" => Priority::P0,
                "p1" => Priority::P1,
                "p2" => Priority::P2,
                _ => Priority::P3,
            },
            risk: match self.risk.as_str() {
                "low" => RiskClass::Low,
                "high" => RiskClass::High,
                "critical" => RiskClass::Critical,
                _ => RiskClass::Medium,
            },
        }
    }
}

fn lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

// ---------------------------------------------------------------------------
// Issues
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateIssueRequest {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub kind: IssueKind,
    #[serde(default)]
    pub source: IssueSource,
    #[serde(default)]
    pub severity: Option<IssueSeverity>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub affected_versions: Vec<String>,
    #[serde(default)]
    pub reproduction: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub actual: Option<String>,
}

impl CreateIssueRequest {
    pub fn into_command(self, project_id: uuid::Uuid) -> CreateIssue {
        CreateIssue {
            project_id,
            title: self.title,
            description: self.description,
            kind: self.kind,
            source: self.source,
            severity: self.severity,
            area: self.area,
            labels: self.labels,
            environment: self.environment,
            affected_versions: self.affected_versions,
            reproduction: self.reproduction,
            expected: self.expected,
            actual: self.actual,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CommentIssueRequest {
    pub body: String,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub expected_version: Option<u32>,
}

impl CommentIssueRequest {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        id: uuid::Uuid,
        actor: Actor,
    ) -> CommentIssue {
        CommentIssue {
            project_id,
            id,
            actor,
            body: self.body,
            internal: self.internal,
            expected_version: self.expected_version,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TransitionIssueRequest {
    pub to: IssueStatus,
    #[serde(default)]
    pub expected_version: Option<u32>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub duplicate_of: Option<uuid::Uuid>,
}

impl TransitionIssueRequest {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        id: uuid::Uuid,
        actor: Actor,
    ) -> TransitionIssue {
        TransitionIssue {
            project_id,
            id,
            actor,
            to: self.to,
            expected_version: self.expected_version,
            resolution: self.resolution,
            duplicate_of: self.duplicate_of,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct IssueListQuery {
    #[serde(default)]
    pub status: Option<IssueStatus>,
    #[serde(default)]
    pub kind: Option<IssueKind>,
    #[serde(default)]
    pub source: Option<IssueSourceKind>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub project: Option<String>,
}

/// HTML form submission for creating an issue from the web console.
#[derive(Debug, Clone, Deserialize)]
pub struct NewIssueForm {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default)]
    pub source_reference: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub area: String,
    #[serde(default)]
    pub labels: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub affected_versions: String,
    #[serde(default)]
    pub reproduction: String,
    #[serde(default)]
    pub expected: String,
    #[serde(default)]
    pub actual: String,
}

impl NewIssueForm {
    pub fn values(&self) -> foundry_web::IssueFormValues {
        foundry_web::IssueFormValues {
            title: self.title.clone(),
            description: self.description.clone(),
            kind: default_if_empty(&self.kind, "feedback"),
            source_kind: default_if_empty(&self.source_kind, "internal"),
            source_reference: self.source_reference.clone(),
            severity: self.severity.clone(),
            area: self.area.clone(),
            labels: self.labels.clone(),
            environment: self.environment.clone(),
            affected_versions: self.affected_versions.clone(),
            reproduction: self.reproduction.clone(),
            expected: self.expected.clone(),
            actual: self.actual.clone(),
        }
    }

    pub fn into_command(self, project_id: uuid::Uuid) -> CreateIssue {
        CreateIssue {
            project_id,
            title: self.title.trim().to_string(),
            description: self.description.trim().to_string(),
            kind: parse_issue_kind(&self.kind),
            source: IssueSource {
                kind: parse_issue_source_kind(&self.source_kind),
                reference: optional_text(&self.source_reference),
            },
            severity: parse_issue_severity(&self.severity),
            area: optional_text(&self.area),
            labels: lines(&self.labels),
            environment: optional_text(&self.environment),
            affected_versions: lines(&self.affected_versions),
            reproduction: optional_text(&self.reproduction),
            expected: optional_text(&self.expected),
            actual: optional_text(&self.actual),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommentIssueForm {
    #[serde(default)]
    pub body: String,
    /// Checkboxes submit `on` when checked and are absent otherwise.
    #[serde(default)]
    pub internal: String,
}

impl CommentIssueForm {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        id: uuid::Uuid,
        actor: Actor,
    ) -> CommentIssue {
        CommentIssue {
            project_id,
            id,
            actor,
            body: self.body.trim().to_string(),
            internal: !self.internal.is_empty(),
            expected_version: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueTransitionForm {
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub resolution: String,
    #[serde(default)]
    pub duplicate_of: String,
}

impl IssueTransitionForm {
    pub fn into_command(
        self,
        project_id: uuid::Uuid,
        id: uuid::Uuid,
        actor: Actor,
    ) -> Result<TransitionIssue, String> {
        let duplicate_of = optional_text(&self.duplicate_of)
            .map(|value| value.parse::<uuid::Uuid>())
            .transpose()
            .map_err(|_| "重复的目标议题 ID 不是合法 UUID".to_string())?;
        Ok(TransitionIssue {
            project_id,
            id,
            actor,
            to: parse_issue_status(&self.to),
            expected_version: None,
            resolution: optional_text(&self.resolution),
            duplicate_of,
        })
    }
}

fn default_if_empty(value: &str, fallback: &str) -> String {
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

pub fn parse_account_kind(value: &str) -> AccountKind {
    match value {
        "agent" => AccountKind::Agent,
        _ => AccountKind::Human,
    }
}

pub fn parse_account_role(value: &str) -> AccountRole {
    match value {
        "admin" => AccountRole::Admin,
        _ => AccountRole::Member,
    }
}

pub fn parse_account_status(value: &str) -> AccountStatus {
    match value {
        "suspended" => AccountStatus::Suspended,
        _ => AccountStatus::Active,
    }
}

pub fn parse_project_role(value: &str) -> ProjectRole {
    match value {
        "viewer" => ProjectRole::Viewer,
        "maintainer" => ProjectRole::Maintainer,
        "owner" => ProjectRole::Owner,
        _ => ProjectRole::Member,
    }
}

pub fn parse_project_status(value: &str) -> ProjectStatus {
    match value {
        "archived" => ProjectStatus::Archived,
        _ => ProjectStatus::Active,
    }
}

pub fn parse_issue_kind(value: &str) -> IssueKind {
    match value {
        "bug" => IssueKind::Bug,
        "question" => IssueKind::Question,
        "idea" => IssueKind::Idea,
        "task_request" => IssueKind::TaskRequest,
        "incident" => IssueKind::Incident,
        _ => IssueKind::Feedback,
    }
}

pub fn parse_issue_source_kind(value: &str) -> IssueSourceKind {
    match value {
        "customer" => IssueSourceKind::Customer,
        "support" => IssueSourceKind::Support,
        "monitoring" => IssueSourceKind::Monitoring,
        "analytics" => IssueSourceKind::Analytics,
        "incident" => IssueSourceKind::Incident,
        "other" => IssueSourceKind::Other,
        _ => IssueSourceKind::Internal,
    }
}

pub fn parse_issue_severity(value: &str) -> Option<IssueSeverity> {
    match value {
        "s1" => Some(IssueSeverity::S1),
        "s2" => Some(IssueSeverity::S2),
        "s3" => Some(IssueSeverity::S3),
        "s4" => Some(IssueSeverity::S4),
        _ => None,
    }
}

pub fn parse_issue_status(value: &str) -> IssueStatus {
    match value {
        "in_progress" => IssueStatus::InProgress,
        "resolved" => IssueStatus::Resolved,
        "closed" => IssueStatus::Closed,
        "rejected" => IssueStatus::Rejected,
        "duplicate" => IssueStatus::Duplicate,
        _ => IssueStatus::Open,
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskListQuery {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub dispatch_state: Option<String>,
    #[serde(default)]
    pub stage: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub active_actor: Option<uuid::Uuid>,
    #[serde(default)]
    pub parent_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

impl TaskListQuery {
    pub fn into_filter(self) -> Result<TaskFilter, String> {
        let parse = |value: Option<&str>| -> Result<Option<TaskState>, String> {
            value.map(str::parse).transpose()
        };
        let state = parse(self.state.as_deref())?;
        let dispatch_state = self
            .dispatch_state
            .as_deref()
            .map(str::parse::<TaskDispatchState>)
            .transpose()?;
        Ok(TaskFilter {
            state,
            dispatch_state,
            stage: self.stage.map(StageKey::new),
            role: self.role.map(RoleKey::new),
            active_actor: self.active_actor,
            parent_id: self.parent_id,
            text: self.text,
            limit: self.limit,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PageQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

impl PageQuery {
    pub fn page(&self, default: i64) -> (i64, i64) {
        (
            self.limit.unwrap_or(default).clamp(1, 200),
            self.offset.unwrap_or(0).max(0),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct TaskOperationRequest {
    pub operation: String,
    pub idempotency_key: String,
}

#[derive(Debug, Deserialize)]
pub struct TaskCommentRequest {
    pub body: String,
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateWorkflowRequest {
    pub title: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub copy_from: Option<WorkflowVersionRef>,
}

impl CreateWorkflowRequest {
    pub fn into_command(self) -> CreateWorkflow {
        CreateWorkflow {
            title: self.title,
            scope: parse_workflow_scope(self.scope.as_deref()),
            copy_from: self.copy_from,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RenameWorkflowRequest {
    pub title: String,
}

impl RenameWorkflowRequest {
    pub fn into_command(self, id: WorkflowId) -> RenameWorkflow {
        RenameWorkflow {
            id,
            title: self.title,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SaveDraftRequest {
    pub body: WorkflowBody,
    pub client_save_id: uuid::Uuid,
}

#[derive(Debug, Deserialize)]
pub struct ValidateWorkflowRequest {
    pub body: WorkflowBody,
}

/// HTML form for creating an account workflow, optionally copied from a
/// platform version selected as `workflow_id@version`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NewWorkflowForm {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub copy_from: String,
}

impl NewWorkflowForm {
    pub fn into_command(self) -> CreateWorkflow {
        CreateWorkflow {
            title: self.title.trim().to_string(),
            scope: parse_workflow_scope(optional_text(&self.scope).as_deref()),
            copy_from: parse_version_ref(&self.copy_from),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RenameWorkflowForm {
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PublishWorkflowForm {
    #[serde(default)]
    pub revision: String,
}

pub fn parse_workflow_scope(value: Option<&str>) -> WorkflowScope {
    match value {
        Some("platform") => WorkflowScope::Platform,
        _ => WorkflowScope::Account,
    }
}

/// Parses the console's `workflow_id@version` copy source select value.
pub fn parse_version_ref(value: &str) -> Option<WorkflowVersionRef> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let (workflow_id, version) = value.rsplit_once('@')?;
    Some(WorkflowVersionRef {
        workflow_id: WorkflowId::new(workflow_id),
        version: version.parse().ok()?,
    })
}

impl SaveDraftRequest {
    pub fn into_command(self, id: WorkflowId, expected_revision: u64) -> SaveWorkflowDraft {
        SaveWorkflowDraft {
            id,
            body: self.body,
            expected_revision,
            client_save_id: self.client_save_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Requirements document
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
pub struct ApplyRequirementsOperationsRequest {
    #[serde(default)]
    pub operations: Vec<foundry_domain::RequirementsOperation>,
}

// ---------------------------------------------------------------------------
// Project brief
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ApplyProjectBriefDraftOperationRequest {
    pub op_id: uuid::Uuid,
    pub base_revision: u64,
    pub operation: foundry_domain::ProjectBriefDraftOperation,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProjectBriefForm {
    #[serde(default)]
    pub draft_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub problem: String,
    #[serde(default)]
    pub goals: String,
    #[serde(default)]
    pub non_goals: String,
    #[serde(default)]
    pub constraints: String,
    #[serde(default)]
    pub risks: String,
    #[serde(default)]
    pub terms: String,
}

impl ProjectBriefForm {
    /// Parse the text form into the structured brief body.
    ///
    /// Lists use one entry per line; structured rows use `|` between fields.
    pub fn into_parts(
        self,
    ) -> Result<(Option<uuid::Uuid>, u64, String, ProjectBriefBody), String> {
        let risks = parse_risks(&self.risks)?;
        let terms = parse_terms(&self.terms)?;
        Ok((
            self.draft_id,
            self.revision,
            self.action,
            ProjectBriefBody {
                schema_version: foundry_domain::PROJECT_BRIEF_SCHEMA_VERSION,
                problem: self.problem.trim().to_string(),
                goals: lines(&self.goals),
                non_goals: lines(&self.non_goals),
                constraints: lines(&self.constraints),
                risks,
                terms,
            },
        ))
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ProjectBriefApproveForm {
    #[serde(default)]
    pub version: u32,
}

#[derive(Debug, Default, Deserialize)]
pub struct RejectProjectBriefForm {
    #[serde(default)]
    pub version: u32,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProjectBriefDraftQuery {
    #[serde(default)]
    pub draft: Option<uuid::Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct AnnotateProjectBriefRequest {
    pub target_type: foundry_domain::ProjectBriefAnnotationTarget,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct AnnotateProjectBriefForm {
    pub target_type: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolveProjectBriefAnnotationRequest {
    pub resolution: String,
}

#[derive(Debug, Deserialize)]
pub struct ProjectBriefAnnotationQuery {
    #[serde(default)]
    pub include_resolved: bool,
}

#[derive(Debug, Deserialize)]
pub struct RequirementsRevisionForm {
    #[serde(default)]
    pub revision: u64,
}

#[derive(Debug, Deserialize)]
pub struct AnnotateRequirementsRequest {
    pub target_type: foundry_domain::AnnotationTarget,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct AnnotateRequirementsForm {
    pub target_type: String,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default)]
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct ResolveRequirementsAnnotationRequest {
    pub resolution: String,
}

#[derive(Debug, Deserialize)]
pub struct RequirementsAnnotationQuery {
    #[serde(default)]
    pub include_resolved: bool,
}

#[derive(Debug, Deserialize)]
pub struct RejectRequirementsForm {
    pub reason: String,
}

fn parse_terms(value: &str) -> Result<Vec<Term>, String> {
    let mut terms = Vec::new();
    for (index, line) in lines(value).iter().enumerate() {
        let parts = split_row(line);
        if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(format!("术语第 {} 行需要：术语 | 解释", index + 1));
        }
        terms.push(Term {
            term: parts[0].to_string(),
            definition: parts[1].to_string(),
        });
    }
    Ok(terms)
}

fn parse_risks(value: &str) -> Result<Vec<Risk>, String> {
    let mut risks = Vec::new();
    for (index, line) in lines(value).iter().enumerate() {
        let parts = split_row(line);
        if parts.len() < 5 {
            return Err(format!(
                "风险第 {} 行需要：描述 | 概率(low/medium/high) | 影响(low/medium/high) | 缓解 | 负责人",
                index + 1
            ));
        }
        risks.push(Risk {
            id: format!("r{}", index + 1),
            description: parts[0].to_string(),
            probability: parse_risk_level(&parts[1]),
            impact: parse_risk_level(&parts[2]),
            mitigation: parts[3].to_string(),
            owner: parts[4].to_string(),
        });
    }
    Ok(risks)
}

fn split_row(line: &str) -> Vec<String> {
    line.split('|')
        .map(|part| part.trim().to_string())
        .collect()
}

fn parse_risk_level(value: &str) -> RiskLevel {
    match value.to_ascii_lowercase().as_str() {
        "low" => RiskLevel::Low,
        "high" => RiskLevel::High,
        _ => RiskLevel::Medium,
    }
}

// ---------------------------------------------------------------------------
// Agent dispatch
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CreateAgentFromTemplateRequest {
    pub task_id: uuid::Uuid,
    pub role: String,
    pub template_id: String,
}

impl CreateAgentFromTemplateRequest {
    pub fn into_command(self) -> foundry_domain::CreateAgentFromTemplate {
        foundry_domain::CreateAgentFromTemplate {
            task_id: self.task_id,
            role: self.role,
            template_id: self.template_id,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AssignAgentRequest {
    pub task_id: uuid::Uuid,
    pub role: String,
    pub agent_definition_id: uuid::Uuid,
}

impl AssignAgentRequest {
    pub fn into_command(self) -> foundry_domain::AssignAgentToTask {
        foundry_domain::AssignAgentToTask {
            task_id: self.task_id,
            role: self.role,
            agent_definition_id: self.agent_definition_id,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct AgentTemplateQuery {
    #[serde(default)]
    pub role: Option<String>,
}

// ---------------------------------------------------------------------------
// Task links
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AddTaskLinkRequest {
    pub kind: String,
    #[serde(default)]
    pub target_id: Option<uuid::Uuid>,
    #[serde(default)]
    pub url: Option<String>,
    pub title: String,
    pub relation: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
