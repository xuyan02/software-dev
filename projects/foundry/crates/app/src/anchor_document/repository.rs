//! Storage for schema-described anchor documents.
//!
//! Drafts carry one shared revision; published versions are immutable. The
//! snapshot holds the anchor tree and every anchor keeps its content-addressed
//! ID.

use async_trait::async_trait;
use foundry_domain::{
    AnchorAnnotation, AnchorOperation, ApprovalDecision, DocumentSchema, DocumentSnapshot,
    Validation,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::RepoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorDocumentDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub schema_version: u32,
    pub current_version: u32,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorDocumentDraftDAO {
    pub document_id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub revision: u64,
    pub base_version: u32,
    pub schema_version: u32,
    pub snapshot: DocumentSnapshot,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorDocumentVersionDAO {
    pub document_id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub version: u32,
    pub snapshot: DocumentSnapshot,
    pub content_hash: String,
    pub source_revision: u64,
    pub created_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub approval_comment: Option<String>,
    pub created_at: OffsetDateTime,
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnchorDocumentVersionRow {
    pub version: u32,
    pub created_by: Uuid,
    pub approved_by: Option<Uuid>,
    pub source_revision: Option<u64>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnchorDocumentApprovalDAO {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub revision: u64,
    pub decision: ApprovalDecision,
    pub actor_id: Uuid,
    pub comment: Option<String>,
    pub content_hash: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub enum AnchorApplyOutcome {
    Applied {
        revision: u64,
        root_id: String,
        created_key: Option<String>,
        updated_at: OffsetDateTime,
    },
    NoDraft,
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
    TooLarge,
    Invalid(String),
}

#[derive(Debug, Clone)]
pub enum AnchorApprovalOutcome {
    Approved {
        version: u32,
        revision: u64,
        approval_id: Uuid,
    },
    Rejected {
        approval_id: Uuid,
    },
    AlreadyPublished {
        version: u32,
    },
    NoDraft,
    Conflict {
        current_revision: u64,
        current_updated_at: OffsetDateTime,
    },
    TooLarge,
    Invalid(Validation),
}

#[allow(clippy::too_many_arguments)]
#[async_trait]
pub trait AnchorDocumentRepository: Send + Sync {
    async fn document(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDAO>, RepoError>;

    async fn ensure_document(
        &self,
        project_id: Uuid,
        kind: &str,
        schema: &DocumentSchema,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(AnchorDocumentDAO, AnchorDocumentDraftDAO), RepoError>;

    async fn draft(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentDraftDAO>, RepoError>;

    async fn apply_operations(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        operations: &[AnchorOperation],
        actor_id: Uuid,
        now: OffsetDateTime,
        schema: &DocumentSchema,
    ) -> Result<AnchorApplyOutcome, RepoError>;

    async fn approve(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        actor_id: Uuid,
        comment: Option<&str>,
        now: OffsetDateTime,
        schema: &DocumentSchema,
    ) -> Result<AnchorApprovalOutcome, RepoError>;

    async fn reject(
        &self,
        project_id: Uuid,
        kind: &str,
        expected_revision: u64,
        actor_id: Uuid,
        comment: &str,
        now: OffsetDateTime,
    ) -> Result<AnchorApprovalOutcome, RepoError>;

    async fn approvals(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentApprovalDAO>, RepoError>;

    async fn versions(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorDocumentVersionRow>, RepoError>;

    async fn version(
        &self,
        project_id: Uuid,
        kind: &str,
        version: u32,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError>;

    async fn latest_version(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Option<AnchorDocumentVersionDAO>, RepoError>;

    async fn annotations(
        &self,
        project_id: Uuid,
        kind: &str,
    ) -> Result<Vec<AnchorAnnotation>, RepoError>;

    async fn insert_annotation(
        &self,
        annotation: AnchorAnnotation,
    ) -> Result<AnchorAnnotation, RepoError>;

    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        kind: &str,
        annotation_id: Uuid,
        resolution: &str,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<Option<AnchorAnnotation>, RepoError>;
}
