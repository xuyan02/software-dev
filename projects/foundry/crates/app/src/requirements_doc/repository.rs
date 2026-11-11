//! Storage for the structured requirements document.
//!
//! Published versions are immutable. The draft is a single shared working copy
//! with an optimistic `revision`; operations are appended to a log.

use async_trait::async_trait;
use foundry_domain::{
    RequirementsAnnotation, RequirementsApprovalDAO, RequirementsDocumentBody,
    RequirementsDocumentDAO, RequirementsDraftDAO, RequirementsOperation, RequirementsVersionDAO,
    RequirementsVersionRow,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::RepoError;

#[derive(Debug, Clone)]
pub enum RequirementsApplyOutcome {
    Applied {
        revision: u64,
        updated_at: OffsetDateTime,
    },
    /// The draft is absent; the caller must ensure it first.
    NoDraft,
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
    TooLarge,
}

#[derive(Debug, Clone)]
pub enum RequirementsApprovalOutcome {
    Approved {
        version: u32,
        revision: u64,
        approval_id: Uuid,
    },
    Rejected {
        approval_id: Uuid,
    },
    /// The revision was already approved and a version exists.
    AlreadyPublished {
        version: u32,
    },
    NoDraft,
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
    TooLarge,
}

#[async_trait]
pub trait RequirementsDocumentRepository: Send + Sync {
    /// Read the document row without creating it.
    async fn document(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsDocumentDAO>, RepoError>;

    /// Create the document and its empty draft if they do not exist.
    async fn ensure_document(
        &self,
        project_id: Uuid,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(RequirementsDocumentDAO, RequirementsDraftDAO), RepoError>;

    async fn draft(&self, project_id: Uuid) -> Result<Option<RequirementsDraftDAO>, RepoError>;

    /// Apply structured operations to the shared draft. The adapter checks
    /// `expected_revision`, applies all operations atomically, appends the
    /// operation log, and advances the revision.
    async fn apply_operations(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        operations: &[RequirementsOperation],
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError>;

    /// Approve the current draft revision and publish it as an immutable
    /// version in one transaction.
    async fn approve(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        comment: Option<&str>,
        now: OffsetDateTime,
    ) -> Result<RequirementsApprovalOutcome, RepoError>;

    /// Record a rejection. The draft stays editable and no version is created.
    async fn reject(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        comment: &str,
        now: OffsetDateTime,
    ) -> Result<RequirementsApprovalOutcome, RepoError>;

    async fn approvals(&self, project_id: Uuid) -> Result<Vec<RequirementsApprovalDAO>, RepoError>;

    async fn versions(&self, project_id: Uuid) -> Result<Vec<RequirementsVersionRow>, RepoError>;

    async fn version(
        &self,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError>;

    async fn latest_version(
        &self,
        project_id: Uuid,
    ) -> Result<Option<RequirementsVersionDAO>, RepoError>;

    /// Update the draft body for the next stage (interaction/UI). This is the
    /// same optimistic path as `apply_operations`, but accepts a full body.
    async fn replace_draft(
        &self,
        project_id: Uuid,
        expected_revision: u64,
        body: &RequirementsDocumentBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<RequirementsApplyOutcome, RepoError>;

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<RequirementsAnnotation>, RepoError>;

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<RequirementsAnnotation>, RepoError>;

    async fn insert_annotation(&self, annotation: &RequirementsAnnotation)
    -> Result<(), RepoError>;

    /// Resolve an open annotation. Returns the updated annotation, or `None`
    /// when it does not exist or is already resolved.
    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
        resolver_id: Uuid,
        resolution: &str,
        now: OffsetDateTime,
    ) -> Result<Option<RequirementsAnnotation>, RepoError>;
}
