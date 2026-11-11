use crate::script::VariableValue;
use crate::{AccountKind, Actor, DomainEventKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

macro_rules! text_key {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;

            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
    };
}

text_key!(
    /// A workflow registered in the process. Tasks fix their workflow at creation.
    WorkflowId
);
text_key!(
    /// A workflow-scoped stage identifier. Not a global enum and not a task state.
    StageKey
);
text_key!(
    /// A workflow-scoped role identifier.
    RoleKey
);
text_key!(
    /// A workflow-defined operation such as `started` or `review_approved`.
    OperationName
);
text_key!(
    /// A workflow-defined terminal label. `Completed` never implies success.
    TaskResult
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    InProgress,
    Completed,
}

/// Reserved platform operation that starts a Ready task.
///
/// It is not declared by a workflow; `apply` handles it before workflow role
/// authorization and runs no stage action.
pub const START_OPERATION: &str = "start";

/// Reserved platform operation that completes a task with result `approved`.
///
/// Used by the requirements-document project-layer approval path to close its
/// goal-clarification task without requiring the acceptor role to be bound as
/// a task actor.
pub const ACCEPT_OPERATION: &str = "accept";

/// Reserved platform operation that returns a task to its initial stage.
///
/// Used by the human requirements-review action: the reason is stored as a
/// document annotation, and the analyst is activated again for revision.
pub const REJECT_OPERATION: &str = "reject";

impl TaskState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed)
    }
}

impl std::str::FromStr for TaskState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            other => Err(format!("unknown task state {other:?}")),
        }
    }
}

/// A read-only scheduling view derived from [`TaskState`] and dependencies.
///
/// It is never persisted and never set by a workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDispatchState {
    Blocked,
    Ready,
    Running,
    Closed,
}

impl TaskDispatchState {
    pub fn derive(state: TaskState, dependencies_ready: bool) -> Self {
        match state {
            TaskState::Pending if dependencies_ready => Self::Ready,
            TaskState::Pending => Self::Blocked,
            TaskState::InProgress => Self::Running,
            TaskState::Completed => Self::Closed,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Closed => "closed",
        }
    }
}

impl std::str::FromStr for TaskDispatchState {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "blocked" => Ok(Self::Blocked),
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "closed" => Ok(Self::Closed),
            other => Err(format!("unknown dispatch state {other:?}")),
        }
    }
}

/// The persisted Task aggregate. Dependencies and actors are separate DAOs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub workflow: WorkflowId,
    pub workflow_version: u32,
    pub title: String,
    pub state: TaskState,
    pub stage: Option<StageKey>,
    pub result: Option<TaskResult>,
    /// Current-stage variable values. Rebuilt on every stage entry. Internal
    /// workflow state; never exposed in the public Task view.
    #[serde(default)]
    pub stage_variables: BTreeMap<String, VariableValue>,
    /// Task variable values. Initialized once when the task is created and
    /// kept across stage changes. Internal workflow state; never exposed in
    /// the public Task view.
    #[serde(default)]
    pub task_variables: BTreeMap<String, VariableValue>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
}

impl TaskDAO {
    pub fn new(
        project_id: Uuid,
        parent_id: Option<Uuid>,
        workflow: WorkflowId,
        workflow_version: u32,
        title: impl Into<String>,
        now: OffsetDateTime,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            project_id,
            parent_id,
            workflow,
            workflow_version,
            title: title.into(),
            state: TaskState::Pending,
            stage: None,
            result: None,
            stage_variables: BTreeMap::new(),
            task_variables: BTreeMap::new(),
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskDependencyDAO {
    pub task_id: Uuid,
    pub depends_on_task_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskActorDAO {
    pub task_id: Uuid,
    pub role_key: RoleKey,
    pub account_id: Uuid,
    pub active: bool,
    pub work: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCommentDAO {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author_account_id: Uuid,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// The workflow view of a dependency: only fields a script may branch on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyState {
    pub task_id: Uuid,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

/// A direct child's state, used for the parent completion gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildState {
    pub task_id: Uuid,
    pub workflow: WorkflowId,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

/// Reader-facing Task snapshot. It is assembled per viewer and never written back.
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub workflow: WorkflowId,
    pub workflow_version: u32,
    pub title: String,
    pub state: TaskState,
    pub dispatch_state: TaskDispatchState,
    pub stage: Option<StageKey>,
    pub result: Option<TaskResult>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub completed_at: Option<OffsetDateTime>,
    pub dependencies: Vec<TaskDependency>,
    pub role_specs: Vec<crate::RoleSpec>,
    pub actors: Vec<TaskActor>,
    pub allowed_operations: Vec<AllowedOperation>,
    pub comments: Vec<TaskComment>,
    pub operations: Vec<TaskOperation>,
}

/// One operation the viewing account may currently send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedOperation {
    pub name: OperationName,
    pub display_name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDependency {
    pub task_id: Uuid,
    pub title: String,
    pub state: TaskState,
    pub result: Option<TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskActor {
    pub role_key: RoleKey,
    pub account_id: Uuid,
    pub display_name: String,
    pub active: bool,
    pub work: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComment {
    pub id: Uuid,
    pub author_account_id: Uuid,
    pub author_display_name: String,
    pub author_kind: AccountKind,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOperation {
    pub id: Uuid,
    pub kind: DomainEventKind,
    pub actor: Actor,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
    pub payload: serde_json::Value,
}

/// A child task produced by a workflow effect, with its immutable dependencies.
#[derive(Debug, Clone)]
pub struct TaskChild {
    pub task: TaskDAO,
    pub dependencies: Vec<Uuid>,
}

/// A child task a `create_issue` action asked to create alongside the Issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIssueTask {
    pub workflow: String,
    pub title: String,
}

/// An Issue a workflow action asked to create for this Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIssueCreate {
    pub kind: String,
    pub title: String,
    pub description: String,
    pub relation: String,
    /// When present, the engine creates this child task and links the Issue to
    /// it; otherwise the Issue links to the task running the action.
    pub task: Option<TaskIssueTask>,
    /// Resolved by the workflow context before the mutation is persisted.
    pub link_task_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentReviewLinkAction {
    Bind,
    Release,
}

/// A document review link effect requested by a workflow stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentReviewLinkIntent {
    pub kind: String,
    pub action: DocumentReviewLinkAction,
}

/// The complete result of applying one trigger event to a Task aggregate.
#[derive(Debug, Clone)]
pub struct TaskMutation {
    pub task: TaskDAO,
    pub actors: Vec<TaskActorDAO>,
    pub dependencies: Vec<DependencyState>,
    pub children: Vec<TaskChild>,
    /// Issues created by `create_issue` statements in this mutation.
    pub issues: Vec<TaskIssueCreate>,
    /// Document review link changes requested by stage entry effects.
    pub review_links: Vec<DocumentReviewLinkIntent>,
    pub events: Vec<crate::DomainEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskError {
    #[error("task is already completed")]
    Completed,
    #[error("dependencies are not completed")]
    DependencyBlocked,
    #[error("agent roles are not fully bound: {0}")]
    UnboundRoles(String),
    #[error("direct child tasks are not completed")]
    ChildrenNotCompleted,
    #[error("invalid operation {0:?} in the current stage")]
    InvalidOperation(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("unknown role {0:?}")]
    UnknownRole(String),
    #[error("unknown stage {0:?}")]
    UnknownStage(String),
    #[error("invalid effect: {0}")]
    InvalidEffect(String),
    #[error("workflow error: {0}")]
    Workflow(String),
}
