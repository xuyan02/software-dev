use crate::commands::{CommentIssue, CreateIssue, TransitionIssue, TriageIssue};
use crate::error::ServiceError;
use crate::ports::{
    Action, ActionContext, EventSink, IssueFilter, IssueRepository, PolicyEngine, Resource,
};
use foundry_domain::{Actor, DomainEvent, DomainEventKind, Issue, TransitionEvidence};
use std::sync::Arc;
use uuid::Uuid;

/// Issue use cases.
///
/// Issues live in problem space. Creating, discussing and classifying them is
/// cheap; deciding not to act, closing, or confirming a fix is a human
/// disposition unless policy says otherwise.
pub struct IssueService<R, E, P> {
    repository: Arc<R>,
    events: Arc<E>,
    policy: Arc<P>,
}

impl<R, E, P> IssueService<R, E, P>
where
    R: IssueRepository,
    E: EventSink,
    P: PolicyEngine,
{
    pub fn new(repository: Arc<R>, events: Arc<E>, policy: Arc<P>) -> Self {
        Self {
            repository,
            events,
            policy,
        }
    }

    pub async fn create(&self, actor: Actor, command: CreateIssue) -> Result<Issue, ServiceError> {
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::CreateIssue,
            resource: Resource::Issues,
        })?;

        let mut issue = Issue::new(
            command.project_id,
            command.title,
            command.description,
            command.kind,
            command.source,
            actor.clone(),
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
        issue.severity = command.severity;
        issue.area = command.area;
        issue.labels = command.labels;
        issue.environment = command.environment;
        issue.affected_versions = command.affected_versions;
        issue.reproduction = command.reproduction;
        issue.expected = command.expected;
        issue.actual = command.actual;
        issue
            .validate()
            .map_err(|error| ServiceError::Validation(error.to_string()))?;

        self.repository.insert(&issue).await?;
        self.emit(DomainEventKind::IssueCreated, &issue, actor)
            .await?;
        Ok(issue)
    }

    /// Reads are project-scoped. An issue from another project is reported as
    /// not found so project isolation does not leak existence.
    pub async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Issue, ServiceError> {
        let issue = self
            .repository
            .get(id)
            .await?
            .ok_or_else(|| ServiceError::not_found("issue", id))?;
        if issue.project_id != project_id {
            return Err(ServiceError::not_found("issue", id));
        }
        Ok(issue)
    }

    pub async fn list(
        &self,
        project_id: Uuid,
        filter: IssueFilter,
    ) -> Result<Vec<Issue>, ServiceError> {
        Ok(self.repository.list(project_id, filter).await?)
    }

    pub async fn comment(&self, command: CommentIssue) -> Result<Issue, ServiceError> {
        let mut issue = self.get(command.project_id, command.id).await?;
        check_version(&issue, command.expected_version)?;

        let actor = command.actor;
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::CommentIssue,
            resource: Resource::Issue(command.id),
        })?;

        let previous_version = issue.version;
        issue
            .comment(actor.clone(), command.body, command.internal)
            .map_err(|error| ServiceError::Validation(error.to_string()))?;

        self.repository.save(&issue, previous_version).await?;
        self.emit(DomainEventKind::IssueCommented, &issue, actor)
            .await?;
        Ok(issue)
    }

    pub async fn triage(&self, command: TriageIssue) -> Result<Issue, ServiceError> {
        let mut issue = self.get(command.project_id, command.id).await?;
        check_version(&issue, command.expected_version)?;

        let actor = command.actor;
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::TriageIssue,
            resource: Resource::Issue(command.id),
        })?;

        let previous_version = issue.version;
        issue
            .triage(&actor, command.triage)
            .map_err(|error| ServiceError::InvalidTransition(error.to_string()))?;

        self.repository.save(&issue, previous_version).await?;
        self.emit(DomainEventKind::IssueTriaged, &issue, actor)
            .await?;
        Ok(issue)
    }

    pub async fn transition(&self, command: TransitionIssue) -> Result<Issue, ServiceError> {
        let mut issue = self.get(command.project_id, command.id).await?;
        let from = issue.status;
        check_version(&issue, command.expected_version)?;

        let actor = command.actor;
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::TransitionIssue {
                from,
                to: command.to,
            },
            resource: Resource::Issue(command.id),
        })?;

        let previous_version = issue.version;
        issue
            .transition_to(
                command.to,
                &actor,
                TransitionEvidence {
                    resolution: command.resolution,
                    duplicate_of: command.duplicate_of,
                },
            )
            .map_err(|error| ServiceError::InvalidTransition(error.to_string()))?;

        self.repository.save(&issue, previous_version).await?;
        self.emit(DomainEventKind::IssueTransitioned, &issue, actor)
            .await?;
        Ok(issue)
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        issue: &Issue,
        actor: Actor,
    ) -> Result<(), ServiceError> {
        let payload = serde_json::to_value(issue)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let event = DomainEvent::new(kind, issue.id, actor, payload);
        self.events.publish(&[event]).await?;
        Ok(())
    }
}

fn check_version(issue: &Issue, expected: Option<u32>) -> Result<(), ServiceError> {
    if let Some(expected) = expected
        && expected != issue.version
    {
        return Err(ServiceError::Conflict {
            expected,
            actual: issue.version,
        });
    }
    Ok(())
}

/// Object-safe facade used by HTTP, MCP and the CLI.
#[async_trait::async_trait]
pub trait IssueUseCases: Send + Sync {
    async fn create(&self, actor: Actor, command: CreateIssue) -> Result<Issue, ServiceError>;
    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Issue, ServiceError>;
    async fn list(&self, project_id: Uuid, filter: IssueFilter)
    -> Result<Vec<Issue>, ServiceError>;
    async fn comment(&self, command: CommentIssue) -> Result<Issue, ServiceError>;
    async fn triage(&self, command: TriageIssue) -> Result<Issue, ServiceError>;
    async fn transition(&self, command: TransitionIssue) -> Result<Issue, ServiceError>;
}

#[async_trait::async_trait]
impl<R, E, P> IssueUseCases for IssueService<R, E, P>
where
    R: IssueRepository,
    E: EventSink,
    P: PolicyEngine,
{
    async fn create(&self, actor: Actor, command: CreateIssue) -> Result<Issue, ServiceError> {
        IssueService::create(self, actor, command).await
    }

    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Issue, ServiceError> {
        IssueService::get(self, project_id, id).await
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: IssueFilter,
    ) -> Result<Vec<Issue>, ServiceError> {
        IssueService::list(self, project_id, filter).await
    }

    async fn comment(&self, command: CommentIssue) -> Result<Issue, ServiceError> {
        IssueService::comment(self, command).await
    }

    async fn triage(&self, command: TriageIssue) -> Result<Issue, ServiceError> {
        IssueService::triage(self, command).await
    }

    async fn transition(&self, command: TransitionIssue) -> Result<Issue, ServiceError> {
        IssueService::transition(self, command).await
    }
}
