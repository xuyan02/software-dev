use super::definition::{
    CreatedEffect, OperationSpec, RESULT_VARIABLE, TitleContext, VarSpec, WorkflowBody,
    WorkflowStage,
};
use crate::script::VariableValue;
use crate::{
    ChildState, DependencyState, DomainEvent, DomainEventKind, OperationName, RoleKey, StageKey,
    TaskActorDAO, TaskChild, TaskDAO, TaskError, TaskMutation, TaskResult, TaskState, WorkflowId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleKind {
    Human,
    Agent,
    Any,
}

/// A role declared by a workflow definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleSpec {
    pub name: String,
    pub kind: RoleKind,
    #[serde(default)]
    pub min_actors: u32,
    #[serde(default)]
    pub max_actors: Option<u32>,
}

impl RoleSpec {
    pub fn new(
        name: impl Into<String>,
        kind: RoleKind,
        min_actors: u32,
        max_actors: Option<u32>,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            min_actors,
            max_actors,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    Created,
    ActorBound {
        role: RoleKey,
        account_id: Uuid,
    },
    Operation {
        operation: OperationName,
        actor_id: Uuid,
    },
    TaskStateChanged {
        state: TaskState,
    },
    ChildCreated {
        child_id: Uuid,
        child_workflow: WorkflowId,
        child_title: String,
    },
    ChildStateChanged {
        child_id: Uuid,
        child_workflow: WorkflowId,
        child_state: TaskState,
        result: Option<TaskResult>,
    },
    DispatchRequested,
    /// Re-evaluate the current stage's transitions from persisted state. Used
    /// by the startup recovery pass; it runs no event action.
    Reevaluate,
}

impl TaskEvent {
    /// Decode a stored event payload into the workflow's trigger language.
    ///
    /// The payload schema is owned by [`crate::apply`] and the runtime; every
    /// consumer reads it through this function.
    pub fn from_domain(event: &DomainEvent) -> Result<Self, TaskError> {
        Self::from_parts(event.kind, &event.payload)
    }

    pub fn from_parts(kind: DomainEventKind, payload: &Value) -> Result<Self, TaskError> {
        let text = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_str)
                .ok_or_else(|| TaskError::Workflow(format!("{key} must be a string")))
        };
        let id = |key: &str| {
            text(key)?
                .parse::<Uuid>()
                .map_err(|error| TaskError::Workflow(format!("invalid {key}: {error}")))
        };
        let state = |key: &str| text(key)?.parse::<TaskState>().map_err(TaskError::Workflow);
        Ok(match kind {
            DomainEventKind::TaskCreated => Self::Created,
            DomainEventKind::OperationReceived => Self::Operation {
                operation: OperationName::new(text("operation")?),
                actor_id: id("actor_id")?,
            },
            DomainEventKind::ActorBound => Self::ActorBound {
                role: RoleKey::new(text("role")?),
                account_id: id("account_id")?,
            },
            DomainEventKind::TaskStateChanged => Self::TaskStateChanged {
                state: state("to")?,
            },
            DomainEventKind::ChildCreated => Self::ChildCreated {
                child_id: id("child_id")?,
                child_workflow: WorkflowId::new(text("workflow")?),
                child_title: text("title")?.to_string(),
            },
            DomainEventKind::ChildStateChanged => Self::ChildStateChanged {
                child_id: id("child_id")?,
                child_workflow: WorkflowId::new(text("child_workflow")?),
                child_state: state("child_state")?,
                result: payload
                    .get("result")
                    .and_then(Value::as_str)
                    .map(TaskResult::new),
            },
            DomainEventKind::DispatchRequested => Self::DispatchRequested,
            DomainEventKind::TaskReevaluated => Self::Reevaluate,
            other => {
                return Err(TaskError::Workflow(format!(
                    "{other:?} is not a task trigger event"
                )));
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveActor {
    pub role: RoleKey,
    pub account_id: Uuid,
    pub work: Option<String>,
}

/// The workflow aggregate while one trigger is being applied.
///
/// It owns the task, actors and children so stage entry can update variables,
/// active actors and children coherently. The interpreter is trusted code: it
/// can only call the methods here, never perform I/O.
pub struct WorkflowContext<'a> {
    workflow: &'a dyn TaskWorkflow,
    task: TaskDAO,
    actors: Vec<TaskActorDAO>,
    dependencies: Vec<DependencyState>,
    children: Vec<ChildState>,
    created_children: Vec<TaskChild>,
    created_issues: Vec<crate::TaskIssueCreate>,
    events: Vec<DomainEvent>,
    now: OffsetDateTime,
}

impl<'a> WorkflowContext<'a> {
    pub fn new(
        workflow: &'a dyn TaskWorkflow,
        task: TaskDAO,
        actors: Vec<TaskActorDAO>,
        dependencies: Vec<DependencyState>,
        children: Vec<ChildState>,
        now: OffsetDateTime,
    ) -> Self {
        Self {
            workflow,
            task,
            actors,
            dependencies,
            children,
            created_children: Vec::new(),
            created_issues: Vec::new(),
            events: Vec::new(),
            now,
        }
    }

    pub fn workflow(&self) -> &'a dyn TaskWorkflow {
        self.workflow
    }

    pub fn task(&self) -> &TaskDAO {
        &self.task
    }

    pub fn actors(&self) -> &[TaskActorDAO] {
        &self.actors
    }

    pub fn dependencies(&self) -> &[DependencyState] {
        &self.dependencies
    }

    pub fn children(&self) -> &[ChildState] {
        &self.children
    }

    pub fn active_actors(&self, role: &str) -> BTreeSet<Uuid> {
        self.actors
            .iter()
            .filter(|actor| actor.active && actor.role_key.as_str() == role)
            .map(|actor| actor.account_id)
            .collect()
    }

    pub fn stage_variables(&self) -> &std::collections::BTreeMap<String, VariableValue> {
        &self.task.stage_variables
    }

    pub fn stage_variables_mut(
        &mut self,
    ) -> &mut std::collections::BTreeMap<String, VariableValue> {
        &mut self.task.stage_variables
    }

    pub fn task_variables(&self) -> &std::collections::BTreeMap<String, VariableValue> {
        &self.task.task_variables
    }

    pub fn task_variables_mut(&mut self) -> &mut std::collections::BTreeMap<String, VariableValue> {
        &mut self.task.task_variables
    }

    /// Initialize task variables from their declared initial values and the
    /// reserved `result` variable. Called once, when the task is created.
    pub fn init_task_variables(&mut self, vars: &std::collections::BTreeMap<String, VarSpec>) {
        self.task.task_variables = vars
            .iter()
            .map(|(name, spec)| (name.clone(), spec.initial_value()))
            .collect();
        self.task.task_variables.insert(
            RESULT_VARIABLE.to_string(),
            VariableValue::String(String::new()),
        );
    }

    /// Recompute the current stage's active actors without touching variables,
    /// `created` effects or transitions. Used for `ActorBound`.
    pub fn refresh_active_actors(&mut self) -> Result<(), TaskError> {
        let Some(name) = self.task.stage.as_ref().map(|stage| stage.to_string()) else {
            return Ok(());
        };
        let workflow = self.workflow;
        let Some(stage) = workflow.stage(&name) else {
            return Ok(());
        };
        let roles = stage.active_roles.clone();
        self.activate_roles(&roles)
    }

    pub fn set_state(&mut self, state: TaskState) -> Result<(), TaskError> {
        if state.is_terminal() {
            return Err(TaskError::InvalidEffect(
                "set_state cannot set completed; use a transition to ended".to_string(),
            ));
        }
        match (self.task.state, state) {
            (TaskState::Pending, TaskState::Pending)
            | (TaskState::InProgress, TaskState::InProgress) => return Ok(()),
            (TaskState::Pending, TaskState::InProgress) => {
                if !self
                    .dependencies
                    .iter()
                    .all(|dependency| dependency.state.is_terminal())
                {
                    return Err(TaskError::DependencyBlocked);
                }
                self.require_bound_agent_roles()?;
            }
            _ => {
                return Err(TaskError::InvalidEffect(format!(
                    "cannot move from {:?} to {:?}",
                    self.task.state, state
                )));
            }
        }

        let from = self.task.state;
        self.task.state = state;
        self.task.updated_at = self.now;
        self.events.push(system_event(
            DomainEventKind::TaskStateChanged,
            self.task.id,
            self.now,
            json!({ "from": from, "to": state }),
        ));
        self.notify_parent(state, None);
        Ok(())
    }

    /// A task may enter `in_progress` only after every agent role that requires
    /// actors has them bound. Later-stage roles are bound before the task
    /// starts, so this check applies to the whole workflow, not just the
    /// initial stage.
    fn require_bound_agent_roles(&self) -> Result<(), TaskError> {
        let mut unbound = Vec::new();
        for spec in self.workflow.roles() {
            if spec.kind != RoleKind::Agent || spec.min_actors == 0 {
                continue;
            }
            let bound = self
                .actors
                .iter()
                .filter(|actor| actor.role_key.as_str() == spec.name)
                .count();
            if bound < spec.min_actors as usize {
                unbound.push(spec.name.clone());
            }
        }
        if unbound.is_empty() {
            Ok(())
        } else {
            Err(TaskError::UnboundRoles(unbound.join(", ")))
        }
    }

    /// Enter a stage by name, running its entry semantics.
    ///
    /// It rebuilds stage variables from their initial values, reactivates the
    /// stage's roles and runs its `created` effects. Entering the current stage
    /// is a valid self-transition. Task variables are left untouched.
    pub fn enter_stage(&mut self, name: &str) -> Result<(), TaskError> {
        let workflow = self.workflow;
        let Some(stage) = workflow.stages().iter().find(|stage| stage.name == name) else {
            return Err(TaskError::UnknownStage(name.to_string()));
        };

        self.set_stage(name);
        self.task.updated_at = self.now;
        self.task.stage_variables = stage
            .vars
            .iter()
            .map(|(name, spec)| (name.clone(), spec.initial_value()))
            .collect();
        self.activate_roles(&stage.active_roles)?;

        let created = stage.created.clone();
        for effect in created {
            match effect {
                CreatedEffect::SetState(state) => self.set_state(state)?,
                CreatedEffect::CreateChild(spec) => {
                    let title = spec.title.render(&TitleContext {
                        task_title: &self.task.title,
                        children: &self.children,
                    })?;
                    self.create_child(spec.workflow.as_str(), title);
                }
            }
        }
        Ok(())
    }

    fn set_stage(&mut self, name: &str) {
        let target = StageKey::new(name);
        if self.task.stage.as_ref() == Some(&target) {
            return;
        }
        let from = self.task.stage.replace(target.clone());
        self.events.push(system_event(
            DomainEventKind::TaskStageChanged,
            self.task.id,
            self.now,
            json!({ "from": from, "to": target }),
        ));
    }

    fn activate_roles(&mut self, roles: &[String]) -> Result<(), TaskError> {
        for spec in self.workflow.roles() {
            let count = self
                .actors
                .iter()
                .filter(|actor| {
                    actor.active
                        && actor.role_key.as_str() == spec.name
                        && roles.iter().any(|role| role == &spec.name)
                })
                .count();
            if let Some(max) = spec.max_actors
                && count > max as usize
            {
                return Err(TaskError::InvalidEffect(format!(
                    "role {} accepts at most {max} active actors",
                    spec.name
                )));
            }
        }

        let previous: Vec<TaskActorDAO> = self.actors.clone();
        for actor in self.actors.iter_mut() {
            actor.active = false;
            actor.work = None;
        }
        for actor in self.actors.iter_mut() {
            if roles.iter().any(|role| role == actor.role_key.as_str()) {
                actor.active = true;
            }
        }

        for (before, after) in previous.iter().zip(self.actors.iter()) {
            if before.active == after.active && before.work == after.work {
                continue;
            }
            let kind = if after.active {
                DomainEventKind::ActorActivated
            } else {
                DomainEventKind::ActorDeactivated
            };
            self.events.push(system_event(
                kind,
                after.task_id,
                self.now,
                json!({
                    "role": after.role_key,
                    "account_id": after.account_id,
                    "work": after.work,
                }),
            ));
        }
        Ok(())
    }

    /// Complete the task through the virtual `ended` stage.
    pub fn complete_task(&mut self, result: Option<TaskResult>) -> Result<(), TaskError> {
        self.complete(result)
    }

    fn complete(&mut self, result: Option<TaskResult>) -> Result<(), TaskError> {
        if !self
            .dependencies
            .iter()
            .all(|dependency| dependency.state.is_terminal())
        {
            return Err(TaskError::DependencyBlocked);
        }
        if !self.children.iter().all(|child| child.state.is_terminal()) {
            return Err(TaskError::ChildrenNotCompleted);
        }

        for actor in self.actors.iter_mut().filter(|actor| actor.active) {
            actor.active = false;
            actor.work = None;
            self.events.push(system_event(
                DomainEventKind::ActorDeactivated,
                self.task.id,
                self.now,
                json!({
                    "role": actor.role_key,
                    "account_id": actor.account_id,
                    "work": Value::Null,
                }),
            ));
        }

        self.task.state = TaskState::Completed;
        self.task.result = result.clone();
        self.task.completed_at = Some(self.now);

        self.events.push(system_event(
            DomainEventKind::TaskCompleted,
            self.task.id,
            self.now,
            json!({ "result": self.task.result }),
        ));
        self.notify_parent(TaskState::Completed, result);
        Ok(())
    }

    fn create_child(&mut self, workflow: &str, title: String) -> Uuid {
        let id = Uuid::now_v7();
        let child = TaskDAO {
            id,
            project_id: self.task.project_id,
            parent_id: Some(self.task.id),
            workflow: WorkflowId::new(workflow),
            // The adapter pins the child's published version before persisting.
            workflow_version: 0,
            title: title.clone(),
            state: TaskState::Pending,
            stage: None,
            result: None,
            stage_variables: Default::default(),
            task_variables: Default::default(),
            created_at: self.now,
            updated_at: self.now,
            completed_at: None,
        };

        self.events.push(system_event(
            DomainEventKind::TaskCreated,
            id,
            self.now,
            json!({
                "workflow": workflow,
                "title": title,
                "parent_id": self.task.id,
                "dependencies": [],
            }),
        ));
        self.events.push(system_event(
            DomainEventKind::ChildCreated,
            self.task.id,
            self.now,
            json!({
                "child_id": id,
                "workflow": workflow,
                "title": title,
            }),
        ));

        self.children.push(ChildState {
            task_id: id,
            workflow: WorkflowId::new(workflow),
            state: TaskState::Pending,
            result: None,
        });
        self.created_children.push(TaskChild {
            task: child,
            dependencies: Vec::new(),
        });
        id
    }

    pub(crate) fn create_issue(
        &mut self,
        mut issue: crate::TaskIssueCreate,
    ) -> Result<(), TaskError> {
        let link_task_id = if let Some(task) = issue.task.take() {
            self.create_child(task.workflow.as_str(), task.title)
        } else {
            self.task.id
        };
        issue.link_task_id = Some(link_task_id);
        self.created_issues.push(issue);
        Ok(())
    }

    fn notify_parent(&mut self, state: TaskState, result: Option<TaskResult>) {
        let Some(parent_id) = self.task.parent_id else {
            return;
        };
        self.events.push(system_event(
            DomainEventKind::ChildStateChanged,
            parent_id,
            self.now,
            json!({
                "child_id": self.task.id,
                "child_workflow": self.task.workflow,
                "child_state": state,
                "result": result,
            }),
        ));
    }

    pub fn emit(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    pub(crate) fn into_mutation(self) -> TaskMutation {
        TaskMutation {
            task: self.task,
            actors: self.actors,
            dependencies: self.dependencies,
            children: self.created_children,
            issues: self.created_issues,
            events: self.events,
        }
    }
}

fn system_event(
    kind: DomainEventKind,
    aggregate_id: Uuid,
    now: OffsetDateTime,
    payload: Value,
) -> DomainEvent {
    DomainEvent {
        id: Uuid::now_v7(),
        kind,
        aggregate_id,
        actor: crate::Actor::System,
        payload,
        occurred_at: now,
    }
}

/// A workflow definition the runtime can execute.
///
/// Implementations must be deterministic and do no I/O. The read-only methods
/// default to the declarative body when one is present; test doubles that only
/// need custom effects can leave [`Self::body`] as `None` and override
/// [`Self::on_event`].
pub trait TaskWorkflow: Send + Sync {
    fn id(&self) -> &str;

    fn body(&self) -> Option<&WorkflowBody> {
        None
    }

    fn roles(&self) -> &[RoleSpec] {
        self.body().map_or(&[], |body| body.roles.as_slice())
    }

    fn stages(&self) -> &[WorkflowStage] {
        self.body().map_or(&[], |body| body.stages.as_slice())
    }

    fn initial_stage(&self) -> Option<&str> {
        self.body().and_then(|body| body.initial_stage.as_deref())
    }

    fn stage(&self, name: &str) -> Option<&WorkflowStage> {
        self.stages().iter().find(|stage| stage.name == name)
    }

    fn operation<'b>(&'b self, stage: &str, name: &str) -> Option<&'b OperationSpec> {
        self.stage(stage).and_then(|stage| {
            stage
                .operations
                .iter()
                .find(|operation| operation.name == name)
        })
    }

    fn has_operation(&self, stage: &str, name: &str) -> bool {
        self.operation(stage, name).is_some()
    }

    fn operation_names(&self, stage: &str) -> Vec<OperationName> {
        self.stage(stage)
            .map(|stage| {
                stage
                    .operations
                    .iter()
                    .map(|operation| OperationName::new(operation.name.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn on_event(&self, ctx: &mut WorkflowContext<'_>, event: TaskEvent) -> Result<(), TaskError>;
}
