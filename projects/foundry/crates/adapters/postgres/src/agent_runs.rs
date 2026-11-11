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
               and t.stage = 'human_review' \
               and a.decision = 'approved' \
               and a.created_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Bind human-review tasks to their current document draft. Idempotent:
    /// unchanged links are left alone.
    pub async fn bind_human_review_document_links(&self) -> Result<u64, RepoError> {
        let result = sqlx::query(
            "insert into anchor_document_review_links \
             (id, project_id, kind, draft_revision, task_id, bound_at) \
             select gen_random_uuid(), t.project_id, \
                    case when t.workflow = 'goal_clarification' then 'project' else 'requirements' end, \
                    d.revision, t.id, now() \
             from tasks t \
             join anchor_documents doc \
               on doc.project_id = t.project_id \
              and doc.kind = case when t.workflow = 'goal_clarification' then 'project' else 'requirements' end \
             join anchor_document_drafts d on d.document_id = doc.id \
             where t.workflow in ('goal_clarification', 'requirements_analysis', 'interaction_design', 'ui_design') \
               and t.state = 'in_progress' \
               and t.stage = 'human_review' \
             on conflict (project_id, kind) where released_at is null \
             do update set task_id = excluded.task_id, \
                           draft_revision = excluded.draft_revision, \
                           bound_at = excluded.bound_at, \
                           released_at = null \
             where anchor_document_review_links.task_id is distinct from excluded.task_id \
                or anchor_document_review_links.draft_revision is distinct from excluded.draft_revision",
        )
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected())
    }

    /// Release links whose task has left human review or is no longer active.
    pub async fn release_stale_document_review_links(&self) -> Result<u64, RepoError> {
        let result = sqlx::query(
            "update anchor_document_review_links l set released_at = now() \
             where l.released_at is null \
               and not exists ( \
                   select 1 from tasks t \
                   where t.id = l.task_id \
                     and t.state = 'in_progress' \
                     and t.stage = 'human_review' \
               )",
        )
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected())
    }

    /// Requirement-design tasks whose requirements anchor document has an
    /// approved revision newer than the task's last update.
    pub async fn approved_requirement_document_tasks(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id and a.kind = 'requirements' \
             where t.workflow in ('requirements_analysis', 'interaction_design', 'ui_design') \
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
