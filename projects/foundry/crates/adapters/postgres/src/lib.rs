//! Postgres adapter.
//!
//! The schema lives in `migrations/` and is applied by [`PostgresStore::migrate`]
//! at startup. Operational state is stored transactionally: aggregates keep a
//! JSONB snapshot in `body` (so a row rebuilds without replaying events) while
//! scalar columns are projections used for filtering and indexing.
//!
//! Design constraints:
//!
//! * `save` is `UPDATE ... WHERE id = $1 AND version = $2`; a zero-row result is
//!   mapped to [`RepoError::Conflict`] (or `NotFound` when the row is absent).
//! * events are written in one transaction. They are not yet atomic with the
//!   aggregate write, because the repository and event sink are separate ports;
//!   the aggregate change commits first.
//! * every list/get path is project-scoped; there is no cross-project query.

use async_trait::async_trait;
use foundry_app::{
    AccountFilter, AccountRepository, EventError, EventSink, IssueFilter, IssueRepository,
    ProjectFilter, ProjectRepository, RepoError, RequirementFilter, RequirementQuery,
    RequirementRepository, SessionRepository,
};
use foundry_domain::{
    Account, AccountKind, AccountRole, AccountStatus, Actor, DomainEvent, Issue, Project,
    ProjectMember, Requirement, Session,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sqlx::postgres::{PgPool, PgPoolOptions};
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;
use time::OffsetDateTime;
use uuid::Uuid;

mod task;
mod workflow;

pub use task::*;
pub use workflow::*;

#[derive(Clone)]
pub struct PostgresStore {
    pool: PgPool,
}

impl PostgresStore {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Apply every migration embedded from `migrations/`. Idempotent: sqlx
    /// records applied versions in `_sqlx_migrations`.
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        sqlx::migrate!("../../../migrations").run(&self.pool).await
    }

    async fn cas_error(&self, table: &str, id: Uuid, expected: u32) -> RepoError {
        let sql = format!("select version from {table} where id = $1");
        match sqlx::query_scalar::<_, i32>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
        {
            Ok(Some(actual)) => RepoError::Conflict {
                expected,
                actual: actual.max(0) as u32,
            },
            Ok(None) => RepoError::NotFound,
            Err(error) => RepoError::Backend(error.to_string()),
        }
    }

    async fn project_members(&self, project_id: Uuid) -> Result<Vec<ProjectMember>, RepoError> {
        let rows: Vec<(Uuid, String, OffsetDateTime)> = sqlx::query_as(
            "select account_id, role, added_at from project_members \
             where project_id = $1 order by added_at",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(|(account_id, role, added_at)| {
                Ok(ProjectMember {
                    account_id,
                    role: parse_enum(&role)?,
                    added_at,
                })
            })
            .collect()
    }

    async fn members_for(
        &self,
        project_ids: &[Uuid],
    ) -> Result<HashMap<Uuid, Vec<ProjectMember>>, RepoError> {
        if project_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows: Vec<(Uuid, Uuid, String, OffsetDateTime)> = sqlx::query_as(
            "select project_id, account_id, role, added_at from project_members \
             where project_id = any($1) order by added_at",
        )
        .bind(project_ids)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let mut grouped: HashMap<Uuid, Vec<ProjectMember>> = HashMap::new();
        for (project_id, account_id, role, added_at) in rows {
            grouped.entry(project_id).or_default().push(ProjectMember {
                account_id,
                role: parse_enum(&role)?,
                added_at,
            });
        }
        Ok(grouped)
    }

    async fn insert_members(
        tx: &mut Transaction<'_, Postgres>,
        project: &Project,
    ) -> Result<(), RepoError> {
        for member in &project.members {
            sqlx::query(
                "insert into project_members (project_id, account_id, role, added_at) \
                 values ($1, $2, $3, $4)",
            )
            .bind(project.id)
            .bind(member.account_id)
            .bind(enum_text(&member.role)?)
            .bind(member.added_at)
            .execute(&mut **tx)
            .await
            .map_err(backend)?;
        }
        Ok(())
    }
}

#[async_trait]
impl RequirementRepository for PostgresStore {
    async fn insert(&self, requirement: &Requirement) -> Result<(), RepoError> {
        let body = to_json(requirement)?;
        sqlx::query(
            "insert into work_items \
             (id, project_id, kind, status, title, owner_kind, owner_id, priority, risk, \
              version, body, created_at, updated_at) \
             values ($1, $2, 'requirement', $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(requirement.id)
        .bind(requirement.project_id)
        .bind(enum_text(&requirement.status)?)
        .bind(&requirement.title)
        .bind(actor_kind(&requirement.owner))
        .bind(requirement.owner.id())
        .bind(enum_text(&requirement.priority)?)
        .bind(enum_text(&requirement.risk)?)
        .bind(requirement.version as i32)
        .bind(body)
        .bind(requirement.created_at)
        .bind(requirement.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn save(
        &self,
        requirement: &Requirement,
        expected_version: u32,
    ) -> Result<(), RepoError> {
        let body = to_json(requirement)?;
        let result = sqlx::query(
            "update work_items set status = $3, title = $4, owner_kind = $5, owner_id = $6, \
             priority = $7, risk = $8, version = $9, body = $10, updated_at = $11 \
             where id = $1 and version = $2 and kind = 'requirement'",
        )
        .bind(requirement.id)
        .bind(expected_version as i32)
        .bind(enum_text(&requirement.status)?)
        .bind(&requirement.title)
        .bind(actor_kind(&requirement.owner))
        .bind(requirement.owner.id())
        .bind(enum_text(&requirement.priority)?)
        .bind(enum_text(&requirement.risk)?)
        .bind(requirement.version as i32)
        .bind(body)
        .bind(requirement.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(self
                .cas_error("work_items", requirement.id, expected_version)
                .await);
        }
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, RepoError> {
        let body: Option<Value> = sqlx::query_scalar(
            "select body from work_items where id = $1 and kind = 'requirement'",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        body.map(from_json).transpose()
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, RepoError> {
        let status = filter.status.map(|status| enum_text(&status)).transpose()?;
        let bodies: Vec<Value> = sqlx::query_scalar(
            "select body from work_items where project_id = $1 and kind = 'requirement' \
             and ($2::text is null or status = $2) \
             and ($3::text is null or owner_id = $3) \
             and ($4::text is null or strpos(lower(title || ' ' || coalesce(body->>'problem', '')), lower($4)) > 0) \
             order by created_at",
        )
        .bind(project_id)
        .bind(status)
        .bind(filter.owner)
        .bind(filter.text)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        bodies.into_iter().map(from_json).collect()
    }
}

/// `RequirementQuery` is the read side used by the HTTP layer. The store is
/// already a `RequirementRepository`; delegate so both traits share one path.
#[async_trait]
impl RequirementQuery for PostgresStore {
    async fn get(&self, id: Uuid) -> Result<Option<Requirement>, RepoError> {
        RequirementRepository::get(self, id).await
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: RequirementFilter,
    ) -> Result<Vec<Requirement>, RepoError> {
        RequirementRepository::list(self, project_id, filter).await
    }
}

#[async_trait]
impl IssueRepository for PostgresStore {
    async fn insert(&self, issue: &Issue) -> Result<(), RepoError> {
        let body = to_json(issue)?;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query(
            "insert into issues \
             (id, project_id, kind, status, title, source_kind, reporter_kind, reporter_id, \
              assignee_kind, assignee_id, severity, area, duplicate_of, version, body, \
              created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(issue.id)
        .bind(issue.project_id)
        .bind(issue.kind.as_str())
        .bind(issue.status.as_str())
        .bind(&issue.title)
        .bind(issue.source.kind.as_str())
        .bind(actor_kind(&issue.reporter))
        .bind(issue.reporter.id())
        .bind(issue.assignee.as_ref().map(actor_kind))
        .bind(issue.assignee.as_ref().map(Actor::id))
        .bind(issue.severity.map(|value| enum_text(&value)).transpose()?)
        .bind(&issue.area)
        .bind(issue.duplicate_of)
        .bind(issue.version as i32)
        .bind(body)
        .bind(issue.created_at)
        .bind(issue.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        sync_comments(&mut tx, issue).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn save(&self, issue: &Issue, expected_version: u32) -> Result<(), RepoError> {
        let body = to_json(issue)?;
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "update issues set kind = $3, status = $4, title = $5, source_kind = $6, \
             reporter_kind = $7, reporter_id = $8, assignee_kind = $9, assignee_id = $10, \
             severity = $11, area = $12, duplicate_of = $13, version = $14, body = $15, \
             updated_at = $16 \
             where id = $1 and version = $2",
        )
        .bind(issue.id)
        .bind(expected_version as i32)
        .bind(issue.kind.as_str())
        .bind(issue.status.as_str())
        .bind(&issue.title)
        .bind(issue.source.kind.as_str())
        .bind(actor_kind(&issue.reporter))
        .bind(issue.reporter.id())
        .bind(issue.assignee.as_ref().map(actor_kind))
        .bind(issue.assignee.as_ref().map(Actor::id))
        .bind(issue.severity.map(|value| enum_text(&value)).transpose()?)
        .bind(&issue.area)
        .bind(issue.duplicate_of)
        .bind(issue.version as i32)
        .bind(body)
        .bind(issue.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(self.cas_error("issues", issue.id, expected_version).await);
        }
        sync_comments(&mut tx, issue).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Issue>, RepoError> {
        let body: Option<Value> = sqlx::query_scalar("select body from issues where id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend)?;
        body.map(from_json).transpose()
    }

    async fn list(&self, project_id: Uuid, filter: IssueFilter) -> Result<Vec<Issue>, RepoError> {
        let status = filter.status.map(|status| status.as_str().to_string());
        let kind = filter.kind.map(|kind| kind.as_str().to_string());
        let source = filter.source.map(|source| source.as_str().to_string());
        let bodies: Vec<Value> = sqlx::query_scalar(
            "select body from issues where project_id = $1 \
             and ($2::text is null or status = $2) \
             and ($3::text is null or kind = $3) \
             and ($4::text is null or source_kind = $4) \
             and ($5::text is null or assignee_id = $5) \
             and ($6::text is null or strpos(lower(title || ' ' || coalesce(body->>'description', '')), lower($6)) > 0) \
             order by created_at desc",
        )
        .bind(project_id)
        .bind(status)
        .bind(kind)
        .bind(source)
        .bind(filter.assignee)
        .bind(filter.text)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        bodies.into_iter().map(from_json).collect()
    }
}

async fn sync_comments(tx: &mut Transaction<'_, Postgres>, issue: &Issue) -> Result<(), RepoError> {
    sqlx::query("delete from issue_comments where issue_id = $1")
        .bind(issue.id)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    for comment in &issue.comments {
        sqlx::query(
            "insert into issue_comments \
             (id, issue_id, author_kind, author_id, body, internal, created_at) \
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(comment.id)
        .bind(issue.id)
        .bind(actor_kind(&comment.author))
        .bind(comment.author.id())
        .bind(&comment.body)
        .bind(comment.internal)
        .bind(comment.created_at)
        .execute(&mut **tx)
        .await
        .map_err(backend)?;
    }
    Ok(())
}

#[async_trait]
impl AccountRepository for PostgresStore {
    async fn insert(&self, account: &Account) -> Result<(), RepoError> {
        sqlx::query(
            "insert into accounts \
             (id, handle, display_name, kind, role, status, email, password_hash, version, \
              created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(account.id)
        .bind(&account.handle)
        .bind(&account.display_name)
        .bind(account.kind.as_str())
        .bind(account.role.as_str())
        .bind(account.status.as_str())
        .bind(&account.email)
        .bind(&account.password_hash)
        .bind(account.version as i32)
        .bind(account.created_at)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn save(&self, account: &Account, expected_version: u32) -> Result<(), RepoError> {
        let result = sqlx::query(
            "update accounts set handle = $3, display_name = $4, kind = $5, role = $6, \
             status = $7, email = $8, password_hash = $9, version = $10, updated_at = $11 \
             where id = $1 and version = $2",
        )
        .bind(account.id)
        .bind(expected_version as i32)
        .bind(&account.handle)
        .bind(&account.display_name)
        .bind(account.kind.as_str())
        .bind(account.role.as_str())
        .bind(account.status.as_str())
        .bind(&account.email)
        .bind(&account.password_hash)
        .bind(account.version as i32)
        .bind(account.updated_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(self
                .cas_error("accounts", account.id, expected_version)
                .await);
        }
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Account>, RepoError> {
        let row = sqlx::query_as::<_, AccountRow>(
            "select id, handle, display_name, kind, role, status, email, password_hash, \
             version, created_at, updated_at from accounts where id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(AccountRow::into_account).transpose()
    }

    async fn get_by_handle(&self, handle: &str) -> Result<Option<Account>, RepoError> {
        let row = sqlx::query_as::<_, AccountRow>(
            "select id, handle, display_name, kind, role, status, email, password_hash, \
             version, created_at, updated_at from accounts where handle = lower(trim($1))",
        )
        .bind(handle)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(AccountRow::into_account).transpose()
    }

    async fn list(&self, filter: AccountFilter) -> Result<Vec<Account>, RepoError> {
        let kind = filter.kind.map(|kind| kind.as_str().to_string());
        let status = filter.status.map(|status| status.as_str().to_string());
        let rows = sqlx::query_as::<_, AccountRow>(
            "select id, handle, display_name, kind, role, status, email, password_hash, \
             version, created_at, updated_at from accounts \
             where ($1::text is null or kind = $1) \
             and ($2::text is null or status = $2) \
             and ($3::text is null or strpos(lower(handle || ' ' || display_name), lower($3)) > 0) \
             order by handle",
        )
        .bind(kind)
        .bind(status)
        .bind(filter.text)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(AccountRow::into_account).collect()
    }
}

type AccountRow = (
    Uuid,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    i32,
    OffsetDateTime,
    OffsetDateTime,
);

trait IntoAccount {
    fn into_account(self) -> Result<Account, RepoError>;
}

impl IntoAccount for AccountRow {
    fn into_account(self) -> Result<Account, RepoError> {
        let (
            id,
            handle,
            display_name,
            kind,
            role,
            status,
            email,
            password_hash,
            version,
            created_at,
            updated_at,
        ) = self;
        Ok(Account {
            id,
            handle,
            display_name,
            kind: parse_account_kind(&kind)?,
            role: parse_account_role(&role)?,
            status: parse_account_status(&status)?,
            email,
            password_hash,
            version: version.max(0) as u32,
            created_at,
            updated_at,
        })
    }
}

#[async_trait]
impl ProjectRepository for PostgresStore {
    async fn insert(&self, project: &Project) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query(
            "insert into projects \
             (id, name, description, status, root_task_id, version, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(project.id)
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.status.as_str())
        .bind(project.root_task_id)
        .bind(project.version as i32)
        .bind(project.created_at)
        .bind(project.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        PostgresStore::insert_members(&mut tx, project).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn insert_with_root(
        &self,
        project: &Project,
        root: &foundry_domain::TaskDAO,
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        sqlx::query(
            "insert into projects \
             (id, name, description, status, root_task_id, version, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(project.id)
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.status.as_str())
        .bind(project.root_task_id)
        .bind(project.version as i32)
        .bind(project.created_at)
        .bind(project.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        PostgresStore::insert_members(&mut tx, project).await?;
        crate::task::insert_task(&mut tx, root).await?;
        crate::task::append_events(&mut tx, project.id, events).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn save(&self, project: &Project, expected_version: u32) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        let result = sqlx::query(
            "update projects set name = $3, description = $4, status = $5, \
             version = $6, updated_at = $7 where id = $1 and version = $2",
        )
        .bind(project.id)
        .bind(expected_version as i32)
        .bind(&project.name)
        .bind(&project.description)
        .bind(project.status.as_str())
        .bind(project.version as i32)
        .bind(project.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(map_write_error)?;
        if result.rows_affected() == 0 {
            return Err(self
                .cas_error("projects", project.id, expected_version)
                .await);
        }
        sqlx::query("delete from project_members where project_id = $1")
            .bind(project.id)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        PostgresStore::insert_members(&mut tx, project).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<Project>, RepoError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "select id, name, description, status, root_task_id, version, created_at, updated_at \
             from projects where id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let members = self.project_members(id).await?;
        row.into_project(members).map(Some)
    }

    async fn list(&self, filter: ProjectFilter) -> Result<Vec<Project>, RepoError> {
        let status = filter.status.map(|status| status.as_str().to_string());
        let rows = sqlx::query_as::<_, ProjectRow>(
            "select id, name, description, status, root_task_id, version, created_at, updated_at \
             from projects \
             where ($1::text is null or status = $1) \
             and ($2::text is null or strpos(lower(name), lower($2)) > 0) \
             order by created_at",
        )
        .bind(status)
        .bind(filter.text)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        let ids: Vec<Uuid> = rows.iter().map(|row| row.0).collect();
        let mut members = self.members_for(&ids).await?;
        rows.into_iter()
            .map(|row| {
                let project_members = members.remove(&row.0).unwrap_or_default();
                row.into_project(project_members)
            })
            .collect()
    }
}

type ProjectRow = (
    Uuid,
    String,
    Option<String>,
    String,
    Option<Uuid>,
    i32,
    OffsetDateTime,
    OffsetDateTime,
);

trait IntoProject {
    fn into_project(self, members: Vec<ProjectMember>) -> Result<Project, RepoError>;
}

impl IntoProject for ProjectRow {
    fn into_project(self, members: Vec<ProjectMember>) -> Result<Project, RepoError> {
        let (id, name, description, status, root_task_id, version, created_at, updated_at) = self;
        Ok(Project {
            id,
            name,
            description,
            status: parse_enum(&status)?,
            root_task_id,
            members,
            version: version.max(0) as u32,
            created_at,
            updated_at,
        })
    }
}

#[async_trait]
impl SessionRepository for PostgresStore {
    async fn insert(&self, session: &Session) -> Result<(), RepoError> {
        sqlx::query(
            "insert into sessions (id, account_id, token_hash, created_at, expires_at, revoked_at) \
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(session.id)
        .bind(session.account_id)
        .bind(&session.token_hash)
        .bind(session.created_at)
        .bind(session.expires_at)
        .bind(session.revoked_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    async fn get_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>, RepoError> {
        let row: Option<(
            Uuid,
            Uuid,
            String,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
        )> = sqlx::query_as(
            "select id, account_id, token_hash, created_at, expires_at, revoked_at \
                 from sessions where token_hash = $1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row.map(
            |(id, account_id, token_hash, created_at, expires_at, revoked_at)| Session {
                id,
                account_id,
                token_hash,
                created_at,
                expires_at,
                revoked_at,
            },
        ))
    }

    async fn save(&self, session: &Session) -> Result<(), RepoError> {
        let result =
            sqlx::query("update sessions set expires_at = $2, revoked_at = $3 where id = $1")
                .bind(session.id)
                .bind(session.expires_at)
                .bind(session.revoked_at)
                .execute(&self.pool)
                .await
                .map_err(backend)?;
        if result.rows_affected() == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }
}

/// Append-only event sink. Events are the durable record of accepted inputs
/// and effects; nothing consumes them beyond reads.
#[derive(Clone)]
pub struct PostgresEventSink {
    pool: PgPool,
}

impl PostgresEventSink {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EventSink for PostgresEventSink {
    async fn publish(&self, events: &[DomainEvent]) -> Result<(), EventError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|error| EventError::Backend(error.to_string()))?;
        for event in events {
            let kind =
                enum_text(&event.kind).map_err(|error| EventError::Backend(error.to_string()))?;
            let actor = serde_json::to_value(&event.actor)
                .map_err(|error| EventError::Backend(error.to_string()))?;
            sqlx::query(
                "insert into events \
                 (id, project_id, aggregate_id, kind, actor, payload, occurred_at) \
                 values ($1, null, $2, $3, $4, $5, $6)",
            )
            .bind(event.id)
            .bind(event.aggregate_id)
            .bind(kind)
            .bind(actor)
            .bind(&event.payload)
            .bind(event.occurred_at)
            .execute(&mut *tx)
            .await
            .map_err(|error| EventError::Backend(error.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|error| EventError::Backend(error.to_string()))?;
        Ok(())
    }
}

fn actor_kind(actor: &Actor) -> &'static str {
    match actor {
        Actor::Human { .. } => "human",
        Actor::Agent { .. } => "agent",
        Actor::System => "system",
    }
}

fn enum_text<T: Serialize>(value: &T) -> Result<String, RepoError> {
    match serde_json::to_value(value).map_err(|error| RepoError::Backend(error.to_string()))? {
        Value::String(text) => Ok(text),
        other => Err(RepoError::Backend(format!(
            "expected a string enum value, got {other}"
        ))),
    }
}

fn parse_enum<T: DeserializeOwned>(value: &str) -> Result<T, RepoError> {
    serde_json::from_value(Value::String(value.to_string()))
        .map_err(|error| RepoError::Backend(format!("unknown enum value {value:?}: {error}")))
}

fn parse_account_kind(value: &str) -> Result<AccountKind, RepoError> {
    parse_enum(value)
}

fn parse_account_role(value: &str) -> Result<AccountRole, RepoError> {
    parse_enum(value)
}

fn parse_account_status(value: &str) -> Result<AccountStatus, RepoError> {
    parse_enum(value)
}

fn to_json<T: Serialize>(value: &T) -> Result<Value, RepoError> {
    serde_json::to_value(value).map_err(|error| RepoError::Backend(error.to_string()))
}

fn from_json<T: DeserializeOwned>(value: Value) -> Result<T, RepoError> {
    serde_json::from_value(value).map_err(|error| RepoError::Backend(error.to_string()))
}

fn backend(error: sqlx::Error) -> RepoError {
    match error {
        sqlx::Error::RowNotFound => RepoError::NotFound,
        other => RepoError::Backend(other.to_string()),
    }
}

fn map_write_error(error: sqlx::Error) -> RepoError {
    if let sqlx::Error::Database(database) = &error
        && database.code().as_deref() == Some("23505")
    {
        let detail = database
            .constraint()
            .map(str::to_string)
            .unwrap_or_else(|| "unique value".to_string());
        return RepoError::Duplicate(detail);
    }
    backend(error)
}
