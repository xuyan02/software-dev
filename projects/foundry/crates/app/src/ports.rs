use crate::error::{EventError, RepoError, ServiceError};
use foundry_domain::{
    Account, AccountKind, AccountStatus, Actor, DomainEvent, Issue, IssueKind, IssueSourceKind,
    IssueStatus, Project, ProjectStatus, Requirement, RequirementStatus, Session,
};
use uuid::Uuid;

/// Storage for requirements.
///
/// `save` takes the version the caller read so the adapter can implement
/// optimistic concurrency control (compare-and-swap). An adapter that cannot
/// detect conflicts must still return `RepoError::Conflict` on mismatch.
#[async_trait::async_trait]
pub trait RequirementRepository: Send + Sync {
    async fn insert(&self, requirement: &Requirement) -> Result<(), RepoError>;

    async fn save(&self, requirement: &Requirement, expected_version: u32)
    -> Result<(), RepoError>;

    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, RepoError>;

    /// Listing always requires a project. There is no cross-project query path.
    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, RepoError>;
}

#[derive(Debug, Clone, Default)]
pub struct RequirementFilter {
    pub status: Option<RequirementStatus>,
    pub owner: Option<String>,
    /// Case-insensitive substring match on title and problem statement.
    pub text: Option<String>,
}

/// Storage for issues. Same optimistic concurrency contract as requirements.
#[async_trait::async_trait]
pub trait IssueRepository: Send + Sync {
    async fn insert(&self, issue: &Issue) -> Result<(), RepoError>;

    async fn save(&self, issue: &Issue, expected_version: u32) -> Result<(), RepoError>;

    async fn get(&self, id: Uuid) -> Result<Option<Issue>, RepoError>;

    /// Listing always requires a project. There is no cross-project query path.
    async fn list(&self, project_id: Uuid, filter: IssueFilter) -> Result<Vec<Issue>, RepoError>;
}

#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    pub status: Option<IssueStatus>,
    pub kind: Option<IssueKind>,
    pub source: Option<IssueSourceKind>,
    pub assignee: Option<String>,
    /// Case-insensitive substring match on title and description.
    pub text: Option<String>,
}

#[async_trait::async_trait]
pub trait AccountRepository: Send + Sync {
    async fn insert(&self, account: &Account) -> Result<(), RepoError>;

    async fn save(&self, account: &Account, expected_version: u32) -> Result<(), RepoError>;

    async fn get(&self, id: Uuid) -> Result<Option<Account>, RepoError>;

    async fn get_by_handle(&self, handle: &str) -> Result<Option<Account>, RepoError>;

    async fn list(&self, filter: AccountFilter) -> Result<Vec<Account>, RepoError>;
}

#[derive(Debug, Clone, Default)]
pub struct AccountFilter {
    pub kind: Option<AccountKind>,
    pub status: Option<AccountStatus>,
    /// Case-insensitive substring match on handle and display name.
    pub text: Option<String>,
}

#[async_trait::async_trait]
pub trait ProjectRepository: Send + Sync {
    async fn insert(&self, project: &Project) -> Result<(), RepoError>;

    /// Insert a project and its root Task in one transaction, appending the
    /// project and task events atomically.
    async fn insert_with_root(
        &self,
        project: &Project,
        root: &foundry_domain::TaskDAO,
        events: &[DomainEvent],
    ) -> Result<(), RepoError>;

    async fn save(&self, project: &Project, expected_version: u32) -> Result<(), RepoError>;

    async fn get(&self, id: Uuid) -> Result<Option<Project>, RepoError>;

    async fn list(&self, filter: ProjectFilter) -> Result<Vec<Project>, RepoError>;
}

#[derive(Debug, Clone, Default)]
pub struct ProjectFilter {
    pub status: Option<ProjectStatus>,
    /// Case-insensitive substring match on the project name.
    pub text: Option<String>,
}

#[async_trait::async_trait]
pub trait SessionRepository: Send + Sync {
    async fn insert(&self, session: &Session) -> Result<(), RepoError>;

    async fn get_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, RepoError>;

    async fn save(&self, session: &Session) -> Result<(), RepoError>;
}

/// Append-only sink for domain events.
#[async_trait::async_trait]
pub trait EventSink: Send + Sync {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), EventError>;
}

/// What an actor is trying to do. Policies are evaluated against this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    CreateRequirement,
    TransitionRequirement {
        from: RequirementStatus,
        to: RequirementStatus,
    },
    CreateIssue,
    CommentIssue,
    TriageIssue,
    TransitionIssue {
        from: IssueStatus,
        to: IssueStatus,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Requirements,
    Requirement(Uuid),
    Issues,
    Issue(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionContext {
    pub actor: Actor,
    pub action: Action,
    pub resource: Resource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDenied {
    pub reason: String,
}

impl PolicyDenied {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl From<PolicyDenied> for ServiceError {
    fn from(denied: PolicyDenied) -> Self {
        ServiceError::Forbidden(denied.reason)
    }
}

/// Decide whether an action is allowed.
///
/// The real implementation is expected to call an external policy engine
/// (OPA, Cedar, ...) with the actor, action and resource. The MVP ships a
/// hard-coded demo policy.
pub trait PolicyEngine: Send + Sync {
    fn check(&self, context: &ActionContext) -> Result<(), PolicyDenied>;
}
