//! Storage for task links.

use async_trait::async_trait;
use foundry_domain::{TaskLink, TaskLinkKind};
use uuid::Uuid;

use crate::RepoError;

#[async_trait]
pub trait TaskLinkRepository: Send + Sync {
    async fn list(&self, task_id: Uuid) -> Result<Vec<TaskLink>, RepoError>;

    async fn list_by_target(
        &self,
        kind: TaskLinkKind,
        target_id: Uuid,
    ) -> Result<Vec<TaskLink>, RepoError>;

    async fn get(&self, task_id: Uuid, link_id: Uuid) -> Result<Option<TaskLink>, RepoError>;

    async fn insert(&self, link: &TaskLink) -> Result<(), RepoError>;

    async fn delete(&self, task_id: Uuid, link_id: Uuid) -> Result<(), RepoError>;

    /// Clear the current handling link for an Issue, keeping it as history.
    async fn deactivate_issue_links(&self, target_id: Uuid) -> Result<(), RepoError>;

    /// Make this task's Issue links the current handlers, deactivating the
    /// previous handlers for the same Issues.
    async fn activate_task_issue_links(&self, task_id: Uuid) -> Result<(), RepoError>;
}
