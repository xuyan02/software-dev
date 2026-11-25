//! Scans for approved work that the control plane must accept.

use foundry_app::RepoError;
use uuid::Uuid;

use crate::{PostgresStore, backend};

impl PostgresStore {
    /// Design-space-writing tasks waiting for human approval without an MR.
    /// Returns `(task_id, project_id, workflow, acceptor_id)`.
    pub async fn design_space_human_review_tasks(
        &self,
    ) -> Result<Vec<(Uuid, Uuid, String, Uuid)>, RepoError> {
        sqlx::query_as(
            "select t.id, t.project_id, t.workflow, \
                    (array_agg(a.account_id order by a.account_id))[1] \
             from tasks t \
             join task_actors a on a.task_id = t.id and a.active and a.role_key = 'acceptor' \
             where t.state = 'in_progress' \
               and t.stage = 'human_review' \
               and t.workflow in ('goal_clarification', 'requirements_analysis', 'design', \
                                  'design_change', 'design_correction', 'design_baseline') \
             group by t.id, t.project_id, t.workflow",
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
