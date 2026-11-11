//! Scans for approved work that the control plane must accept.

use foundry_app::RepoError;
use uuid::Uuid;

use crate::{PostgresStore, backend};

impl PostgresStore {
    /// Goal-clarification tasks whose project document has an approved revision
    /// and the task has not been completed yet.
    pub async fn approved_goal_clarifications(&self) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id and a.kind = 'project' \
             where t.workflow = 'goal_clarification' \
               and t.state = 'in_progress' \
               and t.stage = 'awaiting_acceptance' \
               and a.decision = 'approved' \
               and a.created_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Requirements-analysis tasks whose requirements anchor document has an
    /// approved revision newer than the task's last update.
    pub async fn approved_requirements_analyses(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id and a.kind = 'requirements' \
             where t.workflow = 'requirements_analysis' \
               and t.state = 'in_progress' \
               and t.stage = 'human_review' \
               and a.decision = 'approved' \
               and a.created_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }
}
