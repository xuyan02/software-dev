//! Scans for approved work that the control plane must accept.

use foundry_app::RepoError;
use uuid::Uuid;

use crate::{PostgresStore, backend};

impl PostgresStore {
    /// Goal-clarification tasks whose project document has an approved revision
    /// and the task has not been completed yet.
    pub async fn approved_goal_clarifications(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id, a.revision \
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
                    case when t.workflow = 'goal_clarification' then 'project' \
                         when t.workflow = 'architecture_design' then 'architecture' \
                         when t.workflow = 'detailed_design' then 'design' \
                         when t.workflow = 'iteration' then 'development' \
                         when t.workflow = 'test_planning' then 'test_plan' \
                         when t.workflow = 'test_execution' then 'test_report' \
                         else 'requirements' end, \
                    d.revision, t.id, now() \
             from tasks t \
             join anchor_documents doc \
               on doc.project_id = t.project_id \
              and doc.kind = case when t.workflow = 'goal_clarification' then 'project' \
                                 when t.workflow = 'architecture_design' then 'architecture' \
                                 when t.workflow = 'detailed_design' then 'design' \
                                 when t.workflow = 'iteration' then 'development' \
                                 when t.workflow = 'test_planning' then 'test_plan' \
                                 when t.workflow = 'test_execution' then 'test_report' \
                                 else 'requirements' end \
             join anchor_document_drafts d on d.document_id = doc.id \
             where t.workflow in ('goal_clarification', 'requirements_analysis', 'interaction_design', 'ui_design', 'architecture_design', 'detailed_design', 'iteration', 'test_planning', 'test_execution') \
               and t.state = 'in_progress' \
               and (t.stage = 'human_review' or (t.workflow = 'iteration' and t.stage = 'acceptance')) \
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
                     and (t.stage = 'human_review' or (t.workflow = 'iteration' and t.stage = 'acceptance')) \
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
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id, a.revision \
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

    /// Architecture-design tasks whose architecture anchor document has an
    /// approved revision newer than the task's last update.
    pub async fn approved_architecture_design_tasks(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id, a.revision \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id and a.kind = 'architecture' \
             where t.workflow = 'architecture_design' \
               and t.state = 'in_progress' \
               and t.stage = 'human_review' \
               and a.decision = 'approved' \
               and a.created_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Document-review tasks for a set of workflows whose document has an
    /// approved revision newer than the task's last update.
    pub async fn approved_document_tasks(
        &self,
        kind: &str,
        workflows: &[&str],
        stage: &str,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64)>, RepoError> {
        let workflows: Vec<String> = workflows
            .iter()
            .map(|workflow| workflow.to_string())
            .collect();
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id, a.revision \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id and a.kind = $1 \
             where t.workflow = any($2) \
               and t.state = 'in_progress' \
               and t.stage = $3 \
               and a.decision = 'approved' \
               and a.created_at > t.updated_at",
        )
        .bind(kind)
        .bind(workflows)
        .bind(stage)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Human-review tasks whose document revision was rejected. If the web
    /// request recorded the rejection but failed to send the task operation,
    /// the control plane retries it here.
    pub async fn rejected_document_tasks(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, Uuid, i64, String)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, a.actor_id, a.revision, t.stage \
             from tasks t \
             join anchor_document_approvals a \
               on a.project_id = t.project_id \
              and a.kind = case when t.workflow = 'goal_clarification' then 'project' \
                                 when t.workflow = 'architecture_design' then 'architecture' \
                                 when t.workflow = 'detailed_design' then 'design' \
                                 when t.workflow = 'iteration' then 'development' \
                                 when t.workflow = 'test_planning' then 'test_plan' \
                                 when t.workflow = 'test_execution' then 'test_report' \
                                 else 'requirements' end \
             where t.workflow in ('goal_clarification', 'requirements_analysis', 'interaction_design', 'ui_design', 'architecture_design', 'detailed_design', 'iteration', 'test_planning', 'test_execution') \
               and t.state = 'in_progress' \
               and (t.stage = 'human_review' or (t.workflow = 'iteration' and t.stage = 'acceptance')) \
               and a.decision = 'rejected' \
               and a.created_at > t.updated_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }
}

impl PostgresStore {
    /// Document reviews that a skipped-approval project should approve
    /// automatically. Goal clarification is deliberately excluded: a human
    /// must take part in the interview and approve its result.
    pub async fn pending_auto_document_approvals(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, String, i64, String)>, RepoError> {
        sqlx::query_as(
            "select l.task_id, l.project_id, l.kind, l.draft_revision, owner.handle \
             from anchor_document_review_links l \
             join tasks t on t.id = l.task_id \
             join projects p on p.id = l.project_id \
             join lateral ( \
                 select a.handle \
                 from project_members pm \
                 join accounts a on a.id = pm.account_id \
                 where pm.project_id = l.project_id \
                   and pm.role = 'owner' \
                   and a.kind = 'human' \
                   and a.status = 'active' \
                 order by pm.added_at, pm.account_id \
                 limit 1 \
             ) owner on true \
             where l.released_at is null \
               and p.skip_human_review \
               and t.state = 'in_progress' \
               and (t.stage = 'human_review' or (t.workflow = 'iteration' and t.stage = 'acceptance')) \
               and t.workflow <> 'goal_clarification' \
               and not exists ( \
                   select 1 from anchor_document_approvals a \
                   where a.project_id = l.project_id \
                     and a.kind = l.kind \
                     and a.decision = 'approved' \
                     and a.revision >= l.draft_revision \
               )",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }

    /// Pending design-space merge requests whose project skips approvals.
    /// Returns the owner account that will stand in as the reviewer.
    pub async fn pending_auto_design_merges(&self) -> Result<Vec<(Uuid, Uuid, Uuid)>, RepoError> {
        sqlx::query_as(
            "select mr.project_id, mr.id, owner.account_id \
             from design_space_merge_requests mr \
             join tasks t on t.id = mr.task_id \
             join projects p on p.id = mr.project_id \
             join lateral ( \
                 select pm.account_id \
                 from project_members pm \
                 join accounts a on a.id = pm.account_id \
                 where pm.project_id = mr.project_id \
                   and pm.role = 'owner' \
                   and a.kind = 'human' \
                   and a.status = 'active' \
                 order by pm.added_at, pm.account_id \
                 limit 1 \
             ) owner on true \
             where p.skip_human_review \
               and t.state = 'in_progress' \
               and t.stage in ('review', 'merge', 'human_review')",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)
    }
}
