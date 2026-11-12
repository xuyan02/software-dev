use crate::{
    RepoError, ServiceError, TaskActorRow, TaskCommand, TaskCommentRow, TaskDependencyRow,
    TaskDispatchError, TaskDispatcher, TaskFilter, TaskListRow, TaskRepository, WorkflowCatalog,
};
use foundry_domain::{
    AllowedOperation, OperationName, StageKey, Task, TaskActor, TaskComment, TaskCommentDAO,
    TaskDAO, TaskDependency, TaskDispatchState, TaskOperation, WorkflowVersionRef,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

const VIEW_LIMIT: i64 = 50;

#[derive(Debug, Clone, Copy)]
pub struct TaskViewer {
    pub account_id: Option<Uuid>,
    pub can_see_all: bool,
}

impl TaskViewer {
    pub fn member(account_id: Uuid) -> Self {
        Self {
            account_id: Some(account_id),
            can_see_all: false,
        }
    }

    pub fn manager(account_id: Uuid) -> Self {
        Self {
            account_id: Some(account_id),
            can_see_all: true,
        }
    }

    pub fn system() -> Self {
        Self {
            account_id: None,
            can_see_all: true,
        }
    }
}

pub struct TaskService {
    repository: Arc<dyn TaskRepository>,
    dispatcher: Arc<dyn TaskDispatcher>,
    workflows: Arc<dyn WorkflowCatalog>,
}

impl TaskService {
    pub fn new(
        repository: Arc<dyn TaskRepository>,
        dispatcher: Arc<dyn TaskDispatcher>,
        workflows: Arc<dyn WorkflowCatalog>,
    ) -> Self {
        Self {
            repository,
            dispatcher,
            workflows,
        }
    }

    /// Resolve the project that owns a task. Tasks never move between projects.
    pub async fn project_of(&self, id: Uuid) -> Result<Uuid, ServiceError> {
        self.repository
            .get(id)
            .await?
            .map(|task| task.project_id)
            .ok_or_else(|| ServiceError::not_found("task", id))
    }

    pub async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError> {
        let task = self.load(project_id, id).await?;
        self.view(task, viewer, true).await
    }

    pub async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
        viewer: TaskViewer,
    ) -> Result<Vec<Task>, ServiceError> {
        let rows = self.repository.list(project_id, filter).await?;
        let mut tasks = Vec::with_capacity(rows.len());
        for row in rows {
            tasks.push(self.view_row(row, viewer, false).await?);
        }
        Ok(tasks)
    }

    pub async fn send_operation(
        &self,
        project_id: Uuid,
        id: Uuid,
        actor_id: Uuid,
        operation: OperationName,
        idempotency_key: String,
    ) -> Result<(), ServiceError> {
        if operation.as_str().trim().is_empty() {
            return Err(ServiceError::Validation(
                "operation is required".to_string(),
            ));
        }
        if idempotency_key.trim().is_empty() {
            return Err(ServiceError::Validation(
                "idempotency key is required".to_string(),
            ));
        }
        self.load(project_id, id).await?;
        self.dispatcher
            .process(
                id,
                Some(TaskCommand::SendOperation {
                    actor_id,
                    operation,
                    idempotency_key,
                }),
            )
            .await
            .map_err(dispatch_error)
    }

    pub async fn bind_actor(
        &self,
        project_id: Uuid,
        id: Uuid,
        role: foundry_domain::RoleKey,
        account_id: Uuid,
        bound_by: Uuid,
    ) -> Result<(), ServiceError> {
        self.load(project_id, id).await?;
        self.dispatcher
            .process(
                id,
                Some(TaskCommand::BindActor {
                    role,
                    account_id,
                    bound_by,
                }),
            )
            .await
            .map_err(dispatch_error)
    }

    pub async fn add_comment(
        &self,
        project_id: Uuid,
        id: Uuid,
        author_account_id: Uuid,
        body: impl Into<String>,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError> {
        let body = body.into().trim().to_string();
        if body.is_empty() {
            return Err(ServiceError::Validation(
                "comment body is required".to_string(),
            ));
        }
        self.load(project_id, id).await?;
        let comment = TaskCommentDAO {
            id: Uuid::now_v7(),
            task_id: id,
            author_account_id,
            body,
            created_at: OffsetDateTime::now_utc(),
        };
        self.repository.add_comment(&comment).await?;
        self.get(project_id, id, viewer).await
    }

    pub async fn comments(
        &self,
        project_id: Uuid,
        id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskComment>, ServiceError> {
        self.load(project_id, id).await?;
        Ok(self
            .repository
            .comments(id, limit, offset)
            .await?
            .into_iter()
            .map(comment_view)
            .collect())
    }

    pub async fn history(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, ServiceError> {
        self.load(project_id, id).await?;
        Ok(self
            .repository
            .operations(id, limit, offset)
            .await?
            .into_iter()
            .map(|operation| redact_operation(operation, viewer))
            .collect())
    }

    async fn load(&self, project_id: Uuid, id: Uuid) -> Result<TaskDAO, ServiceError> {
        let task = self
            .repository
            .get(id)
            .await?
            .ok_or_else(|| ServiceError::not_found("task", id))?;
        if task.project_id != project_id {
            return Err(ServiceError::not_found("task", id));
        }
        Ok(task)
    }

    async fn view_row(
        &self,
        row: TaskListRow,
        viewer: TaskViewer,
        full: bool,
    ) -> Result<Task, ServiceError> {
        let TaskListRow {
            task,
            dependencies_ready,
        } = row;
        self.build(task, dependencies_ready, viewer, full).await
    }

    async fn view(
        &self,
        task: TaskDAO,
        viewer: TaskViewer,
        full: bool,
    ) -> Result<Task, ServiceError> {
        let dependencies = if full {
            self.repository.dependencies(task.id).await?
        } else {
            Vec::new()
        };
        let ready = if full {
            dependencies
                .iter()
                .all(|dependency| dependency.state.is_terminal())
        } else {
            true
        };
        self.build_with_dependencies(task, dependencies, ready, viewer, full)
            .await
    }

    async fn build(
        &self,
        task: TaskDAO,
        dependencies_ready: bool,
        viewer: TaskViewer,
        full: bool,
    ) -> Result<Task, ServiceError> {
        let dependencies = if full {
            self.repository.dependencies(task.id).await?
        } else {
            Vec::new()
        };
        self.build_with_dependencies(task, dependencies, dependencies_ready, viewer, full)
            .await
    }

    async fn build_with_dependencies(
        &self,
        task: TaskDAO,
        dependencies: Vec<TaskDependencyRow>,
        dependencies_ready: bool,
        viewer: TaskViewer,
        full: bool,
    ) -> Result<Task, ServiceError> {
        let workflow = self
            .workflows
            .pinned(&WorkflowVersionRef {
                workflow_id: task.workflow.clone(),
                version: task.workflow_version,
            })
            .await?
            .ok_or_else(|| {
                ServiceError::Internal(format!(
                    "workflow {} version {} is not registered",
                    task.workflow, task.workflow_version
                ))
            })?;

        let actor_rows = self.repository.actors(task.id).await?;
        let allowed_operations = if task.state.is_terminal() {
            Vec::new()
        } else {
            let stage = task.stage.as_ref().map(StageKey::as_str).unwrap_or("");
            let active_roles: BTreeSet<&str> = viewer
                .account_id
                .map(|account_id| {
                    actor_rows
                        .iter()
                        .filter(|actor| actor.account_id == account_id && actor.active)
                        .map(|actor| actor.role_key.as_str())
                        .collect()
                })
                .unwrap_or_default();
            workflow
                .stage(stage)
                .map(|stage| {
                    stage
                        .operations
                        .iter()
                        .filter(|operation| {
                            operation
                                .roles
                                .iter()
                                .any(|role| active_roles.contains(role.as_str()))
                        })
                        .map(|operation| AllowedOperation {
                            name: OperationName::new(format!("{}:{}", stage.name, operation.name)),
                            display_name: operation.display_name.clone(),
                            description: operation.description.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let actors = visible_actors(actor_rows, viewer);
        let comments = if full {
            self.repository
                .comments(task.id, VIEW_LIMIT, 0)
                .await?
                .into_iter()
                .map(comment_view)
                .collect()
        } else {
            Vec::new()
        };
        let operations = if full {
            self.repository
                .operations(task.id, VIEW_LIMIT, 0)
                .await?
                .into_iter()
                .map(|operation| redact_operation(operation, viewer))
                .collect()
        } else {
            Vec::new()
        };

        Ok(Task {
            id: task.id,
            project_id: task.project_id,
            parent_id: task.parent_id,
            workflow: task.workflow,
            workflow_version: task.workflow_version,
            title: task.title,
            state: task.state,
            dispatch_state: TaskDispatchState::derive(task.state, dependencies_ready),
            stage: task.stage,
            result: task.result,
            created_at: task.created_at,
            updated_at: task.updated_at,
            completed_at: task.completed_at,
            dependencies: dependencies.into_iter().map(dependency_view).collect(),
            role_specs: workflow.roles().to_vec(),
            actors,
            allowed_operations,
            comments,
            operations,
        })
    }
}

fn visible_actors(rows: Vec<TaskActorRow>, viewer: TaskViewer) -> Vec<TaskActor> {
    rows.into_iter()
        .filter_map(|row| {
            let own = Some(row.account_id) == viewer.account_id;
            if !viewer.can_see_all && !row.active && !own {
                return None;
            }
            let work = if viewer.can_see_all || own {
                row.work
            } else {
                None
            };
            Some(TaskActor {
                role_key: row.role_key,
                account_id: row.account_id,
                display_name: row.display_name,
                active: row.active,
                work,
            })
        })
        .collect()
}

fn dependency_view(row: TaskDependencyRow) -> TaskDependency {
    TaskDependency {
        task_id: row.task_id,
        title: row.title,
        state: row.state,
        result: row.result,
    }
}

fn comment_view(row: TaskCommentRow) -> TaskComment {
    TaskComment {
        id: row.id,
        author_account_id: row.author_account_id,
        author_display_name: row.author_display_name,
        author_kind: row.author_kind,
        body: row.body,
        created_at: row.created_at,
    }
}

fn redact_operation(mut operation: TaskOperation, viewer: TaskViewer) -> TaskOperation {
    if viewer.can_see_all {
        return operation;
    }
    let owner = operation
        .payload
        .get("account_id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok());
    if owner != viewer.account_id
        && let Some(payload) = operation.payload.as_object_mut()
    {
        payload.remove("work");
    }
    operation
}

fn dispatch_error(error: TaskDispatchError) -> ServiceError {
    match error {
        TaskDispatchError::NotFound(id) => ServiceError::not_found("task", id),
        TaskDispatchError::Rejected(message) => ServiceError::Validation(message),
        TaskDispatchError::Forbidden(message) => ServiceError::Forbidden(message),
        TaskDispatchError::Conflict(message) => ServiceError::InvalidTransition(message),
        TaskDispatchError::Backend(message) => ServiceError::Internal(message),
    }
}

impl From<RepoError> for TaskDispatchError {
    fn from(error: RepoError) -> Self {
        match error {
            RepoError::NotFound => Self::NotFound(Uuid::nil()),
            RepoError::Conflict { expected, actual } => Self::Conflict(format!(
                "version conflict: expected {expected}, current is {actual}"
            )),
            RepoError::Duplicate(message) => Self::Conflict(message),
            RepoError::Invalid(message) => Self::Rejected(message),
            RepoError::Backend(message) => Self::Backend(message),
        }
    }
}

/// The Task operations exposed to the interface adapters.
#[async_trait::async_trait]
pub trait TaskUseCases: Send + Sync {
    /// Resolve the project that owns a task; the task itself never moves.
    async fn project_id(&self, id: Uuid) -> Result<Uuid, ServiceError>;

    async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError>;

    async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
        viewer: TaskViewer,
    ) -> Result<Vec<Task>, ServiceError>;

    async fn send_operation(
        &self,
        project_id: Uuid,
        id: Uuid,
        actor_id: Uuid,
        operation: OperationName,
        idempotency_key: String,
    ) -> Result<(), ServiceError>;

    async fn add_comment(
        &self,
        project_id: Uuid,
        id: Uuid,
        author_account_id: Uuid,
        body: String,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError>;

    async fn comments(
        &self,
        project_id: Uuid,
        id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskComment>, ServiceError>;

    async fn history(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, ServiceError>;
}

#[async_trait::async_trait]
impl TaskUseCases for TaskService {
    async fn project_id(&self, id: Uuid) -> Result<Uuid, ServiceError> {
        self.project_of(id).await
    }

    async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError> {
        self.get(project_id, id, viewer).await
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
        viewer: TaskViewer,
    ) -> Result<Vec<Task>, ServiceError> {
        self.list(project_id, filter, viewer).await
    }

    async fn send_operation(
        &self,
        project_id: Uuid,
        id: Uuid,
        actor_id: Uuid,
        operation: OperationName,
        idempotency_key: String,
    ) -> Result<(), ServiceError> {
        self.send_operation(project_id, id, actor_id, operation, idempotency_key)
            .await
    }

    async fn add_comment(
        &self,
        project_id: Uuid,
        id: Uuid,
        author_account_id: Uuid,
        body: String,
        viewer: TaskViewer,
    ) -> Result<Task, ServiceError> {
        self.add_comment(project_id, id, author_account_id, body, viewer)
            .await
    }

    async fn comments(
        &self,
        project_id: Uuid,
        id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskComment>, ServiceError> {
        self.comments(project_id, id, limit, offset).await
    }

    async fn history(
        &self,
        project_id: Uuid,
        id: Uuid,
        viewer: TaskViewer,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, ServiceError> {
        self.history(project_id, id, viewer, limit, offset).await
    }
}
