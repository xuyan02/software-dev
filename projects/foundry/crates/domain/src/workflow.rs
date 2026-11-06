use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{WorkflowBody, WorkflowId};

/// An exact, immutable workflow version.
///
/// Tasks store this pair; they never follow the latest published version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WorkflowVersionRef {
    pub workflow_id: WorkflowId,
    pub version: u32,
}

/// Who owns a workflow definition. Platform definitions are visible to every
/// account; account definitions are private to their owner and administrators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowOwner {
    Platform,
    Account {
        account_id: Uuid,
        display_name: String,
    },
}

/// A workflow a project creator may choose as the project root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowOption {
    pub id: WorkflowId,
    pub title: String,
    pub owner: WorkflowOwner,
}

/// The persisted definition row. Versions, drafts and layouts are separate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinitionDAO {
    pub id: WorkflowId,
    pub owner_account_id: Option<Uuid>,
    pub title: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// One immutable published version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowVersionDAO {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub body: WorkflowBody,
    /// `None` only for the system-seeded builtin v1.
    pub published_by_account_id: Option<Uuid>,
    pub published_at: OffsetDateTime,
}

/// The single shared work-in-progress body of a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDraftDAO {
    pub workflow_id: WorkflowId,
    pub revision: u64,
    pub body: WorkflowBody,
    pub last_client_save_id: Option<Uuid>,
    pub updated_by_account_id: Uuid,
    pub updated_at: OffsetDateTime,
}

/// The display metadata shared by all versions of a definition.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowLayoutDAO {
    pub workflow_id: WorkflowId,
    pub layout: WorkflowLayout,
    pub updated_by_account_id: Uuid,
    pub updated_at: OffsetDateTime,
}

/// A definition as shown in the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: WorkflowId,
    pub title: String,
    pub owner: WorkflowOwner,
    pub latest_version: Option<u32>,
    /// Only exposed to viewers allowed to read the shared draft.
    pub draft_revision: Option<u64>,
    pub editable: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Version metadata for lists; the body is fetched separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVersionInfo {
    pub version: u32,
    pub published_by_account_id: Option<Uuid>,
    pub published_by_display_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: OffsetDateTime,
}

/// A complete immutable version, including its AST.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowVersion {
    pub workflow_id: WorkflowId,
    pub version: u32,
    pub body: WorkflowBody,
    pub published_by_account_id: Option<Uuid>,
    pub published_by_display_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub published_at: OffsetDateTime,
}

/// The shared draft as read by an editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDraftView {
    pub workflow_id: WorkflowId,
    pub revision: u64,
    pub body: WorkflowBody,
    pub updated_by_account_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// Node positions for the flowchart projection. Not part of the AST.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkflowLayout {
    #[serde(default)]
    pub positions: BTreeMap<String, StagePosition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagePosition {
    pub x: f64,
    pub y: f64,
}
