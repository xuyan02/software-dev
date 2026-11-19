//! Postgres storage for project triggers and trigger runs.

use async_trait::async_trait;
use foundry_app::{RepoError, TriggerRepository};
use foundry_domain::{Actor, Trigger, TriggerRun, TriggerRunReason};
use sqlx::FromRow;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::PostgresStore;

#[derive(Debug, FromRow)]
struct TriggerRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
    description: String,
    workflow_id: String,
    input: serde_json::Value,
    schedule: String,
    enabled: bool,
    title_template: String,
    description_template: String,
    created_by: serde_json::Value,
    created_by_id: Uuid,
    next_run_at: Option<OffsetDateTime>,
    last_run_at: Option<OffsetDateTime>,
    version: i32,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

#[derive(Debug, FromRow)]
struct RunRow {
    id: Uuid,
    trigger_id: Uuid,
    task_id: Uuid,
    reason: String,
    triggered_by: serde_json::Value,
    created_at: OffsetDateTime,
}

#[async_trait]
impl TriggerRepository for PostgresStore {
    async fn insert(&self, trigger: &Trigger) -> Result<(), RepoError> {
        sqlx::query(
            "insert into triggers \
             (id, project_id, name, description, workflow_id, input, schedule, enabled, \
              title_template, description_template, created_by, created_by_id, next_run_at, \
              last_run_at, version, created_at, updated_at) \
             values ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)",
        )
        .bind(trigger.id)
        .bind(trigger.project_id)
        .bind(&trigger.name)
        .bind(&trigger.description)
        .bind(&trigger.workflow_id)
        .bind(&trigger.input)
        .bind(&trigger.schedule)
        .bind(trigger.enabled)
        .bind(&trigger.title_template)
        .bind(&trigger.description_template)
        .bind(serde_json::to_value(&trigger.created_by).map_err(backend)?)
        .bind(trigger.created_by_id)
        .bind(trigger.next_run_at)
        .bind(trigger.last_run_at)
        .bind(trigger.version as i32)
        .bind(trigger.created_at)
        .bind(trigger.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn save(&self, trigger: &Trigger, expected_version: u32) -> Result<(), RepoError> {
        let result = sqlx::query(
            "update triggers set name=$3, description=$4, input=$5, schedule=$6, enabled=$7, \
             title_template=$8, description_template=$9, next_run_at=$10, last_run_at=$11, \
             version=$12, updated_at=$13 \
             where id=$1 and version=$2",
        )
        .bind(trigger.id)
        .bind(expected_version as i32)
        .bind(&trigger.name)
        .bind(&trigger.description)
        .bind(&trigger.input)
        .bind(&trigger.schedule)
        .bind(trigger.enabled)
        .bind(&trigger.title_template)
        .bind(&trigger.description_template)
        .bind(trigger.next_run_at)
        .bind(trigger.last_run_at)
        .bind(trigger.version as i32)
        .bind(trigger.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Backend("trigger version conflict".to_string()));
        }
        Ok(())
    }

    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Option<Trigger>, RepoError> {
        let row: Option<TriggerRow> = sqlx::query_as(
            "select id, project_id, name, description, workflow_id, input, schedule, enabled, \
             title_template, description_template, created_by, created_by_id, next_run_at, \
             last_run_at, version, created_at, updated_at \
             from triggers where project_id=$1 and id=$2",
        )
        .bind(project_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(trigger_from_row).transpose()
    }

    async fn list(&self, project_id: Uuid) -> Result<Vec<Trigger>, RepoError> {
        let rows: Vec<TriggerRow> = sqlx::query_as(
            "select id, project_id, name, description, workflow_id, input, schedule, enabled, \
             title_template, description_template, created_by, created_by_id, next_run_at, \
             last_run_at, version, created_at, updated_at \
             from triggers where project_id=$1 order by created_at, id",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(trigger_from_row).collect()
    }

    async fn delete(
        &self,
        project_id: Uuid,
        id: Uuid,
        expected_version: u32,
    ) -> Result<(), RepoError> {
        let result =
            sqlx::query("delete from triggers where project_id=$1 and id=$2 and version=$3")
                .bind(project_id)
                .bind(id)
                .bind(expected_version as i32)
                .execute(&self.pool)
                .await
                .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::Backend("trigger version conflict".to_string()));
        }
        Ok(())
    }

    async fn list_due(&self, now: OffsetDateTime) -> Result<Vec<Trigger>, RepoError> {
        let rows: Vec<TriggerRow> = sqlx::query_as(
            "select id, project_id, name, description, workflow_id, input, schedule, enabled, \
             title_template, description_template, created_by, created_by_id, next_run_at, \
             last_run_at, version, created_at, updated_at \
             from triggers \
             where enabled and schedule <> 'manual' and next_run_at is not null and next_run_at <= $1 \
             order by next_run_at, id",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(trigger_from_row).collect()
    }

    async fn insert_run(&self, run: &TriggerRun) -> Result<(), RepoError> {
        sqlx::query(
            "insert into trigger_runs (id, trigger_id, task_id, reason, triggered_by, created_at) \
             values ($1,$2,$3,$4,$5,$6)",
        )
        .bind(run.id)
        .bind(run.trigger_id)
        .bind(run.task_id)
        .bind(run.reason.as_str())
        .bind(serde_json::to_value(&run.triggered_by).map_err(backend)?)
        .bind(run.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn runs(&self, project_id: Uuid, trigger_id: Uuid) -> Result<Vec<TriggerRun>, RepoError> {
        let rows: Vec<RunRow> = sqlx::query_as(
            "select r.id, r.trigger_id, r.task_id, r.reason, r.triggered_by, r.created_at \
             from trigger_runs r join triggers t on t.id = r.trigger_id \
             where t.project_id=$1 and r.trigger_id=$2 order by r.created_at desc",
        )
        .bind(project_id)
        .bind(trigger_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(run_from_row).collect()
    }

    async fn has_active_run(&self, trigger_id: Uuid) -> Result<bool, RepoError> {
        let active: bool = sqlx::query_scalar(
            "select exists(select 1 from trigger_runs r join tasks t on t.id = r.task_id \
             where r.trigger_id=$1 and t.state <> 'completed')",
        )
        .bind(trigger_id)
        .fetch_one(&self.pool)
        .await
        .map_err(backend)?;
        Ok(active)
    }
}

fn trigger_from_row(row: TriggerRow) -> Result<Trigger, RepoError> {
    Ok(Trigger {
        id: row.id,
        project_id: row.project_id,
        name: row.name,
        description: row.description,
        workflow_id: row.workflow_id,
        input: row.input,
        schedule: row.schedule,
        enabled: row.enabled,
        title_template: row.title_template,
        description_template: row.description_template,
        created_by: serde_json::from_value::<Actor>(row.created_by).map_err(backend)?,
        created_by_id: row.created_by_id,
        next_run_at: row.next_run_at,
        last_run_at: row.last_run_at,
        version: row.version as u32,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

fn run_from_row(row: RunRow) -> Result<TriggerRun, RepoError> {
    Ok(TriggerRun {
        id: row.id,
        trigger_id: row.trigger_id,
        task_id: row.task_id,
        reason: TriggerRunReason::parse(&row.reason).ok_or_else(|| {
            RepoError::Backend(format!("unknown trigger run reason {}", row.reason))
        })?,
        triggered_by: serde_json::from_value::<Actor>(row.triggered_by).map_err(backend)?,
        created_at: row.created_at,
    })
}

fn backend(error: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn map_write_error(error: sqlx::Error) -> RepoError {
    match error {
        sqlx::Error::Database(database) if database.is_unique_violation() => {
            RepoError::Duplicate("trigger".to_string())
        }
        other => RepoError::Backend(other.to_string()),
    }
}
