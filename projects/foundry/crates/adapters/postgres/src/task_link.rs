//! Postgres storage for task links.

use async_trait::async_trait;
use foundry_app::{RepoError, TaskLinkRepository};
use foundry_domain::{Actor, TaskLink, TaskLinkKind, TaskLinkRelation};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::PostgresStore;

pub(crate) async fn insert_task_link_row(
    tx: &mut Transaction<'_, Postgres>,
    link: &TaskLink,
) -> Result<(), RepoError> {
    sqlx::query(
        "insert into task_links \
         (id, task_id, kind, target_id, url, title, relation, created_by, created_at, metadata) \
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(link.id)
    .bind(link.task_id)
    .bind(link.kind.as_str())
    .bind(link.target_id)
    .bind(link.url.clone())
    .bind(link.title.clone())
    .bind(link.relation.as_str())
    .bind(serde_json::to_value(&link.created_by).map_err(backend)?)
    .bind(link.created_at)
    .bind(link.metadata.clone())
    .execute(&mut **tx)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            RepoError::Duplicate(format!("task link on {}", link.task_id))
        }
        other => backend(other),
    })?;
    Ok(())
}

type TaskLinkRow = (
    Uuid,
    Uuid,
    String,
    Option<Uuid>,
    Option<String>,
    String,
    String,
    Value,
    OffsetDateTime,
    Value,
);

#[async_trait]
impl TaskLinkRepository for PostgresStore {
    async fn list(&self, task_id: Uuid) -> Result<Vec<TaskLink>, RepoError> {
        let rows: Vec<TaskLinkRow> = sqlx::query_as(
            "select id, task_id, kind, target_id, url, title, relation, created_by, created_at, metadata \
             from task_links where task_id = $1 order by created_at, id",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(link_from_row).collect()
    }

    async fn get(&self, task_id: Uuid, link_id: Uuid) -> Result<Option<TaskLink>, RepoError> {
        let row: Option<TaskLinkRow> = sqlx::query_as(
            "select id, task_id, kind, target_id, url, title, relation, created_by, created_at, metadata \
             from task_links where task_id = $1 and id = $2",
        )
        .bind(task_id)
        .bind(link_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(link_from_row).transpose()
    }

    async fn insert(&self, link: &TaskLink) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        insert_task_link_row(&mut tx, link).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn delete(&self, task_id: Uuid, link_id: Uuid) -> Result<(), RepoError> {
        let affected = sqlx::query("delete from task_links where task_id = $1 and id = $2")
            .bind(task_id)
            .bind(link_id)
            .execute(&self.pool)
            .await
            .map_err(backend)?
            .rows_affected();
        if affected == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}

fn link_from_row(row: TaskLinkRow) -> Result<TaskLink, RepoError> {
    Ok(TaskLink {
        id: row.0,
        task_id: row.1,
        kind: TaskLinkKind::parse(&row.2)
            .ok_or_else(|| RepoError::Backend(format!("unknown task link kind {}", row.2)))?,
        target_id: row.3,
        url: row.4,
        title: row.5,
        relation: TaskLinkRelation::parse(&row.6)
            .ok_or_else(|| RepoError::Backend(format!("unknown task link relation {}", row.6)))?,
        created_by: serde_json::from_value::<Actor>(row.7).map_err(backend)?,
        created_at: row.8,
        metadata: row.9,
    })
}

fn backend(error: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(error.to_string())
}
