use crate::commands::{CreateRequirement, TransitionRequirement, UpdateRequirement};
use crate::error::ServiceError;
use crate::ports::{
    Action, ActionContext, EventSink, PolicyEngine, RequirementFilter, RequirementRepository,
    Resource,
};
use foundry_domain::{Actor, DomainEvent, DomainEventKind, Requirement, RequirementStatus};
use std::sync::Arc;
use uuid::Uuid;

/// Requirement use cases.
///
/// Every write follows the same order: read, authorize, mutate the aggregate,
/// persist with optimistic concurrency, append events. Keeping this order in
/// one place is what makes the audit trail trustworthy.
pub struct RequirementService<R, E, P> {
    repository: Arc<R>,
    events: Arc<E>,
    policy: Arc<P>,
}

impl<R, E, P> RequirementService<R, E, P>
where
    R: RequirementRepository,
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

    pub async fn create(
        &self,
        actor: Actor,
        command: CreateRequirement,
    ) -> Result<Requirement, ServiceError> {
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::CreateRequirement,
            resource: Resource::Requirements,
        })?;

        let mut requirement = Requirement::new(
            command.project_id,
            command.title,
            command.problem,
            command.acceptance,
            actor.clone(),
            command.priority,
            command.risk,
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
        requirement.users = command.users;
        requirement.non_goals = command.non_goals;
        requirement.constraints = command.constraints;

        requirement
            .validate()
            .map_err(|error| ServiceError::Validation(error.to_string()))?;

        self.repository.insert(&requirement).await?;
        self.emit(DomainEventKind::RequirementCreated, &requirement, actor)
            .await?;
        Ok(requirement)
    }

    /// Reads are project-scoped. An item from another project is reported as
    /// not found so project isolation does not leak existence.
    pub async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Requirement, ServiceError> {
        let requirement = self
            .repository
            .get(id)
            .await?
            .ok_or_else(|| ServiceError::not_found("requirement", id))?;
        if requirement.project_id != project_id {
            return Err(ServiceError::not_found("requirement", id));
        }
        Ok(requirement)
    }

    pub async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, ServiceError> {
        Ok(self.repository.list(project_id, filter).await?)
    }

    pub async fn transition(
        &self,
        command: TransitionRequirement,
    ) -> Result<Requirement, ServiceError> {
        let mut requirement = self.get(command.project_id, command.id).await?;
        let from = requirement.status;

        if let Some(expected) = command.expected_version
            && expected != requirement.version
        {
            return Err(ServiceError::Conflict {
                expected,
                actual: requirement.version,
            });
        }

        let actor = command.actor;
        self.policy.check(&ActionContext {
            actor: actor.clone(),
            action: Action::TransitionRequirement {
                from,
                to: command.to,
            },
            resource: Resource::Requirement(command.id),
        })?;

        let previous_version = requirement.version;
        requirement
            .transition_to(command.to, &actor)
            .map_err(|error| ServiceError::InvalidTransition(error.to_string()))?;

        self.repository.save(&requirement, previous_version).await?;
        self.emit(
            DomainEventKind::RequirementTransitioned,
            &requirement,
            actor,
        )
        .await?;
        Ok(requirement)
    }

    pub async fn update(&self, command: UpdateRequirement) -> Result<Requirement, ServiceError> {
        let mut requirement = self.get(command.project_id, command.id).await?;
        let previous_version = requirement.version;

        if let Some(expected) = command.expected_version
            && expected != previous_version
        {
            return Err(ServiceError::Conflict {
                expected,
                actual: previous_version,
            });
        }

        // Editing an approved or later requirement is a scope change, not a
        // silent edit. Only draft/in-review items may be edited in place.
        if !matches!(
            requirement.status,
            RequirementStatus::Draft | RequirementStatus::InReview
        ) {
            return Err(ServiceError::Forbidden(format!(
                "requirement in status {:?} cannot be edited in place; open a change request",
                requirement.status
            )));
        }

        if let Some(title) = command.title {
            requirement.title = title;
        }
        if let Some(problem) = command.problem {
            requirement.problem = problem;
        }
        if let Some(users) = command.users {
            requirement.users = users;
        }
        if let Some(acceptance) = command.acceptance {
            requirement.acceptance = acceptance;
        }
        if let Some(non_goals) = command.non_goals {
            requirement.non_goals = non_goals;
        }
        if let Some(constraints) = command.constraints {
            requirement.constraints = constraints;
        }
        if let Some(priority) = command.priority {
            requirement.priority = priority;
        }
        if let Some(risk) = command.risk {
            requirement.risk = risk;
        }

        requirement
            .validate()
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        requirement.version = requirement.version.saturating_add(1);
        requirement.updated_at = time::OffsetDateTime::now_utc();

        self.repository.save(&requirement, previous_version).await?;
        self.emit(
            DomainEventKind::RequirementUpdated,
            &requirement,
            command.actor,
        )
        .await?;
        Ok(requirement)
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        requirement: &Requirement,
        actor: Actor,
    ) -> Result<(), ServiceError> {
        let payload = serde_json::to_value(requirement)
            .map_err(|error| ServiceError::Internal(error.to_string()))?;
        let event = DomainEvent::new(kind, requirement.id, actor, payload);
        self.events.publish(&[event]).await?;
        Ok(())
    }
}

/// Object-safe facade used by the delivery layer (HTTP, MCP, CLI, workers).
///
/// Keeping this narrow means an adapter can swap the storage or policy engine
/// without changing any handler signature.
#[async_trait::async_trait]
pub trait RequirementUseCases: Send + Sync {
    async fn create(
        &self,
        actor: Actor,
        command: CreateRequirement,
    ) -> Result<Requirement, ServiceError>;
    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Requirement, ServiceError>;
    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, ServiceError>;
    async fn transition(&self, command: TransitionRequirement)
    -> Result<Requirement, ServiceError>;
    async fn update(&self, command: UpdateRequirement) -> Result<Requirement, ServiceError>;
}

#[async_trait::async_trait]
impl<R, E, P> RequirementUseCases for RequirementService<R, E, P>
where
    R: RequirementRepository,
    E: EventSink,
    P: PolicyEngine,
{
    async fn create(
        &self,
        actor: Actor,
        command: CreateRequirement,
    ) -> Result<Requirement, ServiceError> {
        RequirementService::create(self, actor, command).await
    }

    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Requirement, ServiceError> {
        RequirementService::get(self, project_id, id).await
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, ServiceError> {
        RequirementService::list(self, project_id, filter).await
    }

    async fn transition(
        &self,
        command: TransitionRequirement,
    ) -> Result<Requirement, ServiceError> {
        RequirementService::transition(self, command).await
    }

    async fn update(&self, command: UpdateRequirement) -> Result<Requirement, ServiceError> {
        RequirementService::update(self, command).await
    }
}
