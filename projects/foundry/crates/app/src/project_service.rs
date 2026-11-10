use crate::commands::{
    AddProjectMember, AddProjectMemberByHandle, CreateProject, RemoveProjectMember,
    SetProjectMemberRole, UpdateProject,
};
use crate::error::ServiceError;
use crate::ports::{AccountRepository, EventSink, ProjectFilter, ProjectRepository};
use crate::task::TaskDispatcher;
use crate::workflow::{WorkflowCatalog, WorkflowLookup};
use foundry_domain::{
    Account, AccountStatus, Actor, DomainEvent, DomainEventKind, Project, ProjectRole,
    ProjectStatus, TaskDAO, WorkflowOption,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct ProjectMemberView {
    pub account_id: Uuid,
    pub handle: String,
    pub display_name: String,
    pub kind: foundry_domain::AccountKind,
    pub status: AccountStatus,
    pub role: ProjectRole,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectDetail {
    pub project: Project,
    pub members: Vec<ProjectMemberView>,
}

pub struct ProjectService<P, A, E> {
    projects: Arc<P>,
    accounts: Arc<A>,
    events: Arc<E>,
    workflows: Arc<dyn WorkflowCatalog>,
    tasks: Arc<dyn TaskDispatcher>,
}

impl<P, A, E> ProjectService<P, A, E>
where
    P: ProjectRepository,
    A: AccountRepository,
    E: EventSink,
{
    pub fn new(
        projects: Arc<P>,
        accounts: Arc<A>,
        events: Arc<E>,
        workflows: Arc<dyn WorkflowCatalog>,
        tasks: Arc<dyn TaskDispatcher>,
    ) -> Self {
        Self {
            projects,
            accounts,
            events,
            workflows,
            tasks,
        }
    }

    pub async fn create(
        &self,
        actor: Actor,
        command: CreateProject,
    ) -> Result<Project, ServiceError> {
        let account = self.require_account(&actor).await?;
        if command.root_workflow_id.as_str().trim().is_empty() {
            return Err(ServiceError::Validation(
                "root_workflow_id is required".to_string(),
            ));
        }
        let reference = match self
            .workflows
            .latest_visible(Some(account.id), &command.root_workflow_id)
            .await?
        {
            WorkflowLookup::Published(reference) => reference,
            WorkflowLookup::Unpublished(id) => {
                return Err(ServiceError::Unpublished(id.to_string()));
            }
            WorkflowLookup::NotFound => {
                return Err(ServiceError::not_found_named(
                    "workflow",
                    command.root_workflow_id.as_str(),
                ));
            }
        };

        let mut project = Project::new(command.name, command.description, account.id)
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        let now = time::OffsetDateTime::now_utc();
        let mut root = TaskDAO::new(
            project.id,
            None,
            reference.workflow_id,
            reference.version,
            project.name.clone(),
            now,
        );
        project.root_task_id = Some(root.id);
        root.state = foundry_domain::TaskState::Pending;

        let payload = serde_json::to_value(&project)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let events = [
            DomainEvent::new(
                DomainEventKind::ProjectCreated,
                project.id,
                account.actor(),
                payload,
            ),
            DomainEvent::new(
                DomainEventKind::TaskCreated,
                root.id,
                Actor::System,
                json!({
                    "workflow": root.workflow,
                    "workflow_version": root.workflow_version,
                    "title": root.title,
                    "parent_id": null,
                    "dependencies": [],
                }),
            ),
        ];
        self.projects
            .insert_with_root(&project, &root, &events)
            .await?;

        // The project and root Task are committed. Initialization is best
        // effort: the event is durable, so the dispatcher retries it without
        // turning a committed creation into an error.
        if let Err(error) = self.tasks.process(root.id, None).await {
            tracing::warn!(
                task_id = %root.id,
                error = %error,
                "project created but root task initialization failed"
            );
        }
        Ok(project)
    }

    pub async fn root_workflow_candidates(
        &self,
        actor: Actor,
    ) -> Result<Vec<WorkflowOption>, ServiceError> {
        let account = self.require_account(&actor).await?;
        Ok(self.workflows.root_candidates(account.id).await?)
    }

    pub async fn update(
        &self,
        actor: Actor,
        command: UpdateProject,
    ) -> Result<Project, ServiceError> {
        let (account, mut project) = self.require_owner(&actor, command.id).await?;
        let previous_version = project.version;

        // Reactivation happens before edits so an archived project can be
        // restored and renamed in one request. Archiving happens after edits so
        // the edits themselves are still evaluated against an active project.
        if command.status == Some(ProjectStatus::Active) {
            project
                .activate()
                .map_err(|error| ServiceError::Validation(error.to_string()))?;
        }
        if command.name.is_some() || command.description.is_some() {
            project
                .update(command.name, command.description)
                .map_err(|error| ServiceError::Validation(error.to_string()))?;
        }
        if command.status == Some(ProjectStatus::Archived) {
            project
                .archive()
                .map_err(|error| ServiceError::Validation(error.to_string()))?;
        }

        self.projects.save(&project, previous_version).await?;
        self.emit(DomainEventKind::ProjectUpdated, &project, account.actor())
            .await?;
        Ok(project)
    }

    pub async fn get(&self, actor: Actor, id: Uuid) -> Result<Project, ServiceError> {
        let account = self.require_account(&actor).await?;
        let project = self
            .projects
            .get(id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", id))?;
        if !account.is_admin() && project.member(account.id).is_none() {
            return Err(ServiceError::Forbidden(
                "you are not a member of this project".to_string(),
            ));
        }
        Ok(project)
    }

    pub async fn delete(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError> {
        let (account, project) = self.require_owner(&actor, id).await?;
        self.projects.delete(project.id, project.version).await?;
        tracing::info!(
            project_id = %project.id,
            actor = %account.handle,
            "project deleted"
        );
        Ok(())
    }

    pub async fn list_visible(&self, actor: Actor) -> Result<Vec<Project>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let mut projects = self.projects.list(ProjectFilter::default()).await?;
        if !account.is_admin() {
            projects.retain(|project| project.member(account.id).is_some());
        }
        projects.sort_by_key(|project| project.created_at);
        Ok(projects)
    }

    pub async fn detail(&self, actor: Actor, id: Uuid) -> Result<ProjectDetail, ServiceError> {
        let project = self.get(actor, id).await?;
        let mut members = Vec::new();
        for member in &project.members {
            let Some(account) = self.accounts.get(member.account_id).await? else {
                continue;
            };
            members.push(member_view(&account, member.role));
        }
        members.sort_by(|a, b| b.role.cmp(&a.role).then_with(|| a.handle.cmp(&b.handle)));
        Ok(ProjectDetail { project, members })
    }

    pub async fn add_member(
        &self,
        actor: Actor,
        command: AddProjectMember,
    ) -> Result<Project, ServiceError> {
        self.add_member_by_id(actor, command.project_id, command.account_id, command.role)
            .await
    }

    pub async fn add_member_by_handle(
        &self,
        actor: Actor,
        command: AddProjectMemberByHandle,
    ) -> Result<Project, ServiceError> {
        let (account, _) = self.require_owner(&actor, command.project_id).await?;
        let target = self
            .accounts
            .get_by_handle(&command.handle)
            .await?
            .ok_or_else(|| {
                ServiceError::Validation(format!("account {} does not exist", command.handle))
            })?;
        self.add_member_by_id(actor, command.project_id, target.id, command.role)
            .await
            .map_err(|error| match error {
                ServiceError::Unauthorized(_) => {
                    ServiceError::Unauthorized(format!("account {} is not active", account.handle))
                }
                other => other,
            })
    }

    async fn add_member_by_id(
        &self,
        actor: Actor,
        project_id: Uuid,
        account_id: Uuid,
        role: ProjectRole,
    ) -> Result<Project, ServiceError> {
        let (account, mut project) = self.require_owner(&actor, project_id).await?;
        let target = self
            .accounts
            .get(account_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("account", account_id))?;
        if target.status != AccountStatus::Active {
            return Err(ServiceError::Validation(
                "cannot add a suspended account".to_string(),
            ));
        }
        let previous_version = project.version;
        project
            .add_member(account_id, role)
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        self.projects.save(&project, previous_version).await?;
        self.emit(
            DomainEventKind::ProjectMemberChanged,
            &project,
            account.actor(),
        )
        .await?;
        Ok(project)
    }

    pub async fn set_member_role(
        &self,
        actor: Actor,
        command: SetProjectMemberRole,
    ) -> Result<Project, ServiceError> {
        let (account, mut project) = self.require_owner(&actor, command.project_id).await?;
        let previous_version = project.version;
        project
            .set_member_role(command.account_id, command.role)
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        self.projects.save(&project, previous_version).await?;
        self.emit(
            DomainEventKind::ProjectMemberChanged,
            &project,
            account.actor(),
        )
        .await?;
        Ok(project)
    }

    pub async fn remove_member(
        &self,
        actor: Actor,
        command: RemoveProjectMember,
    ) -> Result<Project, ServiceError> {
        let (account, mut project) = self.require_owner(&actor, command.project_id).await?;
        let previous_version = project.version;
        project
            .remove_member(command.account_id)
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        self.projects.save(&project, previous_version).await?;
        self.emit(
            DomainEventKind::ProjectMemberChanged,
            &project,
            account.actor(),
        )
        .await?;
        Ok(project)
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

    async fn require_owner(
        &self,
        actor: &Actor,
        project_id: Uuid,
    ) -> Result<(Account, Project), ServiceError> {
        let account = self.require_account(actor).await?;
        let project = self
            .projects
            .get(project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", project_id))?;
        if !account.is_admin() && project.role_of(account.id) != Some(ProjectRole::Owner) {
            return Err(ServiceError::Forbidden(
                "project owner role required".to_string(),
            ));
        }
        Ok((account, project))
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        project: &Project,
        actor: Actor,
    ) -> Result<(), ServiceError> {
        let payload = serde_json::to_value(project)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        self.events
            .publish(&[DomainEvent::new(kind, project.id, actor, payload)])
            .await?;
        Ok(())
    }
}

fn member_view(account: &Account, role: ProjectRole) -> ProjectMemberView {
    ProjectMemberView {
        account_id: account.id,
        handle: account.handle.clone(),
        display_name: account.display_name.clone(),
        kind: account.kind,
        status: account.status,
        role,
    }
}

#[async_trait::async_trait]
pub trait ProjectUseCases: Send + Sync {
    async fn create(&self, actor: Actor, command: CreateProject) -> Result<Project, ServiceError>;
    async fn update(&self, actor: Actor, command: UpdateProject) -> Result<Project, ServiceError>;
    async fn delete(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError>;
    async fn get(&self, actor: Actor, id: Uuid) -> Result<Project, ServiceError>;
    async fn list_visible(&self, actor: Actor) -> Result<Vec<Project>, ServiceError>;
    async fn root_workflow_candidates(
        &self,
        actor: Actor,
    ) -> Result<Vec<WorkflowOption>, ServiceError>;
    async fn detail(&self, actor: Actor, id: Uuid) -> Result<ProjectDetail, ServiceError>;
    async fn add_member(
        &self,
        actor: Actor,
        command: AddProjectMember,
    ) -> Result<Project, ServiceError>;
    async fn add_member_by_handle(
        &self,
        actor: Actor,
        command: AddProjectMemberByHandle,
    ) -> Result<Project, ServiceError>;
    async fn set_member_role(
        &self,
        actor: Actor,
        command: SetProjectMemberRole,
    ) -> Result<Project, ServiceError>;
    async fn remove_member(
        &self,
        actor: Actor,
        command: RemoveProjectMember,
    ) -> Result<Project, ServiceError>;
}

#[async_trait::async_trait]
impl<P, A, E> ProjectUseCases for ProjectService<P, A, E>
where
    P: ProjectRepository,
    A: AccountRepository,
    E: EventSink,
{
    async fn create(&self, actor: Actor, command: CreateProject) -> Result<Project, ServiceError> {
        ProjectService::create(self, actor, command).await
    }

    async fn update(&self, actor: Actor, command: UpdateProject) -> Result<Project, ServiceError> {
        ProjectService::update(self, actor, command).await
    }

    async fn delete(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError> {
        ProjectService::delete(self, actor, id).await
    }

    async fn get(&self, actor: Actor, id: Uuid) -> Result<Project, ServiceError> {
        ProjectService::get(self, actor, id).await
    }

    async fn list_visible(&self, actor: Actor) -> Result<Vec<Project>, ServiceError> {
        ProjectService::list_visible(self, actor).await
    }

    async fn root_workflow_candidates(
        &self,
        actor: Actor,
    ) -> Result<Vec<WorkflowOption>, ServiceError> {
        ProjectService::root_workflow_candidates(self, actor).await
    }

    async fn detail(&self, actor: Actor, id: Uuid) -> Result<ProjectDetail, ServiceError> {
        ProjectService::detail(self, actor, id).await
    }

    async fn add_member(
        &self,
        actor: Actor,
        command: AddProjectMember,
    ) -> Result<Project, ServiceError> {
        ProjectService::add_member(self, actor, command).await
    }

    async fn add_member_by_handle(
        &self,
        actor: Actor,
        command: AddProjectMemberByHandle,
    ) -> Result<Project, ServiceError> {
        ProjectService::add_member_by_handle(self, actor, command).await
    }

    async fn set_member_role(
        &self,
        actor: Actor,
        command: SetProjectMemberRole,
    ) -> Result<Project, ServiceError> {
        ProjectService::set_member_role(self, actor, command).await
    }

    async fn remove_member(
        &self,
        actor: Actor,
        command: RemoveProjectMember,
    ) -> Result<Project, ServiceError> {
        ProjectService::remove_member(self, actor, command).await
    }
}
