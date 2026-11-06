use async_trait::async_trait;
use foundry_app::{
    DraftSaveOutcome, PublishOutcome, RepoError, WorkflowCatalog, WorkflowDefinitionRow,
    WorkflowLookup, WorkflowRepository, WorkflowVersionRow,
};
use foundry_domain::{
    DefinitionWorkflow, TaskWorkflow, WorkflowBody, WorkflowDefinitionDAO, WorkflowDraftDAO,
    WorkflowId, WorkflowLayout, WorkflowLayoutDAO, WorkflowOption, WorkflowOwner,
    WorkflowVersionDAO, WorkflowVersionRef,
};
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::PostgresStore;

/// Loads immutable workflow versions from `workflow_versions`.
///
/// Published versions never change, so resolved ASTs are cached forever. The
/// latest-version queries always go to the database; another control-plane
/// instance may have published while this one was running.
pub struct PostgresWorkflowCatalog {
    pool: PgPool,
    pinned: Mutex<HashMap<(String, u32), Arc<dyn TaskWorkflow>>>,
}

impl PostgresWorkflowCatalog {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            pinned: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl WorkflowCatalog for PostgresWorkflowCatalog {
    async fn pinned(
        &self,
        reference: &WorkflowVersionRef,
    ) -> Result<Option<Arc<dyn TaskWorkflow>>, RepoError> {
        let key = (reference.workflow_id.to_string(), reference.version);
        if let Some(workflow) = self.pinned.lock().expect("lock poisoned").get(&key) {
            return Ok(Some(Arc::clone(workflow)));
        }

        let body: Option<Value> = sqlx::query_scalar(
            "select body from workflow_versions where workflow_id = $1 and version = $2",
        )
        .bind(reference.workflow_id.as_str())
        .bind(reference.version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(body) = body else {
            return Ok(None);
        };
        let body: WorkflowBody = serde_json::from_value(body).map_err(backend)?;
        let workflow: Arc<dyn TaskWorkflow> = Arc::new(DefinitionWorkflow::new(
            reference.workflow_id.to_string(),
            body,
        ));
        self.pinned
            .lock()
            .expect("lock poisoned")
            .insert(key, Arc::clone(&workflow));
        Ok(Some(workflow))
    }

    async fn latest_visible(
        &self,
        account_id: Option<Uuid>,
        workflow_id: &WorkflowId,
    ) -> Result<WorkflowLookup, RepoError> {
        let row: Option<(Option<i32>,)> = sqlx::query_as(
            "select (select max(version) from workflow_versions where workflow_id = d.id) \
             from workflow_definitions d \
             where d.id = $1 \
               and (d.owner_account_id is null \
                    or ($2::uuid is not null and d.owner_account_id = $2))",
        )
        .bind(workflow_id.as_str())
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(match row {
            None => WorkflowLookup::NotFound,
            Some((Some(version),)) => WorkflowLookup::Published(WorkflowVersionRef {
                workflow_id: workflow_id.clone(),
                version: version as u32,
            }),
            Some((None,)) => WorkflowLookup::Unpublished(workflow_id.clone()),
        })
    }

    async fn resolve_child(
        &self,
        parent: &WorkflowId,
        child: &WorkflowId,
    ) -> Result<Option<WorkflowVersionRef>, RepoError> {
        let owner: Option<Option<Uuid>> =
            sqlx::query_scalar("select owner_account_id from workflow_definitions where id = $1")
                .bind(parent.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(backend)?;
        let Some(owner) = owner else {
            return Ok(None);
        };
        Ok(match self.latest_visible(owner, child).await? {
            WorkflowLookup::Published(reference) => Some(reference),
            WorkflowLookup::Unpublished(_) | WorkflowLookup::NotFound => None,
        })
    }

    async fn root_candidates(&self, account_id: Uuid) -> Result<Vec<WorkflowOption>, RepoError> {
        let rows: Vec<(String, String, Option<Uuid>, Option<String>)> = sqlx::query_as(
            "select d.id, d.title, d.owner_account_id, a.display_name \
             from workflow_definitions d \
             left join accounts a on a.id = d.owner_account_id \
             where (d.owner_account_id is null or d.owner_account_id = $1) \
               and exists ( \
                   select 1 from workflow_versions v where v.workflow_id = d.id \
               ) \
             order by (d.owner_account_id is not null), d.title, d.id",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, title, owner_account_id, display_name)| WorkflowOption {
                    id: WorkflowId::new(id),
                    title,
                    owner: match owner_account_id {
                        None => WorkflowOwner::Platform,
                        Some(account_id) => WorkflowOwner::Account {
                            account_id,
                            display_name: display_name.unwrap_or_default(),
                        },
                    },
                },
            )
            .collect())
    }
}

impl PostgresStore {
    /// Seed the nine platform workflows and validate that every existing Task
    /// pins a published version.
    ///
    /// The platform catalog is system state, not business data: it is
    /// reproduced on every startup. A mismatch with an existing v1 is fatal
    /// because published versions are immutable.
    pub async fn seed_builtin_workflows(&self) -> Result<(), RepoError> {
        let workflows = foundry_domain::builtin_workflows();
        for workflow in &workflows {
            let id = workflow.id();
            let title = foundry_domain::builtin_title(id).unwrap_or(id);
            sqlx::query(
                "insert into workflow_definitions (id, owner_account_id, title) \
                 values ($1, null, $2) on conflict (id) do nothing",
            )
            .bind(id)
            .bind(title)
            .execute(&self.pool)
            .await
            .map_err(backend)?;

            let owner: Option<Uuid> = sqlx::query_scalar(
                "select owner_account_id from workflow_definitions where id = $1",
            )
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .map_err(backend)?;
            if owner.is_some() {
                return Err(RepoError::Backend(format!(
                    "builtin workflow id {id} is already owned by an account"
                )));
            }

            let body = serde_json::to_value(workflow.body_ref()).map_err(backend)?;
            let inserted: Option<Value> = sqlx::query_scalar(
                "insert into workflow_versions (workflow_id, version, body) \
                 values ($1, 1, $2) \
                 on conflict (workflow_id, version) do nothing \
                 returning body",
            )
            .bind(id)
            .bind(body)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
            if inserted.is_none() {
                let stored: Value = sqlx::query_scalar(
                    "select body from workflow_versions where workflow_id = $1 and version = 1",
                )
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(backend)?;
                let stored: WorkflowBody = serde_json::from_value(stored).map_err(backend)?;
                if &stored != workflow.body_ref() {
                    return Err(RepoError::Backend(format!(
                        "builtin workflow {id} v1 does not match the embedded definition; \
                         published versions are immutable"
                    )));
                }
            }
        }

        let unknown: Vec<(String, i32)> = sqlx::query_as(
            "select distinct t.workflow, t.workflow_version from tasks t \
             left join workflow_versions v \
               on v.workflow_id = t.workflow and v.version = t.workflow_version \
             where v.workflow_id is null order by t.workflow, t.workflow_version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        if !unknown.is_empty() {
            return Err(RepoError::Backend(format!(
                "tasks pin workflows with no published version: {unknown:?}"
            )));
        }

        sqlx::query("alter table tasks validate constraint tasks_workflow_version_fk")
            .execute(&self.pool)
            .await
            .map_err(backend)?;

        sqlx::query(
            "update projects p set root_task_id = t.id \
             from tasks t \
             where t.project_id = p.id and t.parent_id is null and p.root_task_id is null \
               and (select count(*) from tasks r \
                    where r.project_id = p.id and r.parent_id is null) = 1",
        )
        .execute(&self.pool)
        .await
        .map_err(backend)?;

        let missing: Vec<(Uuid, String)> = sqlx::query_as(
            "select id, name from projects where root_task_id is null order by created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        if !missing.is_empty() {
            tracing::warn!(
                projects = ?missing,
                "projects without a unique root task; they are not modified automatically"
            );
        }
        Ok(())
    }
}

fn backend(error: impl std::fmt::Display) -> RepoError {
    RepoError::Backend(error.to_string())
}

fn body_json(body: &WorkflowBody) -> Result<Value, RepoError> {
    serde_json::to_value(body).map_err(backend)
}

fn parse_body(value: Value) -> Result<WorkflowBody, RepoError> {
    serde_json::from_value(value).map_err(backend)
}

fn revision_of(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

#[async_trait]
impl WorkflowRepository for PostgresStore {
    async fn insert_definition(
        &self,
        definition: &WorkflowDefinitionDAO,
        draft: &WorkflowDraftDAO,
        layout: Option<&WorkflowLayoutDAO>,
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query(
            "insert into workflow_definitions \
             (id, owner_account_id, title, created_at, updated_at) \
             values ($1, $2, $3, $4, $5)",
        )
        .bind(definition.id.as_str())
        .bind(definition.owner_account_id)
        .bind(&definition.title)
        .bind(definition.created_at)
        .bind(definition.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(crate::map_write_error)?;
        sqlx::query(
            "insert into workflow_drafts \
             (workflow_id, revision, body, last_client_save_id, updated_by_account_id, updated_at) \
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(draft.workflow_id.as_str())
        .bind(draft.revision as i64)
        .bind(body_json(&draft.body)?)
        .bind(draft.last_client_save_id)
        .bind(draft.updated_by_account_id)
        .bind(draft.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(crate::map_write_error)?;
        if let Some(layout) = layout {
            sqlx::query(
                "insert into workflow_layouts \
                 (workflow_id, layout, updated_by_account_id, updated_at) \
                 values ($1, $2, $3, $4)",
            )
            .bind(layout.workflow_id.as_str())
            .bind(serde_json::to_value(&layout.layout).map_err(backend)?)
            .bind(layout.updated_by_account_id)
            .bind(layout.updated_at)
            .execute(&mut *tx)
            .await
            .map_err(crate::map_write_error)?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn definition(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowDefinitionDAO>, RepoError> {
        let row: Option<(String, Option<Uuid>, String, OffsetDateTime, OffsetDateTime)> =
            sqlx::query_as(
                "select id, owner_account_id, title, created_at, updated_at \
                 from workflow_definitions where id = $1",
            )
            .bind(id.as_str())
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        Ok(row.map(
            |(id, owner, title, created_at, updated_at)| WorkflowDefinitionDAO {
                id: WorkflowId::new(id),
                owner_account_id: owner,
                title,
                created_at,
                updated_at,
            },
        ))
    }

    async fn definition_row(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowDefinitionRow>, RepoError> {
        let row: Option<(
            String,
            Option<Uuid>,
            String,
            OffsetDateTime,
            OffsetDateTime,
            Option<i32>,
            Option<i64>,
        )> = sqlx::query_as(
            "select d.id, d.owner_account_id, d.title, d.created_at, d.updated_at, \
                    (select max(v.version) from workflow_versions v where v.workflow_id = d.id), \
                    (select dr.revision from workflow_drafts dr where dr.workflow_id = d.id) \
             from workflow_definitions d where d.id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row.map(definition_row))
    }

    async fn list_definitions(
        &self,
        account_id: Uuid,
        is_admin: bool,
    ) -> Result<Vec<WorkflowDefinitionRow>, RepoError> {
        let rows: Vec<(
            String,
            Option<Uuid>,
            String,
            OffsetDateTime,
            OffsetDateTime,
            Option<i32>,
            Option<i64>,
        )> = sqlx::query_as(
            "select d.id, d.owner_account_id, d.title, d.created_at, d.updated_at, \
                    (select max(v.version) from workflow_versions v where v.workflow_id = d.id), \
                    (select dr.revision from workflow_drafts dr where dr.workflow_id = d.id) \
             from workflow_definitions d \
             where (d.owner_account_id is null \
                    and ($1 or exists (select 1 from workflow_versions v \
                                       where v.workflow_id = d.id))) \
                or d.owner_account_id = $2 \
                or ($1 and d.owner_account_id is not null) \
             order by (d.owner_account_id is not null), d.title, d.id",
        )
        .bind(is_admin)
        .bind(account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows.into_iter().map(definition_row).collect())
    }

    async fn rename(
        &self,
        id: &WorkflowId,
        title: &str,
        now: OffsetDateTime,
    ) -> Result<(), RepoError> {
        let result = sqlx::query(
            "update workflow_definitions set title = $2, updated_at = $3 where id = $1",
        )
        .bind(id.as_str())
        .bind(title)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    async fn versions(&self, id: &WorkflowId) -> Result<Vec<WorkflowVersionRow>, RepoError> {
        let rows: Vec<(i32, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "select version, published_by_account_id, published_at \
             from workflow_versions where workflow_id = $1 order by version desc",
        )
        .bind(id.as_str())
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|(version, publisher, published_at)| WorkflowVersionRow {
                version: version.max(0) as u32,
                published_by_account_id: publisher,
                published_at,
            })
            .collect())
    }

    async fn version(
        &self,
        id: &WorkflowId,
        version: u32,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError> {
        let row: Option<(String, i32, Value, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "select workflow_id, version, body, published_by_account_id, published_at \
             from workflow_versions where workflow_id = $1 and version = $2",
        )
        .bind(id.as_str())
        .bind(version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_row).transpose()
    }

    async fn latest_version(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowVersionDAO>, RepoError> {
        let row: Option<(String, i32, Value, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "select workflow_id, version, body, published_by_account_id, published_at \
             from workflow_versions where workflow_id = $1 order by version desc limit 1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(version_row).transpose()
    }

    async fn published_ids(
        &self,
        owner_account_id: Option<Uuid>,
    ) -> Result<Vec<WorkflowId>, RepoError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "select distinct v.workflow_id \
             from workflow_versions v join workflow_definitions d on d.id = v.workflow_id \
             where d.owner_account_id is null or d.owner_account_id = $1",
        )
        .bind(owner_account_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows.into_iter().map(WorkflowId::new).collect())
    }

    async fn draft(&self, id: &WorkflowId) -> Result<Option<WorkflowDraftDAO>, RepoError> {
        let row: Option<(String, i64, Value, Option<Uuid>, Uuid, OffsetDateTime)> = sqlx::query_as(
            "select workflow_id, revision, body, last_client_save_id, updated_by_account_id, \
                    updated_at \
             from workflow_drafts where workflow_id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(draft_row).transpose()
    }

    async fn open_draft(
        &self,
        id: &WorkflowId,
        fallback: WorkflowBody,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowDraftDAO, RepoError> {
        sqlx::query(
            "insert into workflow_drafts \
             (workflow_id, revision, body, last_client_save_id, updated_by_account_id, updated_at) \
             values ($1, 1, $2, null, $3, $4) \
             on conflict (workflow_id) do nothing",
        )
        .bind(id.as_str())
        .bind(body_json(&fallback)?)
        .bind(account_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(crate::map_write_error)?;
        self.draft(id)
            .await?
            .ok_or_else(|| RepoError::Backend("draft disappeared after create".to_string()))
    }

    async fn save_draft(
        &self,
        id: &WorkflowId,
        body: &WorkflowBody,
        expected_revision: u64,
        client_save_id: Uuid,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<DraftSaveOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let row: Option<(i64, Option<Uuid>, OffsetDateTime)> = sqlx::query_as(
            "select revision, last_client_save_id, updated_at \
             from workflow_drafts where workflow_id = $1 for update",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((current, last_client_save_id, updated_at)) = row else {
            return Err(RepoError::NotFound);
        };
        if last_client_save_id == Some(client_save_id) {
            tx.commit().await.map_err(backend)?;
            return Ok(DraftSaveOutcome::Saved {
                revision: revision_of(current),
                updated_at,
            });
        }
        if revision_of(current) != expected_revision {
            tx.rollback().await.map_err(backend)?;
            return Ok(DraftSaveOutcome::Conflict {
                current_revision: revision_of(current),
                current_updated_at: updated_at,
            });
        }
        let updated_at: OffsetDateTime = sqlx::query_scalar(
            "update workflow_drafts \
             set body = $2, revision = revision + 1, last_client_save_id = $3, \
                 updated_by_account_id = $4, updated_at = $5 \
             where workflow_id = $1 returning updated_at",
        )
        .bind(id.as_str())
        .bind(body_json(body)?)
        .bind(client_save_id)
        .bind(account_id)
        .bind(now)
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(DraftSaveOutcome::Saved {
            revision: revision_of(current) + 1,
            updated_at,
        })
    }

    async fn publish(
        &self,
        id: &WorkflowId,
        expected_revision: u64,
        publisher: Uuid,
        now: OffsetDateTime,
    ) -> Result<PublishOutcome, RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let row: Option<(i64, Value, OffsetDateTime)> = sqlx::query_as(
            "select revision, body, updated_at from workflow_drafts \
             where workflow_id = $1 for update",
        )
        .bind(id.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some((current, body, updated_at)) = row else {
            tx.rollback().await.map_err(backend)?;
            return Ok(PublishOutcome::NoDraft);
        };
        if revision_of(current) != expected_revision {
            tx.rollback().await.map_err(backend)?;
            return Ok(PublishOutcome::Conflict {
                current_revision: revision_of(current),
                current_updated_at: updated_at,
            });
        }
        let version: i32 = sqlx::query_scalar(
            "select coalesce(max(version), 0) + 1 from workflow_versions where workflow_id = $1",
        )
        .bind(id.as_str())
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query(
            "insert into workflow_versions \
             (workflow_id, version, body, published_by_account_id, published_at) \
             values ($1, $2, $3, $4, $5)",
        )
        .bind(id.as_str())
        .bind(version)
        .bind(body)
        .bind(publisher)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(crate::map_write_error)?;
        sqlx::query(
            "update workflow_drafts \
             set revision = revision + 1, last_client_save_id = null, \
                 updated_by_account_id = $2, updated_at = $3 \
             where workflow_id = $1",
        )
        .bind(id.as_str())
        .bind(publisher)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(PublishOutcome::Published {
            version: version as u32,
            revision: revision_of(current) + 1,
        })
    }

    async fn layout(&self, id: &WorkflowId) -> Result<Option<WorkflowLayoutDAO>, RepoError> {
        let row: Option<(String, Value, Uuid, OffsetDateTime)> = sqlx::query_as(
            "select workflow_id, layout, updated_by_account_id, updated_at \
             from workflow_layouts where workflow_id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(|(workflow_id, layout, updated_by_account_id, updated_at)| {
            Ok(WorkflowLayoutDAO {
                workflow_id: WorkflowId::new(workflow_id),
                layout: serde_json::from_value(layout).map_err(backend)?,
                updated_by_account_id,
                updated_at,
            })
        })
        .transpose()
    }

    async fn save_layout(
        &self,
        id: &WorkflowId,
        layout: &WorkflowLayout,
        account_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<WorkflowLayoutDAO, RepoError> {
        sqlx::query(
            "insert into workflow_layouts \
             (workflow_id, layout, updated_by_account_id, updated_at) \
             values ($1, $2, $3, $4) \
             on conflict (workflow_id) do update \
             set layout = excluded.layout, \
                 updated_by_account_id = excluded.updated_by_account_id, \
                 updated_at = excluded.updated_at",
        )
        .bind(id.as_str())
        .bind(serde_json::to_value(layout).map_err(backend)?)
        .bind(account_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(crate::map_write_error)?;
        Ok(WorkflowLayoutDAO {
            workflow_id: id.clone(),
            layout: layout.clone(),
            updated_by_account_id: account_id,
            updated_at: now,
        })
    }
}

fn definition_row(
    row: (
        String,
        Option<Uuid>,
        String,
        OffsetDateTime,
        OffsetDateTime,
        Option<i32>,
        Option<i64>,
    ),
) -> WorkflowDefinitionRow {
    let (id, owner, title, created_at, updated_at, latest_version, draft_revision) = row;
    WorkflowDefinitionRow {
        definition: WorkflowDefinitionDAO {
            id: WorkflowId::new(id),
            owner_account_id: owner,
            title,
            created_at,
            updated_at,
        },
        latest_version: latest_version.map(|version| version.max(0) as u32),
        draft_revision: draft_revision.map(revision_of),
    }
}

fn version_row(
    row: (String, i32, Value, Option<Uuid>, OffsetDateTime),
) -> Result<WorkflowVersionDAO, RepoError> {
    let (workflow_id, version, body, publisher, published_at) = row;
    Ok(WorkflowVersionDAO {
        workflow_id: WorkflowId::new(workflow_id),
        version: version.max(0) as u32,
        body: parse_body(body)?,
        published_by_account_id: publisher,
        published_at,
    })
}

fn draft_row(
    row: (String, i64, Value, Option<Uuid>, Uuid, OffsetDateTime),
) -> Result<WorkflowDraftDAO, RepoError> {
    let (workflow_id, revision, body, last_client_save_id, updated_by_account_id, updated_at) = row;
    Ok(WorkflowDraftDAO {
        workflow_id: WorkflowId::new(workflow_id),
        revision: revision_of(revision),
        body: parse_body(body)?,
        last_client_save_id,
        updated_by_account_id,
        updated_at,
    })
}
