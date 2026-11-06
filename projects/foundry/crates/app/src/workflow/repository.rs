//! Storage for workflow definitions, versions, drafts and layouts.
//!
//! Published versions are immutable: the trait exposes no update or delete
//! path. Drafts and layouts are mutable working state with their own
//! concurrency rules.

use async_trait::async_trait;
use foundry_domain::{
    WorkflowBody, WorkflowDefinitionDAO, WorkflowDraftDAO, WorkflowId, WorkflowLayout,
    WorkflowLayoutDAO, WorkflowVersionDAO,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::RepoError;

/// A definition with the derived counts shown in lists and detail views.
#[derive(Debug, Clone)]
pub struct WorkflowDefinitionRow {
    pub definition: WorkflowDefinitionDAO,
    pub latest_version: Option<u32>,
    pub draft_revision: Option<u64>,
}

/// Version metadata without its AST.
#[derive(Debug, Clone)]
pub struct WorkflowVersionRow {
    pub version: u32,
    pub published_by_account_id: Option<Uuid>,
    pub published_at: OffsetDateTime,
}

/// Result of a draft save. A retried `client_save_id` reports the current
/// revision without writing.
#[derive(Debug, Clone)]
pub enum DraftSaveOutcome {
    Saved {
        revision: u64,
        updated_at: OffsetDateTime,
    },
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
}

/// Result of publishing the shared draft.
#[derive(Debug, Clone)]
pub enum PublishOutcome {
    Published {
        version: u32,
        revision: u64,
    },
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
    NoDraft,
}

#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    /// Insert the definition, its initial draft and an optional copied layout
    /// in one transaction.
    async fn insert_definition(
        &self,
        definition: &WorkflowDefinitionDAO,
        draft: &WorkflowDraftDAO,
        layout: Option<&WorkflowLayoutDAO>,
    ) -> Result<(), RepoError>;

    async fn definition(&self, id: &WorkflowId)
    -> Result<Option<WorkflowDefinitionDAO>, RepoError>;

    async fn definition_row(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowDefinitionRow>, RepoError>;

    /// Visible definitions ordered by platform first, then title.
    async fn list_definitions(
        &self,
        account_id: Uuid,
        is_admin: bool,
    ) -> Result<Vec<WorkflowDefinitionRow>, RepoError>;

    async fn rename(
        &self,
        id: &WorkflowId,
        title: &str,
        now: OffsetDateTime,
    ) -> Result<(), RepoError>;

    async fn versions(&self, id: &WorkflowId) -> Result<Vec<WorkflowVersionRow>, RepoError>;

    async fn version(
        &self,
        id: &WorkflowId,
        version: u32,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError>;

    /// The newest published version, including its body.
    async fn latest_version(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError>;

    /// Ids that have at least one published version in `owner_account_id`'s
    /// scope. Platform scope (`None`) only contains platform definitions.
    async fn published_ids(
        &self,
        owner_account_id: Option<Uuid>,
    ) -> Result<Vec<WorkflowId>, RepoError>;

    async fn draft(&self, id: &WorkflowId) -> Result<Option<WorkflowDraftDAO>, RepoError>;

    /// Create the shared draft from `fallback` when it does not exist yet.
    async fn open_draft(
        &self,
        id: &WorkflowId,
        fallback: WorkflowBody,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowDraftDAO, RepoError>;

    async fn save_draft(
        &self,
        id: &WorkflowId,
        body: &WorkflowBody,
        expected_revision: u64,
        client_save_id: Uuid,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftSaveOutcome, RepoError>;

    /// Publish the current draft at `expected_revision`, inserting the next
    /// version and bumping the draft revision in one transaction.
    async fn publish(
        &self,
        id: &WorkflowId,
        expected_revision: u64,
        publisher: Uuid,
        now: OffsetDateTime,
    ) -> Result<PublishOutcome, RepoError>;

    async fn layout(&self, id: &WorkflowId) -> Result<Option<WorkflowLayoutDAO>, RepoError>;

    /// Last-write-wins: no revision is involved.
    async fn save_layout(
        &self,
        id: &WorkflowId,
        layout: &WorkflowLayout,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowLayoutDAO, RepoError>;
}
