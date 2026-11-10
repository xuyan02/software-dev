//! Storage for project brief drafts and versions.

use async_trait::async_trait;
use foundry_domain::{
    ProjectBriefAnnotation, ProjectBriefBody, ProjectBriefDraftDAO, ProjectBriefDraftOperation,
    ProjectBriefVersionDAO,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::RepoError;

#[derive(Debug, Clone)]
pub enum DraftOperationOutcome {
    Applied(ProjectBriefDraftDAO),
    /// The `op_id` was already applied; the current draft is returned unchanged.
    Duplicate(ProjectBriefDraftDAO),
    Conflict { current_revision: u64 },
}

#[derive(Debug, Clone)]
pub enum SubmitDraftOutcome {
    Submitted(Box<ProjectBriefVersionDAO>),
    Conflict { current_revision: u64 },
}

#[derive(Debug, Clone)]
pub enum ApproveBriefOutcome {
    Approved(Box<ProjectBriefVersionDAO>),
    Conflict { current_version: u32 },
    NotInReview,
}

#[derive(Debug, Clone)]
pub enum RejectBriefOutcome {
    Rejected(Box<ProjectBriefVersionDAO>),
    Conflict { current_version: u32 },
    NotInReview,
}

#[async_trait]
pub trait ProjectBriefRepository: Send + Sync {
    async fn current(&self, project_id: Uuid) -> Result<Option<ProjectBriefVersionDAO>, RepoError>;

    async fn versions(&self, project_id: Uuid)
    -> Result<Vec<ProjectBriefVersionDAO>, RepoError>;

    async fn version(
        &self,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionDAO>, RepoError>;

    async fn create_draft(
        &self,
        project_id: Uuid,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ProjectBriefDraftDAO, RepoError>;

    /// Drafts created by one account. Drafts are private to their creator.
    async fn drafts(
        &self,
        project_id: Uuid,
        created_by: Uuid,
    ) -> Result<Vec<ProjectBriefDraftDAO>, RepoError>;

    async fn draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<Option<ProjectBriefDraftDAO>, RepoError>;

    /// Atomically record the operation and save the new draft body.
    #[allow(clippy::too_many_arguments)]
    async fn commit_draft_operation(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        op_id: Uuid,
        operation: &ProjectBriefDraftOperation,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftOperationOutcome, RepoError>;

    /// Create a new version from the draft, supersede the previous current
    /// version, move the current pointer and delete the draft.
    async fn submit_draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<SubmitDraftOutcome, RepoError>;

    async fn approve_current(
        &self,
        project_id: Uuid,
        expected_version: u32,
        approver_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ApproveBriefOutcome, RepoError>;

    /// Send the current in-review version back to the writer.
    async fn reject_current(
        &self,
        project_id: Uuid,
        expected_version: u32,
        now: OffsetDateTime,
    ) -> Result<RejectBriefOutcome, RepoError>;

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, RepoError>;

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError>;

    async fn insert_annotation(
        &self,
        annotation: &ProjectBriefAnnotation,
    ) -> Result<(), RepoError>;

    /// Resolve an open annotation. Returns the updated annotation, or `None`
    /// when it does not exist or is already resolved.
    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
        resolver_id: Uuid,
        resolution: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError>;
}
