//! In-memory project triggers.

use async_trait::async_trait;
use foundry_app::{RepoError, TaskRepository, TriggerRepository};
use foundry_domain::{TaskState, Trigger, TriggerRun};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::InMemoryStore;

#[async_trait]
impl TriggerRepository for InMemoryStore {
    async fn insert(&self, trigger: &Trigger) -> Result<(), RepoError> {
        let mut triggers = self.triggers.write().expect("lock poisoned");
        if triggers.iter().any(|existing| existing.id == trigger.id) {
            return Err(RepoError::Duplicate("trigger".to_string()));
        }
        triggers.push(trigger.clone());
        Ok(())
    }

    async fn save(&self, trigger: &Trigger, expected_version: u32) -> Result<(), RepoError> {
        let mut triggers = self.triggers.write().expect("lock poisoned");
        let stored = triggers
            .iter_mut()
            .find(|stored| stored.id == trigger.id)
            .ok_or(RepoError::NotFound)?;
        if stored.version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: stored.version,
            });
        }
        *stored = trigger.clone();
        Ok(())
    }

    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Option<Trigger>, RepoError> {
        Ok(self
            .triggers
            .read()
            .expect("lock poisoned")
            .iter()
            .find(|trigger| trigger.project_id == project_id && trigger.id == id)
            .cloned())
    }

    async fn list(&self, project_id: Uuid) -> Result<Vec<Trigger>, RepoError> {
        let mut triggers = self
            .triggers
            .read()
            .expect("lock poisoned")
            .iter()
            .filter(|trigger| trigger.project_id == project_id)
            .cloned()
            .collect::<Vec<_>>();
        triggers.sort_by_key(|trigger| (trigger.created_at, trigger.id));
        Ok(triggers)
    }

    async fn delete(
        &self,
        project_id: Uuid,
        id: Uuid,
        expected_version: u32,
    ) -> Result<(), RepoError> {
        let mut triggers = self.triggers.write().expect("lock poisoned");
        let Some(position) = triggers
            .iter()
            .position(|trigger| trigger.project_id == project_id && trigger.id == id)
        else {
            return Err(RepoError::NotFound);
        };
        if triggers[position].version != expected_version {
            return Err(RepoError::Conflict {
                expected: expected_version,
                actual: triggers[position].version,
            });
        }
        triggers.remove(position);
        Ok(())
    }

    async fn list_due(&self, now: OffsetDateTime) -> Result<Vec<Trigger>, RepoError> {
        let mut due = self
            .triggers
            .read()
            .expect("lock poisoned")
            .iter()
            .filter(|trigger| {
                trigger.enabled
                    && trigger.schedule != "manual"
                    && trigger.next_run_at.is_some_and(|next| next <= now)
            })
            .cloned()
            .collect::<Vec<_>>();
        due.sort_by_key(|trigger| (trigger.next_run_at, trigger.id));
        Ok(due)
    }

    async fn insert_run(&self, run: &TriggerRun) -> Result<(), RepoError> {
        self.trigger_runs
            .write()
            .expect("lock poisoned")
            .push(run.clone());
        Ok(())
    }

    async fn runs(
        &self,
        project_id: Uuid,
        trigger_id: Uuid,
    ) -> Result<Vec<TriggerRun>, RepoError> {
        let belongs = self
            .triggers
            .read()
            .expect("lock poisoned")
            .iter()
            .any(|trigger| trigger.id == trigger_id && trigger.project_id == project_id);
        if !belongs {
            return Ok(Vec::new());
        }
        let mut runs = self
            .trigger_runs
            .read()
            .expect("lock poisoned")
            .iter()
            .filter(|run| run.trigger_id == trigger_id)
            .cloned()
            .collect::<Vec<_>>();
        runs.sort_by_key(|run| std::cmp::Reverse((run.created_at, run.id)));
        Ok(runs)
    }

    async fn has_active_run(&self, trigger_id: Uuid) -> Result<bool, RepoError> {
        let task_ids = self
            .trigger_runs
            .read()
            .expect("lock poisoned")
            .iter()
            .filter(|run| run.trigger_id == trigger_id)
            .map(|run| run.task_id)
            .collect::<Vec<_>>();
        for task_id in task_ids {
            if let Some(task) = TaskRepository::get(self, task_id).await?
                && task.state != TaskState::Completed
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
