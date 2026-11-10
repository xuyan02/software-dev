//! In-memory task links.

use async_trait::async_trait;
use foundry_app::{RepoError, TaskLinkRepository};
use foundry_domain::TaskLink;
use uuid::Uuid;

use crate::InMemoryStore;

#[derive(Default)]
pub(crate) struct MemoryTaskLinks {
    links: Vec<TaskLink>,
}

impl MemoryTaskLinks {
    pub(crate) fn insert_link(&mut self, link: &TaskLink) -> Result<(), RepoError> {
        if self.links.iter().any(|existing| {
            existing.task_id == link.task_id
                && existing.kind == link.kind
                && existing.relation == link.relation
                && match (existing.target_id, link.target_id) {
                    (Some(left), Some(right)) => left == right,
                    (None, None) => existing.url == link.url,
                    _ => false,
                }
        }) {
            return Err(RepoError::Duplicate(format!(
                "task link on {}",
                link.task_id
            )));
        }
        self.links.push(link.clone());
        Ok(())
    }

    pub(crate) fn delete_tasks(&mut self, task_ids: &[Uuid]) {
        self.links.retain(|link| !task_ids.contains(&link.task_id));
    }
}

#[async_trait]
impl TaskLinkRepository for InMemoryStore {
    async fn list(&self, task_id: Uuid) -> Result<Vec<TaskLink>, RepoError> {
        Ok(self
            .task_links
            .read()
            .expect("lock poisoned")
            .links
            .iter()
            .filter(|link| link.task_id == task_id)
            .cloned()
            .collect())
    }

    async fn get(&self, task_id: Uuid, link_id: Uuid) -> Result<Option<TaskLink>, RepoError> {
        Ok(self
            .task_links
            .read()
            .expect("lock poisoned")
            .links
            .iter()
            .find(|link| link.task_id == task_id && link.id == link_id)
            .cloned())
    }

    async fn insert(&self, link: &TaskLink) -> Result<(), RepoError> {
        self.task_links
            .write()
            .expect("lock poisoned")
            .insert_link(link)
    }

    async fn delete(&self, task_id: Uuid, link_id: Uuid) -> Result<(), RepoError> {
        let mut links = self.task_links.write().expect("lock poisoned");
        let before = links.links.len();
        links
            .links
            .retain(|link| !(link.task_id == task_id && link.id == link_id));
        if links.links.len() == before {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}
