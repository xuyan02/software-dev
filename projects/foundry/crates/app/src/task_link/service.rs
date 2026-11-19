//! Task link use cases.

use async_trait::async_trait;
use foundry_domain::{
    Account, AccountStatus, Actor, DomainEvent, DomainEventKind, ProjectRole, TaskLink,
    TaskLinkKind, TaskLinkRelation,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    AccountRepository, EventSink, ProjectRepository, RepoError, ServiceError, TaskLinkRepository,
    TaskRepository,
};

#[derive(Debug, Clone)]
pub struct AddTaskLink {
    pub task_id: Uuid,
    pub kind: TaskLinkKind,
    pub target_id: Option<Uuid>,
    pub url: Option<String>,
    pub title: String,
    pub relation: TaskLinkRelation,
    /// Make this link the Issue's current handling task, deactivating the
    /// previous handler while keeping it as history.
    pub active: bool,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct RemoveTaskLink {
    pub task_id: Uuid,
    pub link_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkedTaskView {
    pub id: Uuid,
    pub title: String,
    pub workflow: String,
    pub stage: Option<String>,
    pub state: String,
    pub result: Option<String>,
    pub active: bool,
}

pub struct TaskLinkService<R, A, T, P, E> {
    links: Arc<R>,
    accounts: Arc<A>,
    tasks: Arc<T>,
    projects: Arc<P>,
    events: Arc<E>,
}

impl<R, A, T, P, E> TaskLinkService<R, A, T, P, E>
where
    R: TaskLinkRepository,
    A: AccountRepository,
    T: TaskRepository,
    P: ProjectRepository,
    E: EventSink,
{
    pub fn new(
        links: Arc<R>,
        accounts: Arc<A>,
        tasks: Arc<T>,
        projects: Arc<P>,
        events: Arc<E>,
    ) -> Self {
        Self {
            links,
            accounts,
            tasks,
            projects,
            events,
        }
    }

    pub async fn list(&self, actor: Actor, task_id: Uuid) -> Result<Vec<TaskLink>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let task = self.task(&account, task_id, ProjectRole::Viewer).await?;
        Ok(self.links.list(task.id).await?)
    }

    pub async fn add(&self, actor: Actor, command: AddTaskLink) -> Result<TaskLink, ServiceError> {
        let account = self.require_account(&actor).await?;
        let task = self
            .task(&account, command.task_id, ProjectRole::Member)
            .await?;
        let link = TaskLink {
            id: Uuid::now_v7(),
            task_id: task.id,
            kind: command.kind,
            target_id: command.target_id,
            url: command.url,
            title: command.title,
            relation: command.relation,
            active: command.active,
            created_by: account.actor(),
            created_at: time::OffsetDateTime::now_utc(),
            metadata: command.metadata,
        };
        let validation = foundry_domain::validate_task_link(&link);
        if !validation.is_valid() {
            return Err(ServiceError::Validation(
                validation
                    .errors
                    .iter()
                    .map(|issue| issue.message.clone())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        if link.active
            && link.kind == TaskLinkKind::Issue
            && let Some(target_id) = link.target_id
        {
            self.links.deactivate_issue_links(target_id).await?;
        }
        self.links
            .insert(&link)
            .await
            .map_err(task_link_repo_error)?;
        self.emit(
            DomainEventKind::TaskLinkAdded,
            task.id,
            &account,
            json!({
                "task_id": task.id,
                "link_id": link.id,
                "kind": link.kind,
                "relation": link.relation,
            }),
        )
        .await?;
        Ok(link)
    }

    pub async fn linked_issue_tasks(
        &self,
        actor: Actor,
        project_id: Uuid,
        issue_id: Uuid,
    ) -> Result<Vec<LinkedTaskView>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let project = self
            .projects
            .get(project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", project_id))?;
        if !account.is_admin() && project.member(account.id).is_none() {
            return Err(ServiceError::Forbidden(
                "you are not a member of this project".to_string(),
            ));
        }
        let links = self
            .links
            .list_by_target(TaskLinkKind::Issue, issue_id)
            .await?;
        let mut tasks = Vec::new();
        for link in links {
            let Some(task) = self.tasks.get(link.task_id).await? else {
                continue;
            };
            if task.project_id != project_id {
                continue;
            }
            tasks.push(LinkedTaskView {
                id: task.id,
                title: task.title,
                workflow: task.workflow.as_str().to_string(),
                stage: task.stage.map(|stage| stage.as_str().to_string()),
                state: task.state.as_str().to_string(),
                result: task.result.map(|result| result.as_str().to_string()),
                active: link.active,
            });
        }
        // The current handler is listed first; older links follow as history.
        tasks.sort_by_key(|task| std::cmp::Reverse(task.active));
        Ok(tasks)
    }

    /// Called when a task starts: its Issue links become the current handlers,
    /// and the previous handlers stay in the link list as history.
    pub async fn handoff_task_issues(&self, task_id: Uuid) -> Result<(), ServiceError> {
        self.links
            .activate_task_issue_links(task_id)
            .await
            .map_err(ServiceError::from)
    }

    pub async fn remove(&self, actor: Actor, command: RemoveTaskLink) -> Result<(), ServiceError> {
        let account = self.require_account(&actor).await?;
        let task = self
            .task(&account, command.task_id, ProjectRole::Member)
            .await?;
        self.links
            .delete(task.id, command.link_id)
            .await
            .map_err(task_link_repo_error)?;
        self.emit(
            DomainEventKind::TaskLinkRemoved,
            task.id,
            &account,
            json!({ "task_id": task.id, "link_id": command.link_id }),
        )
        .await?;
        Ok(())
    }

    async fn require_account(&self, actor: &Actor) -> Result<Account, ServiceError> {
        let account = self
            .accounts
            .get_by_handle(actor.id())
            .await?
            .ok_or_else(|| ServiceError::Unauthorized("unknown account".to_string()))?;
        if account.status != AccountStatus::Active {
            return Err(ServiceError::Unauthorized(
                "account is suspended".to_string(),
            ));
        }
        Ok(account)
    }

    async fn task(
        &self,
        account: &Account,
        task_id: Uuid,
        minimum: ProjectRole,
    ) -> Result<foundry_domain::TaskDAO, ServiceError> {
        let task = self
            .tasks
            .get(task_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("task", task_id))?;
        let project = self
            .projects
            .get(task.project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", task.project_id))?;
        if project.status == foundry_domain::ProjectStatus::Archived {
            return Err(ServiceError::Forbidden("project is archived".to_string()));
        }
        let role = if account.is_admin() {
            ProjectRole::Owner
        } else {
            project.role_of(account.id).ok_or_else(|| {
                ServiceError::Forbidden("you are not a member of this project".to_string())
            })?
        };
        if role < minimum {
            return Err(ServiceError::Forbidden(
                "project role does not allow this action".to_string(),
            ));
        }
        Ok(task)
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        aggregate_id: Uuid,
        account: &Account,
        payload: Value,
    ) -> Result<(), ServiceError> {
        let event = DomainEvent::new(kind, aggregate_id, account.actor(), payload);
        self.events.publish(&[event]).await?;
        Ok(())
    }
}

fn task_link_repo_error(error: RepoError) -> ServiceError {
    match error {
        RepoError::Invalid(message) => ServiceError::Validation(message),
        other => other.into(),
    }
}

#[async_trait]
pub trait TaskLinkUseCases: Send + Sync {
    async fn list(&self, actor: Actor, task_id: Uuid) -> Result<Vec<TaskLink>, ServiceError>;
    async fn add(&self, actor: Actor, command: AddTaskLink) -> Result<TaskLink, ServiceError>;
    async fn linked_issue_tasks(
        &self,
        actor: Actor,
        project_id: Uuid,
        issue_id: Uuid,
    ) -> Result<Vec<LinkedTaskView>, ServiceError>;

    async fn handoff_task_issues(&self, task_id: Uuid) -> Result<(), ServiceError>;

    async fn remove(&self, actor: Actor, command: RemoveTaskLink) -> Result<(), ServiceError>;
}

#[async_trait]
impl<R, A, T, P, E> TaskLinkUseCases for TaskLinkService<R, A, T, P, E>
where
    R: TaskLinkRepository,
    A: AccountRepository,
    T: TaskRepository,
    P: ProjectRepository,
    E: EventSink,
{
    async fn list(&self, actor: Actor, task_id: Uuid) -> Result<Vec<TaskLink>, ServiceError> {
        TaskLinkService::list(self, actor, task_id).await
    }

    async fn add(&self, actor: Actor, command: AddTaskLink) -> Result<TaskLink, ServiceError> {
        TaskLinkService::add(self, actor, command).await
    }

    async fn linked_issue_tasks(
        &self,
        actor: Actor,
        project_id: Uuid,
        issue_id: Uuid,
    ) -> Result<Vec<LinkedTaskView>, ServiceError> {
        TaskLinkService::linked_issue_tasks(self, actor, project_id, issue_id).await
    }

    async fn remove(&self, actor: Actor, command: RemoveTaskLink) -> Result<(), ServiceError> {
        TaskLinkService::remove(self, actor, command).await
    }

    async fn handoff_task_issues(&self, task_id: Uuid) -> Result<(), ServiceError> {
        TaskLinkService::handoff_task_issues(self, task_id).await
    }
}
