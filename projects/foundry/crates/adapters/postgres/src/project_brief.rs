//! Postgres storage for project brief drafts, versions and annotations.

use async_trait::async_trait;
use foundry_app::{
    ApproveBriefOutcome, DraftOperationOutcome, ProjectBriefRepository, RepoError,
    SubmitDraftOutcome,
};
use foundry_domain::{
    AnnotationStatus, ProjectBriefAnnotation, ProjectBriefAnnotationTarget, ProjectBriefBody,
    ProjectBriefDraftDAO, ProjectBriefDraftOperation, ProjectBriefStatus,
    ProjectBriefVersionDAO,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PostgresStore, backend, map_write_error};

type VersionRow = (
    Uuid,
    i32,
    String,
    Value,
    Uuid,
    Option<Uuid>,
    OffsetDateTime,
    OffsetDateTime,
);

type DraftRow = (
    Uuid,
    Uuid,
    i64,
    Value,
    Uuid,
    Uuid,
    OffsetDateTime,
    OffsetDateTime,
);

#[async_trait]
impl ProjectBriefRepository for PostgresStore {
    async fn current(
        &self,
        project_id: Uuid,
    ) -> Result<Option<ProjectBriefVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select b.project_id, b.version, b.status, b.body, b.created_by, b.approved_by, \
                    b.created_at, b.updated_at \
             from project_brief_current c \
             join project_briefs b \
               on b.project_id = c.project_id and b.version = c.version \
             where c.project_id = $1",
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn versions(
        &self,
        project_id: Uuid,
    ) -> Result<Vec<ProjectBriefVersionDAO>, RepoError> {
        let rows: Vec<VersionRow> = sqlx::query_as(
            "select project_id, version, status, body, created_by, approved_by, \
                    created_at, updated_at \
             from project_briefs where project_id = $1 order by version desc",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(version_from_row).collect()
    }

    async fn version(
        &self,
        project_id: Uuid,
        version: u32,
    ) -> Result<Option<ProjectBriefVersionDAO>, RepoError> {
        let row: Option<VersionRow> = sqlx::query_as(
            "select project_id, version, status, body, created_by, approved_by, \
                    created_at, updated_at \
             from project_briefs where project_id = $1 and version = $2",
        )
        .bind(project_id)
        .bind(version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_from_row).transpose()
    }

    async fn create_draft(
        &self,
        project_id: Uuid,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ProjectBriefDraftDAO, RepoError> {
        let body = serde_json::to_value(body).map_err(json_error)?;
        let row: DraftRow = sqlx::query_as(
            "insert into project_brief_drafts \
             (id, project_id, body, revision, created_by, updated_by, created_at, updated_at) \
             values ($1, $2, $3, 1, $4, $4, $5, $5) \
             returning id, project_id, revision, body, created_by, updated_by, \
                       created_at, updated_at",
        )
        .bind(Uuid::now_v7())
        .bind(project_id)
        .bind(body)
        .bind(actor_id)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(map_write_error)?;
        draft_from_row(row)
    }

    async fn drafts(
        &self,
        project_id: Uuid,
        created_by: Uuid,
    ) -> Result<Vec<ProjectBriefDraftDAO>, RepoError> {
        let rows: Vec<DraftRow> = sqlx::query_as(
            "select id, project_id, revision, body, created_by, updated_by, \
                    created_at, updated_at \
             from project_brief_drafts \
             where project_id = $1 and created_by = $2 \
             order by updated_at desc",
        )
        .bind(project_id)
        .bind(created_by)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(draft_from_row).collect()
    }

    async fn draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
    ) -> Result<Option<ProjectBriefDraftDAO>, RepoError> {
        let row: Option<DraftRow> = sqlx::query_as(
            "select id, project_id, revision, body, created_by, updated_by, \
                    created_at, updated_at \
             from project_brief_drafts where project_id = $1 and id = $2",
        )
        .bind(project_id)
        .bind(draft_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(draft_from_row).transpose()
    }

    async fn commit_draft_operation(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        op_id: Uuid,
        operation: &ProjectBriefDraftOperation,
        body: &ProjectBriefBody,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftOperationOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let row: Option<DraftRow> = sqlx::query_as(
            "select id, project_id, revision, body, created_by, updated_by, \
                    created_at, updated_at \
             from project_brief_drafts where project_id = $1 and id = $2 for update",
        )
        .bind(project_id)
        .bind(draft_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Err(RepoError::NotFound);
        };
        let draft = draft_from_row(row)?;
        let duplicate: Option<i32> = sqlx::query_scalar(
            "select 1 from project_brief_draft_operations \
             where draft_id = $1 and op_id = $2",
        )
        .bind(draft_id)
        .bind(op_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if duplicate.is_some() {
            tx.commit().await.map_err(backend)?;
            return Ok(DraftOperationOutcome::Duplicate(draft));
        }
        if draft.revision != expected_revision {
            tx.commit().await.map_err(backend)?;
            return Ok(DraftOperationOutcome::Conflict {
                current_revision: draft.revision,
            });
        }
        let body_value = serde_json::to_value(body).map_err(json_error)?;
        let updated: Option<DraftRow> = sqlx::query_as(
            "update project_brief_drafts \
             set body = $3, revision = revision + 1, updated_by = $4, updated_at = $5 \
             where project_id = $1 and id = $2 and revision = $6 \
             returning id, project_id, revision, body, created_by, updated_by, \
                       created_at, updated_at",
        )
        .bind(project_id)
        .bind(draft_id)
        .bind(body_value)
        .bind(actor_id)
        .bind(now)
        .bind(expected_revision as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(map_write_error)?;
        let Some(updated) = updated else {
            let current: i64 = sqlx::query_scalar(
                "select revision from project_brief_drafts where project_id = $1 and id = $2",
            )
            .bind(project_id)
            .bind(draft_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            return Ok(DraftOperationOutcome::Conflict {
                current_revision: current.max(0) as u64,
            });
        };
        let operation = serde_json::to_value(operation).map_err(json_error)?;
        sqlx::query(
            "insert into project_brief_draft_operations \
             (draft_id, op_id, base_revision, operation, actor_id, applied_at) \
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(draft_id)
        .bind(op_id)
        .bind(expected_revision as i64)
        .bind(operation)
        .bind(actor_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        let updated = draft_from_row(updated)?;
        tx.commit().await.map_err(backend)?;
        Ok(DraftOperationOutcome::Applied(updated))
    }

    async fn submit_draft(
        &self,
        project_id: Uuid,
        draft_id: Uuid,
        expected_revision: u64,
        actor_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<SubmitDraftOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let row: Option<DraftRow> = sqlx::query_as(
            "select id, project_id, revision, body, created_by, updated_by, \
                    created_at, updated_at \
             from project_brief_drafts where project_id = $1 and id = $2 for update",
        )
        .bind(project_id)
        .bind(draft_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Err(RepoError::NotFound);
        };
        let draft = draft_from_row(row)?;
        if draft.revision != expected_revision {
            tx.commit().await.map_err(backend)?;
            return Ok(SubmitDraftOutcome::Conflict {
                current_revision: draft.revision,
            });
        }
        let current: Option<i32> = sqlx::query_scalar(
            "select version from project_brief_current where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let next = current.map(|version| version + 1).unwrap_or(1);
        if let Some(current) = current {
            sqlx::query(
                "update project_briefs set status = 'superseded', updated_at = $3 \
                 where project_id = $1 and version = $2",
            )
            .bind(project_id)
            .bind(current)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(map_write_error)?;
        }
        let body = serde_json::to_value(&draft.body).map_err(json_error)?;
        let version: VersionRow = sqlx::query_as(
            "insert into project_briefs \
             (project_id, version, status, body, created_by, approved_by, created_at, updated_at) \
             values ($1, $2, 'in_review', $3, $4, null, $5, $5) \
             returning project_id, version, status, body, created_by, approved_by, \
                       created_at, updated_at",
        )
        .bind(project_id)
        .bind(next)
        .bind(body)
        .bind(actor_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_write_error)?;
        sqlx::query(
            "insert into project_brief_current (project_id, version, updated_at) \
             values ($1, $2, $3) \
             on conflict (project_id) do update set \
               version = excluded.version, updated_at = excluded.updated_at",
        )
        .bind(project_id)
        .bind(next)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        sqlx::query("delete from project_brief_drafts where project_id = $1 and id = $2")
            .bind(project_id)
            .bind(draft_id)
            .execute(&mut *tx)
            .await
            .map_err(map_write_error)?;
        let version = version_from_row(version)?;
        tx.commit().await.map_err(backend)?;
        Ok(SubmitDraftOutcome::Submitted(Box::new(version)))
    }

    async fn approve_current(
        &self,
        project_id: Uuid,
        expected_version: u32,
        approver_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<ApproveBriefOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let current: Option<i32> = sqlx::query_scalar(
            "select version from project_brief_current where project_id = $1 for update",
        )
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(current) = current else {
            return Err(RepoError::NotFound);
        };
        if current.max(0) as u32 != expected_version {
            tx.commit().await.map_err(backend)?;
            return Ok(ApproveBriefOutcome::Conflict {
                current_version: current.max(0) as u32,
            });
        }
        let status: Option<String> = sqlx::query_scalar(
            "select status from project_briefs where project_id = $1 and version = $2 for update",
        )
        .bind(project_id)
        .bind(current)
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(status) = status else {
            return Err(RepoError::NotFound);
        };
        if status != "in_review" {
            tx.commit().await.map_err(backend)?;
            return Ok(ApproveBriefOutcome::NotInReview);
        }
        let row: VersionRow = sqlx::query_as(
            "update project_briefs \
             set status = 'approved', approved_by = $3, updated_at = $4 \
             where project_id = $1 and version = $2 \
             returning project_id, version, status, body, created_by, approved_by, \
                       created_at, updated_at",
        )
        .bind(project_id)
        .bind(current)
        .bind(approver_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_write_error)?;
        sqlx::query(
            "update project_brief_current set updated_at = $2 where project_id = $1",
        )
        .bind(project_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        let version = version_from_row(row)?;
        tx.commit().await.map_err(backend)?;
        Ok(ApproveBriefOutcome::Approved(Box::new(version)))
    }

    async fn annotations(
        &self,
        project_id: Uuid,
        include_resolved: bool,
    ) -> Result<Vec<ProjectBriefAnnotation>, RepoError> {
        let rows: Vec<BriefAnnotationRow> = sqlx::query_as(
            "select a.id, a.project_id, a.target_type, a.target_id, a.field, a.body, \
                    a.author_id, acc.handle, acc.display_name, a.status, a.resolution, \
                    a.created_at, a.resolved_at, a.resolved_by \
             from project_brief_annotations a \
             join accounts acc on acc.id = a.author_id \
             where a.project_id = $1 and ($2 or a.status = 'open') \
             order by a.created_at, a.id",
        )
        .bind(project_id)
        .bind(include_resolved)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(annotation_from_row).collect()
    }

    async fn annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError> {
        let row: Option<BriefAnnotationRow> = sqlx::query_as(
            "select a.id, a.project_id, a.target_type, a.target_id, a.field, a.body, \
                    a.author_id, acc.handle, acc.display_name, a.status, a.resolution, \
                    a.created_at, a.resolved_at, a.resolved_by \
             from project_brief_annotations a \
             join accounts acc on acc.id = a.author_id \
             where a.project_id = $1 and a.id = $2",
        )
        .bind(project_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(annotation_from_row).transpose()
    }

    async fn insert_annotation(
        &self,
        annotation: &ProjectBriefAnnotation,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "insert into project_brief_annotations \
             (id, project_id, target_type, target_id, field, body, author_id, status, created_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(annotation.id)
        .bind(annotation.project_id)
        .bind(annotation.target_type.as_str())
        .bind(&annotation.target_id)
        .bind(&annotation.field)
        .bind(&annotation.body)
        .bind(annotation.author_id)
        .bind(annotation.status.as_str())
        .bind(annotation.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn resolve_annotation(
        &self,
        project_id: Uuid,
        id: Uuid,
        resolver_id: Uuid,
        resolution: &str,
        now: OffsetDateTime,
    ) -> Result<Option<ProjectBriefAnnotation>, RepoError> {
        let updated: Option<(Uuid,)> = sqlx::query_as(
            "update project_brief_annotations \
             set status = 'resolved', resolution = $3, resolved_at = $4, resolved_by = $2 \
             where project_id = $1 and id = $5 and status = 'open' \
             returning id",
        )
        .bind(project_id)
        .bind(resolver_id)
        .bind(resolution)
        .bind(now)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        if updated.is_none() {
            return Ok(None);
        }
        self.annotation(project_id, id).await
    }
}

fn json_error(error: serde_json::Error) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn version_from_row(row: VersionRow) -> Result<ProjectBriefVersionDAO, RepoError> {
    Ok(ProjectBriefVersionDAO {
        project_id: row.0,
        version: row.1.max(0) as u32,
        status: ProjectBriefStatus::parse(&row.2),
        body: serde_json::from_value(row.3).unwrap_or_default(),
        created_by: row.4,
        approved_by: row.5,
        created_at: row.6,
        updated_at: row.7,
    })
}

fn draft_from_row(row: DraftRow) -> Result<ProjectBriefDraftDAO, RepoError> {
    Ok(ProjectBriefDraftDAO {
        id: row.0,
        project_id: row.1,
        revision: row.2.max(0) as u64,
        body: serde_json::from_value(row.3).unwrap_or_default(),
        created_by: row.4,
        updated_by: row.5,
        created_at: row.6,
        updated_at: row.7,
    })
}

type BriefAnnotationRow = (
    Uuid,
    Uuid,
    String,
    Option<String>,
    Option<String>,
    String,
    Uuid,
    String,
    String,
    String,
    Option<String>,
    OffsetDateTime,
    Option<OffsetDateTime>,
    Option<Uuid>,
);

fn annotation_from_row(row: BriefAnnotationRow) -> Result<ProjectBriefAnnotation, RepoError> {
    let target_type = ProjectBriefAnnotationTarget::parse(&row.2)
        .ok_or_else(|| RepoError::Backend(format!("unknown annotation target {}", row.2)))?;
    Ok(ProjectBriefAnnotation {
        id: row.0,
        project_id: row.1,
        target_type,
        target_id: row.3,
        field: row.4,
        body: row.5,
        author_id: row.6,
        author_handle: row.7,
        author_name: row.8,
        status: AnnotationStatus::parse(&row.9),
        resolution: row.10,
        created_at: row.11,
        resolved_at: row.12,
        resolved_by: row.13,
    })
}
