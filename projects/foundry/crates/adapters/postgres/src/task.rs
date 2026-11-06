use crate::{PostgresStore, backend, enum_text, map_write_error, parse_enum};
use async_trait::async_trait;
use foundry_app::{
    RepoError, TaskActorRow, TaskCommand, TaskCommentRow, TaskDependencyRow, TaskDispatchError,
    TaskDispatcher, TaskFilter, TaskListRow, TaskRepository, WorkflowCatalog,
};
use foundry_domain::{
    AccountKind, Actor, ChildState, DependencyState, DomainEvent, DomainEventKind, TaskActorDAO,
    TaskAggregate, TaskCommentDAO, TaskDAO, TaskError, TaskEvent, TaskMutation, TaskOperation,
    TaskResult, TaskState, WorkflowId,
};
use serde_json::{Value, json};
use sqlx::postgres::PgPool;
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;
use uuid::Uuid;

const CONSUMER: &str = "task-workflow";

/// Row shape of the `tasks` table projection used by [`row_to_task`].
type TaskRow = (
    Uuid,
    Uuid,
    Option<Uuid>,
    String,
    i32,
    String,
    String,
    Option<String>,
    Option<String>,
    Value,
    Value,
    OffsetDateTime,
    OffsetDateTime,
    Option<OffsetDateTime>,
);

fn trigger_kinds() -> Vec<String> {
    [
        DomainEventKind::TaskCreated,
        DomainEventKind::OperationReceived,
        DomainEventKind::ActorBound,
        DomainEventKind::TaskStateChanged,
        DomainEventKind::ChildCreated,
        DomainEventKind::ChildStateChanged,
        DomainEventKind::DispatchRequested,
    ]
    .into_iter()
    .map(|kind| enum_text(&kind))
    .collect::<Result<_, _>>()
    .unwrap_or_default()
}

#[async_trait]
impl TaskRepository for PostgresStore {
    async fn insert(
        &self,
        task: &TaskDAO,
        dependencies: &[Uuid],
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        insert_task(&mut tx, task).await?;
        for dependency in dependencies {
            insert_dependency(&mut tx, task.id, *dependency).await?;
        }
        append_events(&mut tx, task.project_id, events).await?;
        tx.commit().await.map_err(backend)?;
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<TaskDAO>, RepoError> {
        let row: Option<TaskRow> = sqlx::query_as(
            "select id, project_id, parent_id, workflow, workflow_version, title, state, stage, \
             result, stage_variables, task_variables, created_at, updated_at, completed_at \
             from tasks where id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(row_to_task).transpose()
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
    ) -> Result<Vec<TaskListRow>, RepoError> {
        let state = filter.state.map(TaskState::as_str);
        let dispatch_state = filter
            .dispatch_state
            .map(foundry_domain::TaskDispatchState::as_str);
        let rows: Vec<(
            Uuid,
            Uuid,
            Option<Uuid>,
            String,
            i32,
            String,
            String,
            Option<String>,
            Option<String>,
            Value,
            Value,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
            bool,
        )> = sqlx::query_as(
            "select t.id, t.project_id, t.parent_id, t.workflow, t.workflow_version, t.title, \
             t.state, t.stage, t.result, t.stage_variables, t.task_variables, t.created_at, \
             t.updated_at, t.completed_at, t.dependencies_ready \
             from ( \
                 select t.id, t.project_id, t.parent_id, t.workflow, t.workflow_version, t.title, \
                 t.state, t.stage, t.result, t.stage_variables, t.task_variables, t.created_at, \
                 t.updated_at, \
                 t.completed_at, not exists ( \
                     select 1 from task_dependencies d \
                     join tasks dep on dep.id = d.depends_on_task_id \
                     where d.task_id = t.id and dep.state <> 'completed' \
                 ) as dependencies_ready \
                 from tasks t \
                 where t.project_id = $1 \
                   and ($2::text is null or t.state = $2) \
                   and ($3::uuid is null or t.parent_id = $3) \
                   and ($4::text is null or strpos(lower(t.title), lower($4)) > 0) \
                   and ($5::text is null or t.stage = $5) \
                   and ($6::text is null or exists ( \
                       select 1 from task_actors a \
                       where a.task_id = t.id and a.role_key = $6 \
                   )) \
                   and ($7::uuid is null or exists ( \
                       select 1 from task_actors a \
                       where a.task_id = t.id and a.account_id = $7 and a.active \
                   )) \
             ) t \
             where $8::text is null \
                or ($8 = 'closed' and t.state = 'completed') \
                or ($8 = 'running' and t.state = 'in_progress') \
                or ($8 = 'ready' and t.state = 'pending' and t.dependencies_ready) \
                or ($8 = 'blocked' and t.state = 'pending' and not t.dependencies_ready) \
             order by t.created_at \
             limit $9",
        )
        .bind(project_id)
        .bind(state)
        .bind(filter.parent_id)
        .bind(filter.text)
        .bind(filter.stage.as_ref().map(foundry_domain::StageKey::as_str))
        .bind(filter.role.as_ref().map(foundry_domain::RoleKey::as_str))
        .bind(filter.active_actor)
        .bind(dispatch_state)
        .bind(filter.limit.unwrap_or(100))
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;

        rows.into_iter()
            .map(
                |(
                    id,
                    project_id,
                    parent_id,
                    workflow,
                    workflow_version,
                    title,
                    state,
                    stage,
                    result,
                    stage_variables,
                    task_variables,
                    created_at,
                    updated_at,
                    completed_at,
                    dependencies_ready,
                )| {
                    Ok(TaskListRow {
                        task: TaskDAO {
                            id,
                            project_id,
                            parent_id,
                            workflow: WorkflowId::new(workflow),
                            workflow_version: workflow_version.max(0) as u32,
                            title,
                            state: parse_enum(&state)?,
                            stage: stage.map(foundry_domain::StageKey::new),
                            result: result.map(TaskResult::new),
                            stage_variables: serde_json::from_value(stage_variables)
                                .map_err(|error| RepoError::Backend(error.to_string()))?,
                            task_variables: serde_json::from_value(task_variables)
                                .map_err(|error| RepoError::Backend(error.to_string()))?,
                            created_at,
                            updated_at,
                            completed_at,
                        },
                        dependencies_ready,
                    })
                },
            )
            .collect()
    }

    async fn dependencies(&self, task_id: Uuid) -> Result<Vec<TaskDependencyRow>, RepoError> {
        let rows: Vec<(Uuid, String, String, Option<String>)> = sqlx::query_as(
            "select d.depends_on_task_id, t.title, t.state, t.result \
             from task_dependencies d join tasks t on t.id = d.depends_on_task_id \
             where d.task_id = $1 order by d.depends_on_task_id",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(|(task_id, title, state, result)| {
                Ok(TaskDependencyRow {
                    task_id,
                    title,
                    state: parse_enum(&state)?,
                    result: result.map(TaskResult::new),
                })
            })
            .collect()
    }

    async fn actors(&self, task_id: Uuid) -> Result<Vec<TaskActorRow>, RepoError> {
        let rows: Vec<(String, Uuid, String, bool, Option<String>)> = sqlx::query_as(
            "select a.role_key, a.account_id, acc.display_name, a.active, a.work \
             from task_actors a join accounts acc on acc.id = a.account_id \
             where a.task_id = $1 order by a.active desc, a.role_key, acc.display_name",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(role_key, account_id, display_name, active, work)| TaskActorRow {
                    role_key: foundry_domain::RoleKey::new(role_key),
                    account_id,
                    display_name,
                    active,
                    work,
                },
            )
            .collect())
    }

    async fn comments(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskCommentRow>, RepoError> {
        let rows: Vec<(Uuid, Uuid, String, String, String, OffsetDateTime)> = sqlx::query_as(
            "select c.id, c.author_account_id, acc.display_name, acc.kind, c.body, c.created_at \
             from ( \
                 select * from task_comments where task_id = $1 \
                 order by created_at desc, id desc limit $2 offset $3 \
             ) c join accounts acc on acc.id = c.author_account_id \
             order by c.created_at, c.id",
        )
        .bind(task_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(
                |(id, author_account_id, author_display_name, kind, body, created_at)| {
                    Ok(TaskCommentRow {
                        id,
                        author_account_id,
                        author_display_name,
                        author_kind: parse_enum(&kind)?,
                        body,
                        created_at,
                    })
                },
            )
            .collect()
    }

    async fn operations(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, RepoError> {
        let rows: Vec<(Uuid, String, Value, Value, OffsetDateTime)> = sqlx::query_as(
            "select id, kind, actor, payload, occurred_at from events \
             where aggregate_id = $1 order by seq desc limit $2 offset $3",
        )
        .bind(task_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter()
            .map(|(id, kind, actor, payload, occurred_at)| {
                Ok(TaskOperation {
                    id,
                    kind: parse_enum(&kind)?,
                    actor: serde_json::from_value(actor)
                        .map_err(|error| RepoError::Backend(error.to_string()))?,
                    occurred_at,
                    payload,
                })
            })
            .collect()
    }

    async fn add_comment(&self, comment: &TaskCommentDAO) -> Result<(), RepoError> {
        sqlx::query(
            "insert into task_comments (id, task_id, author_account_id, body, created_at) \
             values ($1, $2, $3, $4, $5)",
        )
        .bind(comment.id)
        .bind(comment.task_id)
        .bind(comment.author_account_id)
        .bind(&comment.body)
        .bind(comment.created_at)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }
}

pub struct PostgresTaskDispatcher {
    pool: PgPool,
    workflows: Arc<dyn WorkflowCatalog>,
    /// Ready tasks already woken by this process.
    woken: Mutex<HashSet<Uuid>>,
}

impl PostgresTaskDispatcher {
    pub fn new(pool: PgPool, workflows: Arc<dyn WorkflowCatalog>) -> Self {
        Self {
            pool,
            workflows,
            woken: Mutex::new(HashSet::new()),
        }
    }
}

#[async_trait]
impl TaskDispatcher for PostgresTaskDispatcher {
    async fn process(
        &self,
        task_id: Uuid,
        command: Option<TaskCommand>,
    ) -> Result<(), TaskDispatchError> {
        let mut tx = self.pool.begin().await.map_err(db_error)?;
        let mut last_seq = lock_cursor(&mut tx, task_id).await?;
        let mut aggregate = match load_aggregate(&mut tx, task_id).await {
            Ok(aggregate) => aggregate,
            Err(RepoError::NotFound) => return Err(TaskDispatchError::NotFound(task_id)),
            Err(error) => return Err(error.into()),
        };

        if let Some(command) = command {
            self.apply_command(&mut tx, &mut aggregate, command).await?;
        }

        let kinds = trigger_kinds();
        loop {
            let rows: Vec<(i64, String, Value, OffsetDateTime)> = sqlx::query_as(
                "select seq, kind, payload, occurred_at from events \
                 where aggregate_id = $1 and seq > $2 and kind = any($3) \
                 order by seq limit 100",
            )
            .bind(task_id)
            .bind(last_seq)
            .bind(&kinds)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_error)?;
            if rows.is_empty() {
                break;
            }

            for (seq, kind, payload, _occurred_at) in rows {
                let kind: DomainEventKind = parse_enum(&kind).map_err(TaskDispatchError::from)?;
                let event = TaskEvent::from_parts(kind, &payload).map_err(task_error)?;
                let workflow = self
                    .workflows
                    .pinned(&foundry_domain::WorkflowVersionRef {
                        workflow_id: aggregate.task.workflow.clone(),
                        version: aggregate.task.workflow_version,
                    })
                    .await
                    .map_err(TaskDispatchError::from)?
                    .ok_or_else(|| {
                        TaskDispatchError::Rejected(format!(
                            "workflow {} version {} is not registered",
                            aggregate.task.workflow, aggregate.task.workflow_version
                        ))
                    })?;

                let TaskAggregate {
                    task,
                    actors,
                    dependencies,
                    mut children,
                } = aggregate;
                let mutation = foundry_domain::apply(
                    &*workflow,
                    TaskAggregate {
                        task,
                        actors,
                        dependencies,
                        children: std::mem::take(&mut children),
                    },
                    event,
                    OffsetDateTime::now_utc(),
                )
                .map_err(task_error)?;

                persist_mutation(&mut tx, &mutation).await?;
                children.extend(mutation.children.iter().map(|child| ChildState {
                    task_id: child.task.id,
                    workflow: child.task.workflow.clone(),
                    state: child.task.state,
                    result: child.task.result.clone(),
                }));
                aggregate = TaskAggregate {
                    task: mutation.task,
                    actors: mutation.actors,
                    dependencies: mutation.dependencies,
                    children,
                };
                last_seq = seq;
            }
        }

        sqlx::query(
            "update event_consumer_cursors set last_seq = $2, updated_at = now() \
             where consumer_id = $1 and partition_key = $3",
        )
        .bind(CONSUMER)
        .bind(last_seq)
        .bind(task_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(())
    }

    async fn dispatch_ready(&self) -> Result<usize, TaskDispatchError> {
        let ready: Vec<(Uuid, Uuid)> = sqlx::query_as(
            "select t.id, t.project_id from tasks t \
             where t.state = 'pending' \
               and not exists ( \
                   select 1 from task_dependencies d join tasks dep on dep.id = d.depends_on_task_id \
                   where d.task_id = t.id and dep.state <> 'completed' \
               ) \
             order by t.created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let ready_ids: HashSet<Uuid> = ready.iter().map(|(id, _)| *id).collect();
        let pending: Vec<(Uuid, Uuid)> = {
            let mut woken = self.woken.lock().expect("lock poisoned");
            woken.retain(|id| ready_ids.contains(id));
            ready
                .into_iter()
                .filter(|(id, _)| !woken.contains(id))
                .collect()
        };
        if pending.is_empty() {
            return Ok(0);
        }

        let mut tx = self.pool.begin().await.map_err(db_error)?;
        for (id, project_id) in &pending {
            let event = DomainEvent::new(
                DomainEventKind::DispatchRequested,
                *id,
                Actor::System,
                json!({}),
            );
            append_events(&mut tx, *project_id, &[event]).await?;
        }
        tx.commit().await.map_err(db_error)?;
        self.woken
            .lock()
            .expect("lock poisoned")
            .extend(pending.iter().map(|(id, _)| *id));
        Ok(pending.len())
    }

    async fn process_pending(&self, limit: i64) -> Result<usize, TaskDispatchError> {
        let kinds = trigger_kinds();
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "select distinct e.aggregate_id from events e \
             left join event_consumer_cursors c \
               on c.consumer_id = $1 and c.partition_key = e.aggregate_id::text \
             where e.kind = any($2) and e.seq > coalesce(c.last_seq, 0) \
             order by e.aggregate_id limit $3",
        )
        .bind(CONSUMER)
        .bind(&kinds)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;

        let mut processed = 0;
        for id in ids {
            match self.process(id, None).await {
                Ok(()) => processed += 1,
                Err(TaskDispatchError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(processed)
    }
}

impl PostgresTaskDispatcher {
    async fn apply_command(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        aggregate: &mut TaskAggregate,
        command: TaskCommand,
    ) -> Result<(), TaskDispatchError> {
        match command {
            TaskCommand::SendOperation {
                actor_id,
                operation,
                idempotency_key,
            } => {
                let duplicate: Option<i32> = sqlx::query_scalar(
                    "select 1 from task_operations where task_id = $1 and idempotency_key = $2",
                )
                .bind(aggregate.task.id)
                .bind(&idempotency_key)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_error)?;
                if duplicate.is_some() {
                    return Ok(());
                }
                if aggregate.task.state.is_terminal() {
                    return Err(TaskDispatchError::Rejected(
                        "completed tasks accept no operation".to_string(),
                    ));
                }

                let actor = load_actor(tx, actor_id)
                    .await?
                    .ok_or_else(|| TaskDispatchError::Forbidden("unknown actor".to_string()))?;
                sqlx::query(
                    "insert into task_operations \
                     (id, task_id, operation, actor_id, idempotency_key, created_at) \
                     values ($1, $2, $3, $4, $5, $6)",
                )
                .bind(Uuid::now_v7())
                .bind(aggregate.task.id)
                .bind(operation.as_str())
                .bind(actor_id)
                .bind(&idempotency_key)
                .bind(OffsetDateTime::now_utc())
                .execute(&mut **tx)
                .await
                .map_err(map_write_error)?;

                let event = DomainEvent::new(
                    DomainEventKind::OperationReceived,
                    aggregate.task.id,
                    actor,
                    json!({
                        "operation": operation,
                        "actor_id": actor_id,
                        "idempotency_key": idempotency_key,
                    }),
                );
                append_events(tx, aggregate.task.project_id, &[event]).await?;
            }
            TaskCommand::BindActor {
                role,
                account_id,
                bound_by,
            } => {
                if aggregate.task.state.is_terminal() {
                    return Err(TaskDispatchError::Rejected(
                        "completed tasks accept no role members".to_string(),
                    ));
                }
                let workflow = self
                    .workflows
                    .pinned(&foundry_domain::WorkflowVersionRef {
                        workflow_id: aggregate.task.workflow.clone(),
                        version: aggregate.task.workflow_version,
                    })
                    .await
                    .map_err(TaskDispatchError::from)?
                    .ok_or_else(|| {
                        TaskDispatchError::Rejected(format!(
                            "workflow {} version {} is not registered",
                            aggregate.task.workflow, aggregate.task.workflow_version
                        ))
                    })?;
                if workflow
                    .roles()
                    .iter()
                    .all(|spec| spec.name != role.as_str())
                {
                    return Err(TaskDispatchError::Rejected(format!(
                        "workflow {} has no role {role}",
                        aggregate.task.workflow
                    )));
                }

                let actor = load_actor(tx, bound_by).await?.unwrap_or(Actor::System);
                let inserted = sqlx::query(
                    "insert into task_actors (task_id, role_key, account_id, active, work) \
                     values ($1, $2, $3, false, null) on conflict do nothing",
                )
                .bind(aggregate.task.id)
                .bind(role.as_str())
                .bind(account_id)
                .execute(&mut **tx)
                .await
                .map_err(map_write_error)?;
                if inserted.rows_affected() == 0 {
                    return Ok(());
                }

                let event = DomainEvent::new(
                    DomainEventKind::ActorBound,
                    aggregate.task.id,
                    actor,
                    json!({ "role": role, "account_id": account_id }),
                );
                append_events(tx, aggregate.task.project_id, &[event]).await?;
                aggregate.actors.push(TaskActorDAO {
                    task_id: aggregate.task.id,
                    role_key: role,
                    account_id,
                    active: false,
                    work: None,
                });
            }
        }
        Ok(())
    }
}

async fn lock_cursor(
    tx: &mut Transaction<'_, Postgres>,
    task_id: Uuid,
) -> Result<i64, TaskDispatchError> {
    let partition_key = task_id.to_string();
    sqlx::query(
        "insert into event_consumer_cursors (consumer_id, partition_key) values ($1, $2) \
         on conflict do nothing",
    )
    .bind(CONSUMER)
    .bind(&partition_key)
    .execute(&mut **tx)
    .await
    .map_err(db_error)?;
    sqlx::query_scalar(
        "select last_seq from event_consumer_cursors \
         where consumer_id = $1 and partition_key = $2 for update",
    )
    .bind(CONSUMER)
    .bind(&partition_key)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_error)
}

async fn load_aggregate(
    tx: &mut Transaction<'_, Postgres>,
    task_id: Uuid,
) -> Result<TaskAggregate, RepoError> {
    let row: Option<TaskRow> = sqlx::query_as(
        "select id, project_id, parent_id, workflow, workflow_version, title, state, stage, \
         result, created_at, updated_at, completed_at from tasks where id = $1 for update",
    )
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(backend)?;
    let task = row
        .map(row_to_task)
        .transpose()?
        .ok_or(RepoError::NotFound)?;

    let dependency_rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select d.depends_on_task_id, t.state, t.result \
         from task_dependencies d join tasks t on t.id = d.depends_on_task_id \
         where d.task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    let dependencies = dependency_rows
        .into_iter()
        .map(|(task_id, state, result)| {
            Ok(DependencyState {
                task_id,
                state: parse_enum(&state)?,
                result: result.map(TaskResult::new),
            })
        })
        .collect::<Result<Vec<_>, RepoError>>()?;

    let children: Vec<ChildState> = sqlx::query_as::<_, (Uuid, String, String, Option<String>)>(
        "select id, workflow, state, result from tasks where parent_id = $1",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?
    .into_iter()
    .map(|(task_id, workflow, state, result)| {
        Ok(ChildState {
            task_id,
            workflow: WorkflowId::new(workflow),
            state: parse_enum(&state)?,
            result: result.map(TaskResult::new),
        })
    })
    .collect::<Result<Vec<_>, RepoError>>()?;

    let actor_rows: Vec<(String, Uuid, bool, Option<String>)> = sqlx::query_as(
        "select role_key, account_id, active, work from task_actors where task_id = $1",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(backend)?;
    let actors = actor_rows
        .into_iter()
        .map(|(role_key, account_id, active, work)| TaskActorDAO {
            task_id,
            role_key: foundry_domain::RoleKey::new(role_key),
            account_id,
            active,
            work,
        })
        .collect();

    Ok(TaskAggregate {
        task,
        actors,
        dependencies,
        children,
    })
}

async fn persist_mutation(
    tx: &mut Transaction<'_, Postgres>,
    mutation: &TaskMutation,
) -> Result<(), RepoError> {
    sqlx::query(
        "update tasks set state = $2, stage = $3, result = $4, stage_variables = $5, \
         task_variables = $6, completed_at = $7, updated_at = $8 where id = $1",
    )
    .bind(mutation.task.id)
    .bind(mutation.task.state.as_str())
    .bind(
        mutation
            .task
            .stage
            .as_ref()
            .map(foundry_domain::StageKey::as_str),
    )
    .bind(mutation.task.result.as_ref().map(TaskResult::as_str))
    .bind(
        serde_json::to_value(&mutation.task.stage_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
    )
    .bind(
        serde_json::to_value(&mutation.task.task_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
    )
    .bind(mutation.task.completed_at)
    .bind(mutation.task.updated_at)
    .execute(&mut **tx)
    .await
    .map_err(map_write_error)?;

    for actor in &mutation.actors {
        sqlx::query(
            "update task_actors set active = $4, work = $5 \
             where task_id = $1 and role_key = $2 and account_id = $3",
        )
        .bind(actor.task_id)
        .bind(actor.role_key.as_str())
        .bind(actor.account_id)
        .bind(actor.active)
        .bind(&actor.work)
        .execute(&mut **tx)
        .await
        .map_err(map_write_error)?;
    }

    for child in &mutation.children {
        let version =
            resolve_child_version(tx, &mutation.task.workflow, &child.task.workflow).await?;
        let mut child_task = child.task.clone();
        child_task.workflow_version = version;
        insert_task(tx, &child_task).await?;
        for dependency in &child.dependencies {
            insert_dependency(tx, child.task.id, *dependency).await?;
        }
    }

    append_events(tx, mutation.task.project_id, &mutation.events).await
}

/// Resolve a child workflow in the parent definition owner's scope.
///
/// Published versions are immutable, so the latest version is the highest
/// version number. The lookup shares the dispatcher transaction and therefore
/// sees the same catalog as the insert.
async fn resolve_child_version(
    tx: &mut Transaction<'_, Postgres>,
    parent: &WorkflowId,
    child: &WorkflowId,
) -> Result<u32, RepoError> {
    let owner: Option<Option<Uuid>> =
        sqlx::query_scalar("select owner_account_id from workflow_definitions where id = $1")
            .bind(parent.as_str())
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
    let owner = owner
        .ok_or_else(|| RepoError::Backend(format!("parent workflow {} is not defined", parent)))?;
    let version: Option<i32> = sqlx::query_scalar(
        "select max(v.version) from workflow_versions v \
         join workflow_definitions d on d.id = v.workflow_id \
         where v.workflow_id = $1 \
           and (d.owner_account_id is null or ($2::uuid is not null and d.owner_account_id = $2))",
    )
    .bind(child.as_str())
    .bind(owner)
    .fetch_one(&mut **tx)
    .await
    .map_err(backend)?;
    version
        .filter(|version| *version > 0)
        .map(|version| version as u32)
        .ok_or_else(|| {
            RepoError::Backend(format!(
                "child workflow {} has no published version in the parent scope",
                child
            ))
        })
}

pub(crate) async fn insert_task(
    tx: &mut Transaction<'_, Postgres>,
    task: &TaskDAO,
) -> Result<(), RepoError> {
    sqlx::query(
        "insert into tasks \
         (id, project_id, parent_id, workflow, workflow_version, title, state, stage, result, \
          stage_variables, task_variables, created_at, updated_at, completed_at) \
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(task.id)
    .bind(task.project_id)
    .bind(task.parent_id)
    .bind(task.workflow.as_str())
    .bind(task.workflow_version as i32)
    .bind(&task.title)
    .bind(task.state.as_str())
    .bind(task.stage.as_ref().map(foundry_domain::StageKey::as_str))
    .bind(task.result.as_ref().map(TaskResult::as_str))
    .bind(
        serde_json::to_value(&task.stage_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
    )
    .bind(
        serde_json::to_value(&task.task_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
    )
    .bind(task.created_at)
    .bind(task.updated_at)
    .bind(task.completed_at)
    .execute(&mut **tx)
    .await
    .map_err(map_write_error)?;
    Ok(())
}

async fn insert_dependency(
    tx: &mut Transaction<'_, Postgres>,
    task_id: Uuid,
    depends_on_task_id: Uuid,
) -> Result<(), RepoError> {
    sqlx::query(
        "insert into task_dependencies (task_id, depends_on_task_id) values ($1, $2) \
         on conflict do nothing",
    )
    .bind(task_id)
    .bind(depends_on_task_id)
    .execute(&mut **tx)
    .await
    .map_err(map_write_error)?;
    Ok(())
}

pub(crate) async fn append_events(
    tx: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    events: &[DomainEvent],
) -> Result<(), RepoError> {
    for event in events {
        let kind = enum_text(&event.kind)?;
        let actor = serde_json::to_value(&event.actor)
            .map_err(|error| RepoError::Backend(error.to_string()))?;
        sqlx::query(
            "insert into events (id, project_id, aggregate_id, kind, actor, payload, occurred_at) \
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(event.id)
        .bind(project_id)
        .bind(event.aggregate_id)
        .bind(kind)
        .bind(actor)
        .bind(&event.payload)
        .bind(event.occurred_at)
        .execute(&mut **tx)
        .await
        .map_err(map_write_error)?;
    }
    Ok(())
}

async fn load_actor(
    tx: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<Option<Actor>, RepoError> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("select handle, display_name, kind from accounts where id = $1")
            .bind(account_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(backend)?;
    row.map(|(handle, display_name, kind)| {
        let kind: AccountKind = parse_enum(&kind)?;
        Ok(match kind {
            AccountKind::Human => Actor::Human {
                id: handle,
                name: Some(display_name),
            },
            AccountKind::Agent => Actor::agent(handle, "agent"),
        })
    })
    .transpose()
}

fn row_to_task(row: TaskRow) -> Result<TaskDAO, RepoError> {
    let (
        id,
        project_id,
        parent_id,
        workflow,
        workflow_version,
        title,
        state,
        stage,
        result,
        stage_variables,
        task_variables,
        created_at,
        updated_at,
        completed_at,
    ) = row;
    Ok(TaskDAO {
        id,
        project_id,
        parent_id,
        workflow: WorkflowId::new(workflow),
        workflow_version: workflow_version.max(0) as u32,
        title,
        state: parse_enum(&state)?,
        stage: stage.map(foundry_domain::StageKey::new),
        result: result.map(TaskResult::new),
        stage_variables: serde_json::from_value(stage_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
        task_variables: serde_json::from_value(task_variables)
            .map_err(|error| RepoError::Backend(error.to_string()))?,
        created_at,
        updated_at,
        completed_at,
    })
}

fn task_error(error: TaskError) -> TaskDispatchError {
    match error {
        TaskError::Unauthorized(message) => TaskDispatchError::Forbidden(message),
        other => TaskDispatchError::Rejected(other.to_string()),
    }
}

fn db_error(error: sqlx::Error) -> TaskDispatchError {
    TaskDispatchError::Backend(error.to_string())
}
