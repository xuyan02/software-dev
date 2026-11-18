//! Postgres persistence for design-space merge requests.

use crate::{PostgresStore, backend, map_write_error};
use async_trait::async_trait;
use foundry_app::{MergeRequestHead, MergeRequestRepository, RepoError};
use foundry_domain::{DesignSpaceMergeRequestDAO, DomainEvent, TaskActorDAO, TaskDAO};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct MergeRequestRow {
    id: Uuid,
    project_id: Uuid,
    title: String,
    description: String,
    source_branch: String,
    target_branch: String,
    source_commit: String,
    target_commit: String,
    task_id: Uuid,
    merge_commit: Option<String>,
    approved_content_digest: Option<String>,
    approved_source_commit: Option<String>,
    approved_by: Option<Uuid>,
    approved_at: Option<OffsetDateTime>,
    created_by: Uuid,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

fn row_to_merge_request(row: MergeRequestRow) -> DesignSpaceMergeRequestDAO {
    DesignSpaceMergeRequestDAO {
        id: row.id,
        project_id: row.project_id,
        title: row.title,
        description: row.description,
        source_branch: row.source_branch,
        target_branch: row.target_branch,
        source_commit: row.source_commit,
        target_commit: row.target_commit,
        task_id: row.task_id,
        merge_commit: row.merge_commit,
        approved_content_digest: row.approved_content_digest,
        approved_source_commit: row.approved_source_commit,
        approved_by: row.approved_by,
        approved_at: row.approved_at,
        created_by: row.created_by,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

#[async_trait]
impl MergeRequestRepository for PostgresStore {
    async fn insert_with_task(
        &self,
        merge_request: &DesignSpaceMergeRequestDAO,
        task: &TaskDAO,
        actors: &[TaskActorDAO],
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        let mut tx = self.pool().begin().await.map_err(backend)?;
        crate::task::insert_task(&mut tx, task).await?;
        for actor in actors {
            sqlx::query(
                "insert into task_actors (task_id, role_key, account_id, active, work) \
                 values ($1, $2, $3, $4, $5)",
            )
            .bind(actor.task_id)
            .bind(actor.role_key.as_str())
            .bind(actor.account_id)
            .bind(actor.active)
            .bind(&actor.work)
            .execute(&mut *tx)
            .await
            .map_err(map_write_error)?;
        }
        sqlx::query(
            "insert into design_space_merge_requests \
             (id, project_id, title, description, source_branch, target_branch, \
              source_commit, target_commit, task_id, merge_commit, \
              approved_content_digest, approved_source_commit, approved_by, approved_at, \
              created_by, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(merge_request.id)
        .bind(merge_request.project_id)
        .bind(&merge_request.title)
        .bind(&merge_request.description)
        .bind(&merge_request.source_branch)
        .bind(&merge_request.target_branch)
        .bind(&merge_request.source_commit)
        .bind(&merge_request.target_commit)
        .bind(merge_request.task_id)
        .bind(&merge_request.merge_commit)
        .bind(&merge_request.approved_content_digest)
        .bind(&merge_request.approved_source_commit)
        .bind(merge_request.approved_by)
        .bind(merge_request.approved_at)
        .bind(merge_request.created_by)
        .bind(merge_request.created_at)
        .bind(merge_request.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        crate::task::append_events(&mut tx, merge_request.project_id, events).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn insert(&self, merge_request: &DesignSpaceMergeRequestDAO) -> Result<(), RepoError> {
        sqlx::query(
            "insert into design_space_merge_requests \
             (id, project_id, title, description, source_branch, target_branch, \
              source_commit, target_commit, task_id, merge_commit, \
              approved_content_digest, approved_source_commit, approved_by, approved_at, \
              created_by, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(merge_request.id)
        .bind(merge_request.project_id)
        .bind(&merge_request.title)
        .bind(&merge_request.description)
        .bind(&merge_request.source_branch)
        .bind(&merge_request.target_branch)
        .bind(&merge_request.source_commit)
        .bind(&merge_request.target_commit)
        .bind(merge_request.task_id)
        .bind(&merge_request.merge_commit)
        .bind(&merge_request.approved_content_digest)
        .bind(&merge_request.approved_source_commit)
        .bind(merge_request.approved_by)
        .bind(merge_request.approved_at)
        .bind(merge_request.created_by)
        .bind(merge_request.created_at)
        .bind(merge_request.updated_at)
        .execute(self.pool())
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn find_for_task(
        &self,
        project_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError> {
        let row: Option<MergeRequestRow> = sqlx::query_as(
            "select id, project_id, title, description, source_branch, target_branch, \
             source_commit, target_commit, task_id, merge_commit, \
              approved_content_digest, approved_source_commit, approved_by, approved_at, \
              created_by, created_at, updated_at \
             from design_space_merge_requests where project_id = $1 and task_id = $2",
        )
        .bind(project_id)
        .bind(task_id)
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        Ok(row.map(row_to_merge_request))
    }

    async fn update_commits(
        &self,
        project_id: Uuid,
        id: Uuid,
        head: &MergeRequestHead,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError> {
        let result = sqlx::query(
            "update design_space_merge_requests set source_branch = $3, target_branch = $4, \
             source_commit = $5, target_commit = $6, updated_at = $7 \
             where project_id = $1 and id = $2 and merge_commit is null",
        )
        .bind(project_id)
        .bind(id)
        .bind(&head.source_branch)
        .bind(&head.target_branch)
        .bind(&head.source_commit)
        .bind(&head.target_commit)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(map_write_error)?;
        Ok(result.rows_affected() == 1)
    }

    async fn get(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<DesignSpaceMergeRequestDAO>, RepoError> {
        let row: Option<MergeRequestRow> = sqlx::query_as(
            "select id, project_id, title, description, source_branch, target_branch, \
             source_commit, target_commit, task_id, merge_commit, \
              approved_content_digest, approved_source_commit, approved_by, approved_at, \
              created_by, created_at, updated_at \
             from design_space_merge_requests where project_id = $1 and id = $2",
        )
        .bind(project_id)
        .bind(id)
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        Ok(row.map(row_to_merge_request))
    }

    async fn list(&self, project_id: Uuid) -> Result<Vec<DesignSpaceMergeRequestDAO>, RepoError> {
        let rows: Vec<MergeRequestRow> = sqlx::query_as(
            "select id, project_id, title, description, source_branch, target_branch, \
             source_commit, target_commit, task_id, merge_commit, \
              approved_content_digest, approved_source_commit, approved_by, approved_at, \
              created_by, created_at, updated_at \
             from design_space_merge_requests where project_id = $1 \
             order by created_at desc",
        )
        .bind(project_id)
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(rows.into_iter().map(row_to_merge_request).collect())
    }

    async fn save_merge_commit(
        &self,
        project_id: Uuid,
        id: Uuid,
        merge_commit: &str,
        now: OffsetDateTime,
    ) -> Result<bool, RepoError> {
        let result = sqlx::query(
            "update design_space_merge_requests set merge_commit = $3, updated_at = $4 \
             where project_id = $1 and id = $2 and merge_commit is null",
        )
        .bind(project_id)
        .bind(id)
        .bind(merge_commit)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(map_write_error)?;
        Ok(result.rows_affected() == 1)
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
        let result = sqlx::query(
            "update design_space_merge_requests \
             set approved_content_digest = $3, approved_source_commit = $4, \
                 approved_by = $5, approved_at = $6, updated_at = $6 \
             where project_id = $1 and id = $2 and merge_commit is null",
        )
        .bind(project_id)
        .bind(id)
        .bind(content_digest)
        .bind(source_commit)
        .bind(approved_by)
        .bind(now)
        .execute(self.pool())
        .await
        .map_err(map_write_error)?;
        Ok(result.rows_affected() == 1)
    }
}
