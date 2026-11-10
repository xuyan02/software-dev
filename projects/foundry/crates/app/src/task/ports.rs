use crate::RepoError;
use foundry_domain::{
    AccountKind, DomainEvent, OperationName, RoleKey, StageKey, TaskCommentDAO, TaskDAO,
    TaskDispatchState, TaskOperation, TaskResult, TaskState,
};
use uuid::Uuid;

/// Persistence for the Task aggregate and its read-side projections.
///
/// Writes are used only by Task creation, the workflow dispatcher and comment
/// creation. Views are assembled by the application layer from these DAOs; the
/// repository never writes a view object.
#[async_trait::async_trait]
pub trait TaskRepository: Send + Sync {
    async fn insert(
        &self,
        task: &TaskDAO,
        dependencies: &[Uuid],
        events: &[DomainEvent],
    ) -> Result<(), RepoError>;

    async fn get(&self, id: Uuid) -> Result<Option<TaskDAO>, RepoError>;

    async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
    ) -> Result<Vec<TaskListRow>, RepoError>;

    async fn dependencies(&self, task_id: Uuid) -> Result<Vec<TaskDependencyRow>, RepoError>;

    async fn actors(&self, task_id: Uuid) -> Result<Vec<TaskActorRow>, RepoError>;

    async fn comments(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskCommentRow>, RepoError>;

    async fn operations(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, RepoError>;

    async fn add_comment(&self, comment: &TaskCommentDAO) -> Result<(), RepoError>;
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter {
    pub state: Option<TaskState>,
    /// Derived from `state` and dependency readiness; never stored.
    pub dispatch_state: Option<TaskDispatchState>,
    pub stage: Option<StageKey>,
    /// Tasks with a member in this role, active or not.
    pub role: Option<RoleKey>,
    /// Tasks where this account is an active actor.
    pub active_actor: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    /// Case-insensitive substring match on the title.
    pub text: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct TaskListRow {
    pub task: TaskDAO,
    pub dependencies_ready: bool,
}

#[derive(Debug, Clone)]
pub struct TaskDependencyRow {
    pub task_id: Uuid,
    pub title: String,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone)]
pub struct TaskActorRow {
    pub role_key: RoleKey,
    pub account_id: Uuid,
    pub display_name: String,
    pub active: bool,
    pub work: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskCommentRow {
    pub id: Uuid,
    pub author_account_id: Uuid,
    pub author_display_name: String,
    pub author_kind: AccountKind,
    pub body: String,
    pub created_at: time::OffsetDateTime,
}

/// A command that must run inside the `task-workflow` partition.
#[derive(Debug, Clone)]
pub enum TaskCommand {
    SendOperation {
        actor_id: Uuid,
        operation: OperationName,
        idempotency_key: String,
    },
    BindActor {
        role: RoleKey,
        account_id: Uuid,
        bound_by: Uuid,
    },
    /// Re-evaluate the current stage's transitions from persisted state.
    Reevaluate,
}

#[derive(Debug, thiserror::Error)]
pub enum TaskDispatchError {
    #[error("task {0} not found")]
    NotFound(Uuid),
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("storage backend error: {0}")]
    Backend(String),
}

/// Runs the `task-workflow` consumer.
///
/// Implementations lock the consumer cursor before the Task row, append any
/// command event, apply all pending triggers, and commit effects, events and
/// the cursor together.
#[async_trait::async_trait]
pub trait TaskDispatcher: Send + Sync {
    async fn process(
        &self,
        task_id: Uuid,
        command: Option<TaskCommand>,
    ) -> Result<(), TaskDispatchError>;

    /// Append `DispatchRequested` to every Ready task that this dispatcher has
    /// not woken yet. Wakes are remembered per process; a restart may wake a
    /// task once more, which workflows must tolerate.
    async fn dispatch_ready(&self) -> Result<usize, TaskDispatchError>;

    /// Process every partition with unprocessed trigger events.
    async fn process_pending(&self, limit: i64) -> Result<usize, TaskDispatchError>;
}
