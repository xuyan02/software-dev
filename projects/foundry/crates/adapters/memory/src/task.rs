//! In-memory Task repository and `task-workflow` consumer.
//!
//! The store holds the aggregate, its projections and the event log under one
//! lock, which gives the same serialization guarantee the Postgres adapter gets
//! from the consumer cursor row.

use crate::InMemoryStore;
use async_trait::async_trait;
use foundry_app::{
    RepoError, TaskActorRow, TaskCommand, TaskCommentRow, TaskDependencyRow, TaskDispatchError,
    TaskDispatcher, TaskFilter, TaskListRow, TaskRepository,
};
use foundry_domain::{
    Account, AccountKind, Actor, ChildState, DependencyState, DomainEvent, DomainEventKind,
    TaskActorDAO, TaskAggregate, TaskCommentDAO, TaskDAO, TaskEvent, TaskMutation, TaskOperation,
    TaskState,
};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Default)]
pub(crate) struct MemoryTasks {
    tasks: HashMap<Uuid, TaskDAO>,
    dependencies: HashMap<Uuid, Vec<Uuid>>,
    actors: HashMap<Uuid, Vec<TaskActorDAO>>,
    comments: Vec<TaskCommentDAO>,
    inputs: HashSet<(Uuid, String)>,
    events: Vec<(u64, DomainEvent)>,
    cursors: HashMap<Uuid, u64>,
    seq: u64,
}

impl MemoryTasks {
    pub(crate) fn insert(
        &mut self,
        task: &TaskDAO,
        dependencies: &[Uuid],
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        if self.tasks.contains_key(&task.id) {
            return Err(RepoError::Backend(format!(
                "task {} already exists",
                task.id
            )));
        }
        self.tasks.insert(task.id, task.clone());
        if !dependencies.is_empty() {
            self.dependencies.insert(task.id, dependencies.to_vec());
        }
        self.append(events);
        Ok(())
    }

    fn append(&mut self, events: &[DomainEvent]) {
        for event in events {
            self.seq += 1;
            self.events.push((self.seq, event.clone()));
        }
    }

    fn is_ready(&self, task_id: Uuid) -> bool {
        self.dependencies
            .get(&task_id)
            .is_none_or(|dependencies| self.dependencies_ready(dependencies))
    }

    fn dependencies_ready(&self, dependencies: &[Uuid]) -> bool {
        dependencies.iter().all(|id| {
            self.tasks
                .get(id)
                .is_some_and(|task| task.state.is_terminal())
        })
    }

    fn aggregate(&self, task_id: Uuid) -> Option<TaskAggregate> {
        let task = self.tasks.get(&task_id)?.clone();
        let dependencies = self
            .dependencies
            .get(&task_id)
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let task = self.tasks.get(id)?;
                Some(DependencyState {
                    task_id: *id,
                    state: task.state,
                    result: task.result.clone(),
                })
            })
            .collect();
        let children = self
            .tasks
            .values()
            .filter(|child| child.parent_id == Some(task_id))
            .map(|child| ChildState {
                task_id: child.id,
                workflow: child.workflow.clone(),
                state: child.state,
                result: child.result.clone(),
            })
            .collect();
        Some(TaskAggregate {
            task,
            actors: self.actors.get(&task_id).cloned().unwrap_or_default(),
            dependencies,
            children,
        })
    }

    fn persist(&mut self, mutation: &TaskMutation) {
        self.tasks.insert(mutation.task.id, mutation.task.clone());
        for actor in &mutation.actors {
            let actors = self.actors.entry(actor.task_id).or_default();
            match actors.iter_mut().find(|existing| {
                existing.role_key == actor.role_key && existing.account_id == actor.account_id
            }) {
                Some(existing) => *existing = actor.clone(),
                None => actors.push(actor.clone()),
            }
        }
        for child in &mutation.children {
            self.tasks.insert(child.task.id, child.task.clone());
            self.dependencies
                .insert(child.task.id, child.dependencies.clone());
        }
        self.append(&mutation.events);
    }

    fn pending(&self, task_id: Uuid, last_seq: u64) -> Vec<(u64, DomainEvent)> {
        self.events
            .iter()
            .filter(|(seq, event)| {
                *seq > last_seq && event.aggregate_id == task_id && is_trigger(event.kind)
            })
            .cloned()
            .collect()
    }
}

fn is_trigger(kind: DomainEventKind) -> bool {
    matches!(
        kind,
        DomainEventKind::TaskCreated
            | DomainEventKind::OperationReceived
            | DomainEventKind::ActorBound
            | DomainEventKind::TaskStateChanged
            | DomainEventKind::ChildCreated
            | DomainEventKind::ChildStateChanged
            | DomainEventKind::DispatchRequested
    )
}

#[async_trait]
impl TaskRepository for InMemoryStore {
    async fn insert(
        &self,
        task: &TaskDAO,
        dependencies: &[Uuid],
        events: &[DomainEvent],
    ) -> Result<(), RepoError> {
        self.tasks
            .write()
            .expect("lock poisoned")
            .insert(task, dependencies, events)
    }

    async fn get(&self, id: Uuid) -> Result<Option<TaskDAO>, RepoError> {
        Ok(self
            .tasks
            .read()
            .expect("lock poisoned")
            .tasks
            .get(&id)
            .cloned())
    }

    async fn list(
        &self,
        project_id: Uuid,
        filter: TaskFilter,
    ) -> Result<Vec<TaskListRow>, RepoError> {
        let state = self.tasks.read().expect("lock poisoned");
        let mut rows: Vec<TaskListRow> = state
            .tasks
            .values()
            .filter(|task| task.project_id == project_id)
            .filter(|task| filter.state.is_none_or(|value| task.state == value))
            .filter(|task| {
                filter
                    .stage
                    .as_ref()
                    .is_none_or(|stage| task.stage.as_ref() == Some(stage))
            })
            .filter(|task| {
                filter
                    .parent_id
                    .is_none_or(|parent_id| task.parent_id == Some(parent_id))
            })
            .filter(|task| {
                filter
                    .text
                    .as_deref()
                    .is_none_or(|text| task.title.to_lowercase().contains(&text.to_lowercase()))
            })
            .filter(|task| {
                let actors = state.actors.get(&task.id);
                filter.role.as_ref().is_none_or(|role| {
                    actors.is_some_and(|actors| actors.iter().any(|actor| actor.role_key == *role))
                })
            })
            .filter(|task| {
                filter.active_actor.is_none_or(|account_id| {
                    state.actors.get(&task.id).is_some_and(|actors| {
                        actors
                            .iter()
                            .any(|actor| actor.account_id == account_id && actor.active)
                    })
                })
            })
            .map(|task| TaskListRow {
                task: task.clone(),
                dependencies_ready: state.is_ready(task.id),
            })
            .filter(|row| {
                filter.dispatch_state.is_none_or(|dispatch| {
                    dispatch
                        == foundry_domain::TaskDispatchState::derive(
                            row.task.state,
                            row.dependencies_ready,
                        )
                })
            })
            .collect();
        rows.sort_by_key(|row| row.task.created_at);
        rows.truncate(filter.limit.unwrap_or(100).max(0) as usize);
        Ok(rows)
    }

    async fn dependencies(&self, task_id: Uuid) -> Result<Vec<TaskDependencyRow>, RepoError> {
        let state = self.tasks.read().expect("lock poisoned");
        Ok(state
            .dependencies
            .get(&task_id)
            .into_iter()
            .flatten()
            .filter_map(|id| {
                let task = state.tasks.get(id)?;
                Some(TaskDependencyRow {
                    task_id: *id,
                    title: task.title.clone(),
                    state: task.state,
                    result: task.result.clone(),
                })
            })
            .collect())
    }

    async fn actors(&self, task_id: Uuid) -> Result<Vec<TaskActorRow>, RepoError> {
        let state = self.tasks.read().expect("lock poisoned");
        let accounts = self.accounts.read().expect("lock poisoned");
        let mut rows: Vec<TaskActorRow> = state
            .actors
            .get(&task_id)
            .into_iter()
            .flatten()
            .map(|actor| TaskActorRow {
                role_key: actor.role_key.clone(),
                account_id: actor.account_id,
                display_name: accounts
                    .get(&actor.account_id)
                    .map(|account| account.display_name.clone())
                    .unwrap_or_default(),
                active: actor.active,
                work: actor.work.clone(),
            })
            .collect();
        rows.sort_by(|left, right| {
            right
                .active
                .cmp(&left.active)
                .then_with(|| left.role_key.cmp(&right.role_key))
                .then_with(|| left.display_name.cmp(&right.display_name))
        });
        Ok(rows)
    }

    async fn comments(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskCommentRow>, RepoError> {
        let state = self.tasks.read().expect("lock poisoned");
        let accounts = self.accounts.read().expect("lock poisoned");
        let mut comments: Vec<&TaskCommentDAO> = state
            .comments
            .iter()
            .filter(|comment| comment.task_id == task_id)
            .collect();
        comments.sort_by_key(|comment| (comment.created_at, comment.id));
        let start = comments
            .len()
            .saturating_sub((offset + limit).max(0) as usize);
        let end = comments.len().saturating_sub(offset.max(0) as usize);
        Ok(comments[start..end]
            .iter()
            .map(|comment| TaskCommentRow {
                id: comment.id,
                author_account_id: comment.author_account_id,
                author_display_name: accounts
                    .get(&comment.author_account_id)
                    .map(|account| account.display_name.clone())
                    .unwrap_or_default(),
                author_kind: accounts
                    .get(&comment.author_account_id)
                    .map(|account| account.kind)
                    .unwrap_or_default(),
                body: comment.body.clone(),
                created_at: comment.created_at,
            })
            .collect())
    }

    async fn operations(
        &self,
        task_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TaskOperation>, RepoError> {
        let state = self.tasks.read().expect("lock poisoned");
        let mut events: Vec<&(u64, DomainEvent)> = state
            .events
            .iter()
            .filter(|(_, event)| event.aggregate_id == task_id)
            .collect();
        events.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
        Ok(events
            .into_iter()
            .skip(offset.max(0) as usize)
            .take(limit.max(0) as usize)
            .map(|(_, event)| TaskOperation {
                id: event.id,
                kind: event.kind,
                actor: event.actor.clone(),
                occurred_at: event.occurred_at,
                payload: event.payload.clone(),
            })
            .collect())
    }

    async fn add_comment(&self, comment: &TaskCommentDAO) -> Result<(), RepoError> {
        self.tasks
            .write()
            .expect("lock poisoned")
            .comments
            .push(comment.clone());
        Ok(())
    }
}

#[async_trait]
impl TaskDispatcher for InMemoryTaskDispatcher {
    async fn process(
        &self,
        task_id: Uuid,
        command: Option<TaskCommand>,
    ) -> Result<(), TaskDispatchError> {
        self.store.process_partition(task_id, command)
    }

    async fn dispatch_ready(&self) -> Result<usize, TaskDispatchError> {
        let ready: Vec<Uuid> = {
            let state = self.store.tasks.read().expect("lock poisoned");
            let mut ids: Vec<Uuid> = state
                .tasks
                .values()
                .filter(|task| task.state == TaskState::Pending)
                .filter(|task| state.is_ready(task.id))
                .map(|task| task.id)
                .collect();
            ids.sort_by_key(|id| state.tasks[id].created_at);
            ids
        };

        let ready: HashSet<Uuid> = ready.into_iter().collect();
        let mut woken = self.woken.lock().expect("lock poisoned");
        woken.retain(|id| ready.contains(id));
        let ids: Vec<Uuid> = ready.difference(&woken).copied().collect();
        if ids.is_empty() {
            return Ok(0);
        }

        let mut state = self.store.tasks.write().expect("lock poisoned");
        for id in &ids {
            state.append(&[DomainEvent::new(
                DomainEventKind::DispatchRequested,
                *id,
                Actor::System,
                json!({}),
            )]);
        }
        drop(state);
        woken.extend(ids.iter().copied());
        Ok(ids.len())
    }

    async fn process_pending(&self, limit: i64) -> Result<usize, TaskDispatchError> {
        let ids: Vec<Uuid> = {
            let state = self.store.tasks.read().expect("lock poisoned");
            let mut ids: Vec<Uuid> = state
                .events
                .iter()
                .filter(|(_, event)| is_trigger(event.kind))
                .filter(|(seq, event)| {
                    state
                        .cursors
                        .get(&event.aggregate_id)
                        .is_none_or(|cursor| seq > cursor)
                })
                .map(|(_, event)| event.aggregate_id)
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids.truncate(limit.max(0) as usize);
            ids
        };
        let mut processed = 0;
        for id in ids {
            match self.store.process_partition(id, None) {
                Ok(()) => processed += 1,
                Err(TaskDispatchError::NotFound(_)) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(processed)
    }
}

pub struct InMemoryTaskDispatcher {
    store: Arc<InMemoryStore>,
    /// Ready tasks already woken by this process.
    woken: Mutex<HashSet<Uuid>>,
}

impl InMemoryTaskDispatcher {
    pub fn new(store: Arc<InMemoryStore>) -> Self {
        Self {
            store,
            woken: Mutex::new(HashSet::new()),
        }
    }
}

impl InMemoryStore {
    fn process_partition(
        &self,
        task_id: Uuid,
        command: Option<TaskCommand>,
    ) -> Result<(), TaskDispatchError> {
        let mut stored = self.tasks.write().expect("lock poisoned");
        // Work on a copy so a rejected command or a failing workflow leaves no
        // input row, event or cursor advance behind.
        let mut state = stored.clone();
        let mut aggregate = state
            .aggregate(task_id)
            .ok_or(TaskDispatchError::NotFound(task_id))?;

        if let Some(command) = command {
            self.apply_command(&mut state, &mut aggregate, command)?;
        }

        let mut last_seq = state.cursors.get(&task_id).copied().unwrap_or(0);
        loop {
            let pending = state.pending(task_id, last_seq);
            if pending.is_empty() {
                break;
            }
            for (seq, event) in pending {
                let workflow = self
                    .pinned_workflow(&aggregate.task.workflow, aggregate.task.workflow_version)
                    .ok_or_else(|| {
                        TaskDispatchError::Rejected(format!(
                            "workflow {} version {} is not registered",
                            aggregate.task.workflow, aggregate.task.workflow_version
                        ))
                    })?;
                let trigger = TaskEvent::from_domain(&event).map_err(task_error)?;
                let mut mutation = foundry_domain::apply(
                    &*workflow,
                    aggregate,
                    trigger,
                    OffsetDateTime::now_utc(),
                )
                .map_err(task_error)?;
                for child in &mut mutation.children {
                    let reference = self
                        .child_version(&mutation.task.workflow, &child.task.workflow)
                        .ok_or_else(|| {
                            TaskDispatchError::Rejected(format!(
                                "child workflow {} is not published in the parent scope",
                                child.task.workflow
                            ))
                        })?;
                    child.task.workflow_version = reference.version;
                }
                state.persist(&mutation);
                aggregate = aggregate_of(mutation);
                last_seq = seq;
            }
        }
        state.cursors.insert(task_id, last_seq);
        *stored = state;
        Ok(())
    }

    fn apply_command(
        &self,
        state: &mut MemoryTasks,
        aggregate: &mut TaskAggregate,
        command: TaskCommand,
    ) -> Result<(), TaskDispatchError> {
        let task_id = aggregate.task.id;
        match command {
            TaskCommand::SendOperation {
                actor_id,
                operation,
                idempotency_key,
            } => {
                if state.inputs.contains(&(task_id, idempotency_key.clone())) {
                    return Ok(());
                }
                if aggregate.task.state.is_terminal() {
                    return Err(TaskDispatchError::Rejected(
                        "completed tasks accept no operation".to_string(),
                    ));
                }
                let actor = self
                    .task_actor(actor_id)
                    .ok_or_else(|| TaskDispatchError::Forbidden("unknown actor".to_string()))?;
                state.inputs.insert((task_id, idempotency_key.clone()));
                state.append(&[DomainEvent::new(
                    DomainEventKind::OperationReceived,
                    task_id,
                    actor,
                    json!({
                        "operation": operation,
                        "actor_id": actor_id,
                        "idempotency_key": idempotency_key,
                    }),
                )]);
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
                    .pinned_workflow(&aggregate.task.workflow, aggregate.task.workflow_version)
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
                if aggregate
                    .actors
                    .iter()
                    .any(|actor| actor.role_key == role && actor.account_id == account_id)
                {
                    return Ok(());
                }

                let actor = self.task_actor(bound_by).unwrap_or(Actor::System);
                let bound = TaskActorDAO {
                    task_id,
                    role_key: role.clone(),
                    account_id,
                    active: false,
                    work: None,
                };
                state.actors.entry(task_id).or_default().push(bound.clone());
                aggregate.actors.push(bound);
                state.append(&[DomainEvent::new(
                    DomainEventKind::ActorBound,
                    task_id,
                    actor,
                    json!({ "role": role, "account_id": account_id }),
                )]);
            }
        }
        Ok(())
    }

    fn task_actor(&self, account_id: Uuid) -> Option<Actor> {
        let accounts = self.accounts.read().expect("lock poisoned");
        accounts.get(&account_id).map(account_actor)
    }
}

fn account_actor(account: &Account) -> Actor {
    match account.kind {
        AccountKind::Human => Actor::Human {
            id: account.handle.clone(),
            name: Some(account.display_name.clone()),
        },
        AccountKind::Agent => Actor::agent(account.handle.clone(), "agent"),
    }
}

fn aggregate_of(mutation: TaskMutation) -> TaskAggregate {
    TaskAggregate {
        task: mutation.task,
        actors: mutation.actors,
        dependencies: mutation.dependencies,
        children: mutation
            .children
            .into_iter()
            .map(|child| ChildState {
                task_id: child.task.id,
                workflow: child.task.workflow,
                state: child.task.state,
                result: child.task.result,
            })
            .collect(),
    }
}

fn task_error(error: foundry_domain::TaskError) -> TaskDispatchError {
    match error {
        foundry_domain::TaskError::Unauthorized(message) => TaskDispatchError::Forbidden(message),
        other => TaskDispatchError::Rejected(other.to_string()),
    }
}
