//! In-memory task links.

use async_trait::async_trait;
use foundry_app::{RepoError, TaskLinkRepository};
use foundry_domain::{TaskLink, TaskLinkKind};
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

    async fn list_by_target(
        &self,
        kind: TaskLinkKind,
        target_id: Uuid,
    ) -> Result<Vec<TaskLink>, RepoError> {
        Ok(self
            .task_links
            .read()
            .expect("lock poisoned")
            .links
            .iter()
            .filter(|link| link.kind == kind && link.target_id == Some(target_id))
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

    async fn deactivate_issue_links(&self, target_id: Uuid) -> Result<(), RepoError> {
        let mut links = self.task_links.write().expect("lock poisoned");
        for link in &mut links.links {
            if link.kind == TaskLinkKind::Issue && link.target_id == Some(target_id) {
                link.active = false;
            }
        }
        Ok(())
    }

    async fn activate_task_issue_links(&self, task_id: Uuid) -> Result<(), RepoError> {
        let mut links = self.task_links.write().expect("lock poisoned");
        let targets = links
            .links
            .iter()
            .filter(|link| link.task_id == task_id && link.kind == TaskLinkKind::Issue)
            .filter_map(|link| link.target_id)
            .collect::<Vec<_>>();
        for target_id in targets {
            for link in &mut links.links {
                if link.kind == TaskLinkKind::Issue && link.target_id == Some(target_id) {
                    link.active = link.task_id == task_id;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_domain::{Actor, TaskLink, TaskLinkRelation};
    use serde_json::Value;
    use time::OffsetDateTime;

    fn link(task_id: Uuid, issue_id: Uuid, active: bool) -> TaskLink {
        TaskLink {
            id: Uuid::now_v7(),
            task_id,
            kind: TaskLinkKind::Issue,
            target_id: Some(issue_id),
            url: None,
            title: "Issue".to_string(),
            relation: TaskLinkRelation::References,
            active,
            created_by: Actor::System,
            created_at: OffsetDateTime::now_utc(),
            metadata: Value::Null,
        }
    }

    #[tokio::test]
    async fn handoff_keeps_one_active_issue_link_and_keeps_history() {
        let store = InMemoryStore::new();
        let issue_id = Uuid::now_v7();
        let first_task = Uuid::now_v7();
        let second_task = Uuid::now_v7();
        store
            .insert(&link(first_task, issue_id, true))
            .await
            .unwrap();
        store
            .insert(&link(second_task, issue_id, false))
            .await
            .unwrap();

        store.activate_task_issue_links(second_task).await.unwrap();

        let links = store
            .list_by_target(TaskLinkKind::Issue, issue_id)
            .await
            .unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links.iter().filter(|link| link.active).count(), 1);
        assert!(
            links
                .iter()
                .find(|link| link.task_id == second_task)
                .unwrap()
                .active
        );
        assert!(
            !links
                .iter()
                .find(|link| link.task_id == first_task)
                .unwrap()
                .active
        );
    }
}
