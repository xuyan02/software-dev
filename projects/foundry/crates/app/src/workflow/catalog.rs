//! Workflow definition catalog.
//!
//! Definitions are data. The application never holds a mutable "current
//! workflow"; it asks the catalog for an exact published version, or for the
//! latest version visible in a scope.

use async_trait::async_trait;
use foundry_domain::{TaskWorkflow, WorkflowId, WorkflowOption, WorkflowVersionRef};
use std::sync::Arc;
use uuid::Uuid;

use crate::RepoError;

/// The outcome of looking up the latest published version in a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowLookup {
    Published(WorkflowVersionRef),
    /// The definition exists and is visible, but has no published version.
    Unpublished(WorkflowId),
    /// The definition does not exist or is not visible in this scope.
    NotFound,
}

/// Reads immutable workflow versions and resolves the latest published one.
#[async_trait]
pub trait WorkflowCatalog: Send + Sync {
    /// Load the immutable AST for an exact version.
    async fn pinned(
        &self,
        reference: &WorkflowVersionRef,
    ) -> Result<Option<Arc<dyn TaskWorkflow>>, RepoError>;

    /// Latest published version visible to `account_id`.
    ///
    /// `None` is the platform scope used when resolving children of platform
    /// definitions; it only sees platform definitions.
    async fn latest_visible(
        &self,
        account_id: Option<Uuid>,
        workflow_id: &WorkflowId,
    ) -> Result<WorkflowLookup, RepoError>;

    /// Resolve a child workflow in the parent definition owner's scope.
    async fn resolve_child(
        &self,
        parent: &WorkflowId,
        child: &WorkflowId,
    ) -> Result<Option<WorkflowVersionRef>, RepoError>;

    /// Published workflows a project creator may choose as the root, grouped
    /// by owner in the returned order.
    async fn root_candidates(&self, account_id: Uuid) -> Result<Vec<WorkflowOption>, RepoError>;
}
