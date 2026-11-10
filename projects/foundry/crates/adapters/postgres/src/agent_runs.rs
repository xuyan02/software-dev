//! Scans for approved work that the control plane must accept.

use foundry_app::RepoError;
use uuid::Uuid;

use crate::{PostgresStore, backend};

impl PostgresStore {
    /// Project-brief-writing tasks whose current brief version has been
    /// approved but the task has not been completed yet.
    pub async fn approved_project_briefs(&self) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, b.approved_by \
             from tasks t \
             join project_brief_current c on c.project_id = t.project_id \
             join project_briefs b \
               on b.project_id = c.project_id and b.version = c.version \
             where t.workflow = 'project_brief_writing' \
               and t.state = 'in_progress' \
               and t.stage = 'acceptance' \
               and b.status = 'approved' \
               and b.approved_by is not null",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Requirements-analysis tasks whose requirements document has a published
    /// version newer than the task's last update.
    pub async fn approved_requirements_analyses(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, v.created_by \
             from tasks t \
             join requirements_documents d on d.project_id = t.project_id \
             join requirements_document_versions v \
               on v.document_id = d.id and v.version = d.current_version \
             where t.workflow = 'requirements_analysis' \
               and t.state = 'in_progress' \
               and t.stage = 'human_review' \
               and v.published_at is not null \
               and v.published_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }
}
