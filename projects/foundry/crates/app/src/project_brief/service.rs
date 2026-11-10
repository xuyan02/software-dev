//! Application service for project brief drafts and versions.

use async_trait::async_trait;
use foundry_domain::{
    Account, AccountKind, AccountStatus, Actor, DomainEvent, DomainEventKind,
    ProjectBriefAnnotation, ProjectBriefAnnotationTarget, ProjectBriefBody,
    ProjectBriefDraftDAO, ProjectBriefDraftOperation, ProjectBriefStatus,
    ProjectBriefVersionDAO, ProjectRole, ProjectStatus,
};
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AccountRepository, ApproveBriefOutcome, DraftOperationOutcome, EventSink,
    ProjectBriefRepository, ProjectRepository, ServiceError, SubmitDraftOutcome,
};

#[derive(Debug, Clone)]
pub struct CreateProjectBriefDraft {
    pub project_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct ApplyProjectBriefDraftOperation {
    pub project_id: Uuid,
    pub draft_id: Uuid,
    pub op_id: Uuid,
    pub base_revision: u64,
    pub operation: ProjectBriefDraftOperation,
}

#[derive(Debug, Clone)]
pub struct SubmitProjectBriefDraft {
    pub project_id: Uuid,
    pub draft_id: Uuid,
    pub expected_revision: u64,
}

#[derive(Debug, Clone)]
pub struct ApproveProjectBrief {
    pub project_id: Uuid,
    pub expected_version: u32,
}

#[derive(Debug, Clone)]
pub struct AnnotateProjectBrief {
    pub project_id: Uuid,
    pub target_type: ProjectBriefAnnotationTarget,
    pub target_id: Option<String>,
    pub field: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct ResolveProjectBriefAnnotation {
    pub project_id: Uuid,
    pub annotation_id: Uuid,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectBriefDraftView {
    pub id: Uuid,
    pub project_id: Uuid,
    pub revision: u64,
    pub body: ProjectBriefBody,
    pub created_by: Uuid,
    pub updated_by: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectBriefVersionView {
    pub project_id: Uuid,
    pub version: u32,
    pub status: ProjectBriefStatus,
    pub body: ProjectBriefBody,
    pub created_by: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approved_by: Option<Uuid>,
    pub editable: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub struct ProjectBriefService<R, A, P, E> {
    repository: Arc<R>,
    accounts: Arc<A>,
    projects: Arc<P>,
    events: Arc<E>,
}

impl<R, A, P, E> ProjectBriefService<R, A, P, E>
where
    R: ProjectBriefRepository,
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

    pub async fn current(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let role = self
            .project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self
            .repository
            .current(project_id)
            .await?
            .map(|version| version_view(version, role >= ProjectRole::Member)))
    }

    pub async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefVersionView>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let role = self
            .project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self
            .repository
            .versions(project_id)
            .await?
            .into_iter()
            .map(|version| version_view(version, role >= ProjectRole::Member))
            .collect())
    }

    pub async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError> {
        let account = self.require_account(&actor).await?;
        let role = self
            .project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self
            .repository
            .version(project_id, version)
            .await?
            .map(|version| version_view(version, role >= ProjectRole::Member)))
    }

    pub async fn create_draft(
        &self,
        actor: Actor,
        command: CreateProjectBriefDraft,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let body = self
            .repository
            .current(command.project_id)
            .await?
            .map(|version| version.body)
            .unwrap_or_default();
        let draft = self
            .repository
            .create_draft(
                command.project_id,
                &body,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?;
        Ok(draft_view(draft))
    }

    pub async fn drafts(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefDraftView>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        Ok(self
            .repository
            .drafts(project_id, account.id)
            .await?
            .into_iter()
            .map(draft_view)
            .collect())
    }

    pub async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Member)
            .await?;
        let draft = self
            .repository
            .draft(project_id, draft_id)
            .await?
            .filter(|draft| draft.created_by == account.id)
            .ok_or_else(|| ServiceError::not_found("project brief draft", draft_id))?;
        Ok(draft_view(draft))
    }

    pub async fn apply_operation(
        &self,
        actor: Actor,
        command: ApplyProjectBriefDraftOperation,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let draft = self
            .repository
            .draft(command.project_id, command.draft_id)
            .await?
            .filter(|draft| draft.created_by == account.id)
            .ok_or_else(|| ServiceError::not_found("project brief draft", command.draft_id))?;
        if draft.revision != command.base_revision {
            return Err(ServiceError::DraftConflict {
                expected: command.base_revision,
                current: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let mut body = draft.body;
        command.operation.clone().apply(&mut body);
        let outcome = self
            .repository
            .commit_draft_operation(
                command.project_id,
                command.draft_id,
                command.base_revision,
                command.op_id,
                &command.operation,
                &body,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?;
        match outcome {
            DraftOperationOutcome::Applied(draft) | DraftOperationOutcome::Duplicate(draft) => {
                self.emit(
                    DomainEventKind::ProjectBriefDrafted,
                    command.project_id,
                    &account,
                    json!({ "draft_id": draft.id, "revision": draft.revision }),
                )
                .await?;
                Ok(draft_view(draft))
            }
            DraftOperationOutcome::Conflict { current_revision } => {
                Err(ServiceError::DraftConflict {
                    expected: command.base_revision,
                    current: current_revision,
                    current_updated_at: OffsetDateTime::now_utc(),
                })
            }
        }
    }

    pub async fn submit_draft(
        &self,
        actor: Actor,
        command: SubmitProjectBriefDraft,
    ) -> Result<ProjectBriefVersionView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, command.project_id, ProjectRole::Member)
            .await?;
        let draft = self
            .repository
            .draft(command.project_id, command.draft_id)
            .await?
            .filter(|draft| draft.created_by == account.id)
            .ok_or_else(|| ServiceError::not_found("project brief draft", command.draft_id))?;
        if draft.revision != command.expected_revision {
            return Err(ServiceError::DraftConflict {
                expected: command.expected_revision,
                current: draft.revision,
                current_updated_at: draft.updated_at,
            });
        }
        let validation = foundry_domain::validate_project_brief(&draft.body);
        if !validation.is_valid() {
            return Err(ServiceError::ProjectBriefInvalid(validation));
        }
        match self
            .repository
            .submit_draft(
                command.project_id,
                command.draft_id,
                command.expected_revision,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?
        {
            SubmitDraftOutcome::Submitted(version) => {
                self.emit(
                    DomainEventKind::ProjectBriefSubmitted,
                    command.project_id,
                    &account,
                    json!({ "version": version.version, "draft_id": command.draft_id }),
                )
                .await?;
                Ok(version_view(*version, true))
            }
            SubmitDraftOutcome::Conflict { current_revision } => {
                Err(ServiceError::DraftConflict {
                    expected: command.expected_revision,
                    current: current_revision,
                    current_updated_at: OffsetDateTime::now_utc(),
                })
            }
        }
    }

    pub async fn approve(
        &self,
        actor: Actor,
        command: ApproveProjectBrief,
    ) -> Result<ProjectBriefVersionView, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.require_human(&account)?;
        self.project_role(&account, command.project_id, ProjectRole::Maintainer)
            .await?;
        match self
            .repository
            .approve_current(
                command.project_id,
                command.expected_version,
                account.id,
                OffsetDateTime::now_utc(),
            )
            .await?
        {
            ApproveBriefOutcome::Approved(version) => {
                self.emit(
                    DomainEventKind::ProjectBriefApproved,
                    command.project_id,
                    &account,
                    json!({ "version": version.version }),
                )
                .await?;
                Ok(version_view(*version, false))
            }
            ApproveBriefOutcome::Conflict { current_version } => Err(ServiceError::Conflict {
                expected: command.expected_version,
                actual: current_version,
            }),
            ApproveBriefOutcome::NotInReview => Err(ServiceError::InvalidTransition(
                "project brief must be in review before approval".to_string(),
            )),
        }
    }

    pub async fn list_annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, ServiceError> {
        let account = self.require_account(&actor).await?;
        self.project_role(&account, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.repository.annotations(project_id, include_resolved).await?)
    }

    pub async fn add_annotation(
        &self,
        actor: Actor,
        command: AnnotateProjectBrief,
    ) -> Result<ProjectBriefAnnotation, ServiceError> {
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
        let current = self.repository.current(command.project_id).await?;
        if command.target_type != ProjectBriefAnnotationTarget::Document {
            let Some(current) = current.as_ref() else {
                return Err(ServiceError::Validation(
                    "project brief is empty".to_string(),
                ));
            };
            if !brief_annotation_target_exists(
                &current.body,
                command.target_type,
                target_id.as_deref(),
            ) {
                return Err(ServiceError::Validation(format!(
                    "annotation target {} {:?} is not in the brief",
                    command.target_type.as_str(),
                    target_id
                )));
            }
        }
        let annotation = ProjectBriefAnnotation {
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
            status: foundry_domain::AnnotationStatus::Open,
            resolution: None,
            created_at: OffsetDateTime::now_utc(),
            resolved_at: None,
            resolved_by: None,
        };
        self.repository.insert_annotation(&annotation).await?;
        Ok(annotation)
    }

    pub async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveProjectBriefAnnotation,
    ) -> Result<ProjectBriefAnnotation, ServiceError> {
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
                ServiceError::not_found("project brief annotation", command.annotation_id)
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
        if account.kind != AccountKind::Human {
            return Err(ServiceError::Forbidden(
                "agent accounts cannot approve project briefs".to_string(),
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

fn draft_view(draft: ProjectBriefDraftDAO) -> ProjectBriefDraftView {
    ProjectBriefDraftView {
        id: draft.id,
        project_id: draft.project_id,
        revision: draft.revision,
        body: draft.body,
        created_by: draft.created_by,
        updated_by: draft.updated_by,
        created_at: draft.created_at,
        updated_at: draft.updated_at,
    }
}

fn version_view(version: ProjectBriefVersionDAO, editable: bool) -> ProjectBriefVersionView {
    ProjectBriefVersionView {
        project_id: version.project_id,
        version: version.version,
        status: version.status,
        body: version.body,
        created_by: version.created_by,
        approved_by: version.approved_by,
        editable,
        created_at: version.created_at,
        updated_at: version.updated_at,
    }
}

fn brief_annotation_target_exists(
    body: &ProjectBriefBody,
    target: ProjectBriefAnnotationTarget,
    target_id: Option<&str>,
) -> bool {
    let Some(target_id) = target_id else {
        return true;
    };
    match target {
        ProjectBriefAnnotationTarget::Risks => {
            body.risks.iter().any(|risk| risk.id == target_id)
        }
        ProjectBriefAnnotationTarget::Terms => {
            body.terms.iter().any(|term| term.term == target_id)
        }
        ProjectBriefAnnotationTarget::Document
        | ProjectBriefAnnotationTarget::Problem
        | ProjectBriefAnnotationTarget::Goals
        | ProjectBriefAnnotationTarget::Scope
        | ProjectBriefAnnotationTarget::Constraints
        | ProjectBriefAnnotationTarget::Budget => true,
    }
}

#[async_trait]
pub trait ProjectBriefUseCases: Send + Sync {
    async fn current(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError>;
    async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefVersionView>, ServiceError>;
    async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError>;
    async fn create_draft(
        &self,
        actor: Actor,
        command: CreateProjectBriefDraft,
    ) -> Result<ProjectBriefDraftView, ServiceError>;
    async fn drafts(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefDraftView>, ServiceError>;
    async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<ProjectBriefDraftView, ServiceError>;
    async fn apply_operation(
        &self,
        actor: Actor,
        command: ApplyProjectBriefDraftOperation,
    ) -> Result<ProjectBriefDraftView, ServiceError>;
    async fn submit_draft(
        &self,
        actor: Actor,
        command: SubmitProjectBriefDraft,
    ) -> Result<ProjectBriefVersionView, ServiceError>;
    async fn approve(
        &self,
        actor: Actor,
        command: ApproveProjectBrief,
    ) -> Result<ProjectBriefVersionView, ServiceError>;
    async fn annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, ServiceError>;
    async fn annotate(
        &self,
        actor: Actor,
        command: AnnotateProjectBrief,
    ) -> Result<ProjectBriefAnnotation, ServiceError>;
    async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveProjectBriefAnnotation,
    ) -> Result<ProjectBriefAnnotation, ServiceError>;
}

#[async_trait]
impl<R, A, P, E> ProjectBriefUseCases for ProjectBriefService<R, A, P, E>
where
    R: ProjectBriefRepository,
    A: AccountRepository,
    P: ProjectRepository,
    E: EventSink,
{
    async fn current(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError> {
        ProjectBriefService::current(self, actor, project_id).await
    }

    async fn versions(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefVersionView>, ServiceError> {
        ProjectBriefService::versions(self, actor, project_id).await
    }

    async fn version(
        &self,
        actor: Actor,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionView>, ServiceError> {
        ProjectBriefService::version(self, actor, project_id, version).await
    }

    async fn create_draft(
        &self,
        actor: Actor,
        command: CreateProjectBriefDraft,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        ProjectBriefService::create_draft(self, actor, command).await
    }

    async fn drafts(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefDraftView>, ServiceError> {
        ProjectBriefService::drafts(self, actor, project_id).await
    }

    async fn draft(
        &self,
        actor: Actor,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        ProjectBriefService::draft(self, actor, project_id, draft_id).await
    }

    async fn apply_operation(
        &self,
        actor: Actor,
        command: ApplyProjectBriefDraftOperation,
    ) -> Result<ProjectBriefDraftView, ServiceError> {
        ProjectBriefService::apply_operation(self, actor, command).await
    }

    async fn submit_draft(
        &self,
        actor: Actor,
        command: SubmitProjectBriefDraft,
    ) -> Result<ProjectBriefVersionView, ServiceError> {
        ProjectBriefService::submit_draft(self, actor, command).await
    }

    async fn approve(
        &self,
        actor: Actor,
        command: ApproveProjectBrief,
    ) -> Result<ProjectBriefVersionView, ServiceError> {
        ProjectBriefService::approve(self, actor, command).await
    }

    async fn annotations(
        &self,
        actor: Actor,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, ServiceError> {
        ProjectBriefService::list_annotations(self, actor, project_id, include_resolved).await
    }

    async fn annotate(
        &self,
        actor: Actor,
        command: AnnotateProjectBrief,
    ) -> Result<ProjectBriefAnnotation, ServiceError> {
        ProjectBriefService::add_annotation(self, actor, command).await
    }

    async fn resolve_annotation(
        &self,
        actor: Actor,
        command: ResolveProjectBriefAnnotation,
    ) -> Result<ProjectBriefAnnotation, ServiceError> {
        ProjectBriefService::resolve_annotation(self, actor, command).await
    }
}
