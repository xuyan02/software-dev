//! In-memory persistence for design-space merge requests.

use crate::InMemoryStore;
use async_trait::async_trait;
use foundry_app::{MergeRequestHead, MergeRequestRepository, RepoError};
use foundry_domain::{DesignSpaceMergeRequestDAO, DomainEvent, TaskActorDAO, TaskDAO};
use time::OffsetDateTime;
use uuid::Uuid;

#[async_trait]
impl MergeRequestRepository for InMemoryStore {
    async fn insert_with_task(
        &self,
        merge_request: &DesignSpaceMergeRequestDAO,
        task: &TaskDAO,
        actors: &[TaskActorDAO],
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        {
            let mut tasks = self.tasks.write().expect("lock poisoned");
            tasks.insert(task, &[], events)?;
            tasks.insert_actors(actors)?;
        }
        let mut requests = self.merge_requests.write().expect("lock poisoned");
        if requests.contains_key(&merge_request.id) {
            return Err(RepoError::Duplicate(format!(
                "merge request {}",
                merge_request.id
            )));
        }
        requests.insert(merge_request.id, merge_request.clone());
        Ok(())
    }

    async fn insert(&self, merge_request: &DesignSpaceMergeRequestDAO) -> Result<(), RepoError> {
        let mut requests = self.merge_requests.write().expect("lock poisoned");
        if requests.contains_key(&merge_request.id)
            || requests
                .values()
                .any(|existing| existing.task_id == merge_request.task_id)
        {
            return Err(RepoError::Duplicate(format!(
                "merge request for task {}",
                merge_request.task_id
            )));
        }
        requests.insert(merge_request.id, merge_request.clone());
        Ok(())
    }

    async fn find_for_task(
        &self,
        project_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError> {
        Ok(self
            .merge_requests
            .read()
            .expect("lock poisoned")
            .values()
            .find(|request| request.project_id == project_id && request.task_id == task_id)
            .cloned())
    }

    async fn update_commits(
        &self,
        project_id: Uuid,
        id: Uuid,
        head: &MergeRequestHead,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError> {
        let mut requests = self.merge_requests.write().expect("lock poisoned");
        let Some(request) = requests
            .get_mut(&id)
            .filter(|request| request.project_id == project_id)
        else {
            return Ok(false);
        };
        if request.merge_commit.is_some() {
            return Ok(false);
        }
        request.source_branch = head.source_branch.clone();
        request.target_branch = head.target_branch.clone();
        request.source_commit = head.source_commit.clone();
        request.target_commit = head.target_commit.clone();
        request.updated_at = now;
        Ok(true)
    }

    async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError> {
        Ok(self
            .merge_requests
            .read()
            .expect("lock poisoned")
            .get(&id)
            .filter(|request| request.project_id == project_id)
            .cloned())
    }

    async fn list(&self, project_id: Uuid) -> Result<Vec<DesignSpaceMergeRequestDAO>, RepoError> {
        let mut requests: Vec<DesignSpaceMergeRequestDAO> = self
            .merge_requests
            .read()
            .expect("lock poisoned")
            .values()
            .filter(|request| request.project_id == project_id)
            .cloned()
            .collect();
        requests.sort_by_key(|request| std::cmp::Reverse(request.created_at));
        Ok(requests)
    }

    async fn save_merge_commit(
        &self,
        project_id: Uuid,
        id: Uuid,
        merge_commit: &str,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError> {
        let mut requests = self.merge_requests.write().expect("lock poisoned");
        let Some(request) = requests
            .get_mut(&id)
            .filter(|request| request.project_id == project_id)
        else {
            return Ok(false);
        };
        if request.merge_commit.is_some() {
            return Ok(false);
        }
        request.merge_commit = Some(merge_commit.to_string());
        request.updated_at = now;
        Ok(true)
    }

    async fn save_approval(
        &self,
        project_id: Uuid,
        id: Uuid,
        content_digest: &str,
        source_commit: &str,
        approved_by: Uuid,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError> {
        let mut requests = self.merge_requests.write().expect("lock poisoned");
        let Some(request) = requests
            .get_mut(&id)
            .filter(|request| request.project_id == project_id)
        else {
            return Ok(false);
        };
        if request.merge_commit.is_some() {
            return Ok(false);
        }
        request.approved_content_digest = Some(content_digest.to_string());
        request.approved_source_commit = Some(source_commit.to_string());
        request.approved_by = Some(approved_by);
        request.approved_at = Some(now);
        request.updated_at = now;
        Ok(true)
    }
}
