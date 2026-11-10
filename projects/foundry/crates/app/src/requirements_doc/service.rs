//! Requirements document use cases (RFC 0020).

use async_trait::async_trait;
use foundry_domain::{
    Account, AccountStatus, Actor, AnnotationStatus, AnnotationTarget, DomainEvent,
    DomainEventKind, ProjectRole, ProjectStatus, RequirementsAnnotation, RequirementsDocumentBody,
    RequirementsDraftDAO, RequirementsOperation, Validation, ValidationIssue,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AccountRepository, EventSink, ProjectRepository, RepoError, RequirementsApplyOutcome,
    RequirementsDocumentRepository, RequirementsPublishOutcome, ServiceError,
};

#[derive(Debug, Clone)]
pub struct ApplyRequirementsOperations {
    pub project_id: Uuid,
    pub expected_revision: u64,
    pub operations: Vec<RequirementsOperation>,
}

#[derive(Debug, Clone)]
pub struct PublishRequirementsDocument {
    pub project_id: Uuid,
    pub expected_revision: u64,
}

#[derive(Debug, Clone)]
pub struct AnnotateRequirements {
    pub project_id: Uuid,
    pub target_type: AnnotationTarget,
    pub target_id: Option<String>,
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ResolveRequirementsAnnotation {
    pub project_id: Uuid,
    pub annotation_id: Uuid,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementsDocumentView {
    pub project_id: Uuid,
    pub document_id: Option<Uuid>,
    pub current_version: u32,
    pub latest_version: Option<u32>,
    pub draft_revision: Option<u64>,
    pub editable: bool,
    pub body: RequirementsDocumentBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementsDraftView {
    pub project_id: Uuid,
    pub revision: u64,
    pub base_version: u32,
    pub body: RequirementsDocumentBody,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementsVersionView {
    pub project_id: Uuid,
    pub version: u32,
    pub body: RequirementsDocumentBody,
    pub created_by: Uuid,
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequirementsPublished {
    pub version: u32,
    pub revision: u64,
    pub warnings: Vec<ValidationIssue>,
}

pub struct RequirementsDocumentService<R, A, P, E> {
    repository: Arc<R>,
    accounts: Arc<A>,
    projects: Arc<P>,
    events: Arc<E>,
}

impl<R, A, P, E> RequirementsDocumentService<R, A, P, E>
where
    R: RequirementsDocumentRepository,
    A: AccountRepository,
    P: ProjectRepository,
    E: EventSink,
{
    pub fn new(repository: Arc<R>, accounts: Arc<A>, projects: Arc<P>, events: Arc<E>) -> Self {
        Self {
            repository,
            accounts,
            projects,
            events,
        }
    }

    pub async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDocumentView, ServiceError> {
        let account = self.require_account(&actor).await?;
        let role = self
            .project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let document = self.repository.document(project_id).await?;
        let draft = self.repository.draft(project_id).await?;
        let latest = self.repository.latest_version(project_id).await?;
        let body = if let Some(draft) = &draft {
            draft.body.clone()
        } else if let Some(latest) = &latest {
            latest.body.clone()
        } else {
            RequirementsDocumentBody::default()
        };
        Ok(RequirementsDocumentView {
            project_id,
            document_id: document.as_ref().map(|document| document.id),
            current_version: document
                .as_ref()
                .map(|document| document.current_version)
                .unwrap_or(0),
            latest_version: latest.as_ref().map(|version| version.version),
            draft_revision: draft.as_ref().map(|draft| draft.revision),
            editable: role >= ProjectRole::Member,
            body,
        })
    }

    pub async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        let (_, draft) = self
            .repository
            .ensure_document(project_id, account.id, OffsetDateTime::now_utc())
            .await?;
        Ok(draft_view(draft))
    }

    pub async fn apply(
        &self,
        actor: Actor,
        command: ApplyRequirementsOperations,
    ) -> Result<RequirementsDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let (_, _) = self
            .repository
            .ensure_document(command.project_id, account.id, OffsetDateTime::now_utc())
            .await?;
        let now = OffsetDateTime::now_utc();
        match self
            .repository
            .apply_operations(
                command.project_id,
                command.expected_revision,
                &command.operations,
                account.id,
                now,
            )
            .await
            .map_err(requirements_repo_error)?
        {
            RequirementsApplyOutcome::Applied { revision, .. } => {
                let draft = self
                    .repository
                    .draft(command.project_id)
                    .await?
                    .ok_or_else(|| ServiceError::DraftNotFound(command.project_id.to_string()))?;
                self.emit(
                    DomainEventKind::RequirementsDraftSaved,
                    command.project_id,
                    &account,
                    json!({
                        "project_id": command.project_id,
                        "revision": revision,
                        "operations": command.operations.len(),
                    }),
                )
                .await?;
                let mut view = draft_view(draft);
                view.revision = revision;
                Ok(view)
            }
            RequirementsApplyOutcome::Conflict {
                current_revision,
                current_updated_at,
            } => Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: current_revision,
                current_updated_at,
            }),
            RequirementsApplyOutcome::NoDraft => {
                Err(ServiceError::DraftNotFound(command.project_id.to_string()))
            }
            RequirementsApplyOutcome::TooLarge => Err(ServiceError::RequirementsTooLarge),
        }
    }

    pub async fn validate(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Validation, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let body = match self.repository.draft(project_id).await? {
            Some(draft) => draft.body,
            None => match self.repository.latest_version(project_id).await? {
                Some(version) => version.body,
                None => RequirementsDocumentBody::default(),
            },
        };
        Ok(foundry_domain::validate_requirements_document(&body))
    }

    pub async fn publish(
        &self,
        actor: Actor,
        command: PublishRequirementsDocument,
    ) -> Result<RequirementsPublished, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.require_human(&account)?;
        self.project_role(&account, command.project_id, ProjectRole::Maintainer)
            .await?;
        let draft = self
            .repository
            .draft(command.project_id)
            .await?
            .ok_or_else(|| ServiceError::DraftNotFound(command.project_id.to_string()))?;
        if draft.revision != command.expected_revision {
            return Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let validation = foundry_domain::validate_requirements_document(&draft.body);
        if !validation.is_valid() {
            return Err(ServiceError::RequirementsInvalid(validation));
        }
        match self
            .repository
            .publish(
                command.project_id,
                command.expected_revision,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await
            .map_err(requirements_repo_error)?
        {
            RequirementsPublishOutcome::Published { version, revision } => {
                self.emit(
                    DomainEventKind::RequirementsVersionPublished,
                    command.project_id,
                    &account,
                    json!({
                        "project_id": command.project_id,
                        "version": version,
                        "revision": revision,
                    }),
                )
                .await?;
                Ok(RequirementsPublished {
                    version,
                    revision,
                    warnings: validation.warnings,
                })
            }
            RequirementsPublishOutcome::Conflict {
                current_revision,
                current_updated_at,
            } => Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: current_revision,
                current_updated_at,
            }),
            RequirementsPublishOutcome::NoDraft => {
                Err(ServiceError::DraftNotFound(command.project_id.to_string()))
            }
            RequirementsPublishOutcome::TooLarge => Err(ServiceError::RequirementsTooLarge),
        }
    }

    pub async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<foundry_domain::RequirementsVersionRow>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.repository.versions(project_id).await?)
    }

    pub async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<RequirementsVersionView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        let version = self
            .repository
            .version(project_id, version)
            .await?
            .ok_or_else(|| {
                ServiceError::not_found_named(
                    "requirements document version",
                    format!("{project_id}@{version}"),
                )
            })?;
        Ok(RequirementsVersionView {
            project_id,
            version: version.version,
            body: version.body,
            created_by: version.created_by,
            published_at: version.published_at,
        })
    }

    pub async fn list_annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.repository.annotations(project_id, include_resolved).await?)
    }

    pub async fn add_annotation(
        &self,
        actor: Actor,
        command: AnnotateRequirements,
    ) -> Result<RequirementsAnnotation, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let body = command.body.trim();
        if body.is_empty() {
            return Err(ServiceError::Validation(
                "annotation body is required".to_string(),
            ));
        }
        let target_id = command
            .target_id
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if command.target_type != AnnotationTarget::Document && target_id.is_none() {
            return Err(ServiceError::Validation(
                "target_id is required for this annotation".to_string(),
            ));
        }
        match self.repository.draft(command.project_id).await? {
            Some(draft) => {
                if !annotation_target_exists(&draft.body, command.target_type, target_id.as_deref())
                {
                    return Err(ServiceError::Validation(format!(
                        "annotation target {} {:?} is not in the draft",
                        command.target_type.as_str(),
                        target_id
                    )));
                }
            }
            None if command.target_type != AnnotationTarget::Document => {
                return Err(ServiceError::Validation(
                    "requirements draft is empty".to_string(),
                ));
            }
            None => {}
        }
        let annotation = RequirementsAnnotation {
            id: Uuid::now_v7(),
            project_id: command.project_id,
            target_type: command.target_type,
            target_id,
            field: command
                .field
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            body: body.to_string(),
            author_id: account.id,
            author_handle: account.handle.clone(),
            author_name: account.display_name.clone(),
            status: AnnotationStatus::Open,
            resolution: None,
            created_at: OffsetDateTime::now_utc(),
            resolved_at: None,
            resolved_by: None,
        };
        self.repository.insert_annotation(&annotation).await?;
        Ok(annotation)
    }

    pub async fn resolve_annotation_inner(
        &self,
        actor: Actor,
        command: ResolveRequirementsAnnotation,
    ) -> Result<RequirementsAnnotation, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let resolution = command.resolution.trim();
        if resolution.is_empty() {
            return Err(ServiceError::Validation(
                "resolution is required".to_string(),
            ));
        }
        self.repository
            .resolve_annotation(
                command.project_id,
                command.annotation_id,
                account.id,
                resolution,
                OffsetDateTime::now_utc(),
            )
            .await?
            .ok_or_else(|| {
                ServiceError::not_found("requirements annotation", command.annotation_id)
            })
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

    fn require_human(&self, account: &Account) -> Result<(), ServiceError> {
        if account.kind != foundry_domain::AccountKind::Human {
            return Err(ServiceError::Forbidden(
                "agent accounts cannot publish requirements documents".to_string(),
            ));
        }
        Ok(())
    }

    async fn project_role(
        &self,
        account: &Account,
        project_id: Uuid,
        minimum: ProjectRole,
    ) -> Result<ProjectRole, ServiceError> {
        let project = self
            .projects
            .get(project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", project_id))?;
        if project.status == ProjectStatus::Archived {
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
        Ok(role)
    }

    async fn emit(
        &self,
        kind: DomainEventKind,
        project_id: Uuid,
        account: &Account,
        payload: serde_json::Value,
    ) -> Result<(), ServiceError> {
        let event = DomainEvent::new(kind, project_id, account.actor(), payload);
        self.events.publish(&[event]).await?;
        Ok(())
    }
}

fn annotation_target_exists(
    body: &RequirementsDocumentBody,
    target: AnnotationTarget,
    target_id: Option<&str>,
) -> bool {
    let Some(target_id) = target_id else {
        return target == AnnotationTarget::Document;
    };
    match target {
        AnnotationTarget::Document => true,
        AnnotationTarget::User => body.users.iter().any(|user| user.id == target_id),
        AnnotationTarget::Function => body.functions.iter().any(|function| function.id == target_id),
        AnnotationTarget::Story => body
            .functions
            .iter()
            .any(|function| function.stories.iter().any(|story| story.id == target_id)),
        AnnotationTarget::Operation => body.functions.iter().any(|function| {
            function
                .operations
                .iter()
                .any(|operation| operation.id == target_id)
        }),
    }
}

fn requirements_repo_error(error: RepoError) -> ServiceError {
    match error {
        RepoError::Invalid(message) => ServiceError::Validation(message),
        other => other.into(),
    }
}

fn draft_view(draft: RequirementsDraftDAO) -> RequirementsDraftView {
    RequirementsDraftView {
        project_id: draft.project_id,
        revision: draft.revision,
        base_version: draft.base_version,
        body: draft.body,
        updated_at: draft.updated_at,
    }
}

#[async_trait]
pub trait RequirementsDocumentUseCases: Send + Sync {
    async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDocumentView, ServiceError>;

    async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDraftView, ServiceError>;

    async fn apply(
        &self,
        actor: Actor,
        command: ApplyRequirementsOperations,
    ) -> Result<RequirementsDraftView, ServiceError>;

    async fn validate(&self, actor: Actor, project_id: Uuid) -> Result<Validation, ServiceError>;

    async fn publish(
        &self,
        actor: Actor,
        command: PublishRequirementsDocument,
    ) -> Result<RequirementsPublished, ServiceError>;

    async fn annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, ServiceError>;

    async fn annotate(
        &self,
        actor: Actor,
        command: AnnotateRequirements,
    ) -> Result<RequirementsAnnotation, ServiceError>;

    async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveRequirementsAnnotation,
    ) -> Result<RequirementsAnnotation, ServiceError>;

    async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<foundry_domain::RequirementsVersionRow>, ServiceError>;

    async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<RequirementsVersionView, ServiceError>;
}

#[async_trait]
impl<R, A, P, E> RequirementsDocumentUseCases for RequirementsDocumentService<R, A, P, E>
where
    R: RequirementsDocumentRepository,
    A: AccountRepository,
    P: ProjectRepository,
    E: EventSink,
{
    async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDocumentView, ServiceError> {
        RequirementsDocumentService::get(self, actor, project_id).await
    }

    async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<RequirementsDraftView, ServiceError> {
        RequirementsDocumentService::draft(self, actor, project_id).await
    }

    async fn apply(
        &self,
        actor: Actor,
        command: ApplyRequirementsOperations,
    ) -> Result<RequirementsDraftView, ServiceError> {
        RequirementsDocumentService::apply(self, actor, command).await
    }

    async fn validate(&self, actor: Actor, project_id: Uuid) -> Result<Validation, ServiceError> {
        RequirementsDocumentService::validate(self, actor, project_id).await
    }

    async fn publish(
        &self,
        actor: Actor,
        command: PublishRequirementsDocument,
    ) -> Result<RequirementsPublished, ServiceError> {
        RequirementsDocumentService::publish(self, actor, command).await
    }

    async fn annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, ServiceError> {
        RequirementsDocumentService::list_annotations(self, actor, project_id, include_resolved)
            .await
    }

    async fn annotate(
        &self,
        actor: Actor,
        command: AnnotateRequirements,
    ) -> Result<RequirementsAnnotation, ServiceError> {
        RequirementsDocumentService::add_annotation(self, actor, command).await
    }

    async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveRequirementsAnnotation,
    ) -> Result<RequirementsAnnotation, ServiceError> {
        RequirementsDocumentService::resolve_annotation_inner(self, actor, command).await
    }

    async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<foundry_domain::RequirementsVersionRow>, ServiceError> {
        RequirementsDocumentService::versions(self, actor, project_id).await
    }

    async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<RequirementsVersionView, ServiceError> {
        RequirementsDocumentService::version(self, actor, project_id, version).await
    }
}
