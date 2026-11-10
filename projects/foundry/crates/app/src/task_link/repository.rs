//! Storage for task links.

use async_trait::async_trait;
use foundry_domain::TaskLink;
use uuid::Uuid;

use crate::RepoError;

#[async_trait]
pub trait TaskLinkRepository: Send + Sync {
    async fn list(&self, task_id: Uuid) -> Result<Vec<TaskLink>, RepoError>;

    async fn get(&self, task_id: Uuid, link_id: Uuid) -> Result<Option<TaskLink>, RepoError>;

    async fn insert(&self, link: &TaskLink) -> Result<(), RepoError>;

    async fn delete(&self, task_id: Uuid, link_id: Uuid) -> Result<(), RepoError>;
}
