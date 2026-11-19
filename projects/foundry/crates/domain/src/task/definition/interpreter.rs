use std::collections::BTreeMap;

use super::ast::{ENDED_STAGE, EventKind, RESULT_VARIABLE, WorkflowBody};
use crate::script::{
    EventAccess, EventData, RuntimeContext, ScriptIssue, ScriptScope, VarType, VariableValue,
    compile_action, compile_condition, eval_condition, run_action,
};
use crate::{
    TaskError, TaskEvent, TaskResult, TaskState, TaskWorkflow, WorkflowContext, WorkflowStage,
};

/// Executes a declarative [`WorkflowBody`].
///
/// The interpreter is trusted code: a definition can only produce the effects
/// exposed by [`WorkflowContext`], never arbitrary I/O or state mutation.
pub struct DefinitionWorkflow {
    id: String,
    body: WorkflowBody,
}

impl DefinitionWorkflow {
    pub fn new(id: impl Into<String>, body: WorkflowBody) -> Self {
        Self {
            id: id.into(),
            body,
        }
    }

    pub fn body_ref(&self) -> &WorkflowBody {
        &self.body
    }
}

impl TaskWorkflow for DefinitionWorkflow {
    fn id(&self) -> &str {
        &self.id
    }

    fn body(&self) -> Option<&WorkflowBody> {
        Some(&self.body)
    }

    fn on_event(&self, ctx: &mut WorkflowContext<'_>, event: TaskEvent) -> Result<(), TaskError> {
        match event {
            TaskEvent::Created => {
                ctx.init_task_variables(&self.body.vars);
                let Some(initial) = self.body.initial_stage.as_deref() else {
                    return Ok(());
                };
                ctx.enter_stage(initial)?;
                let Some(stage) = self.stage(initial) else {
                    return Err(TaskError::UnknownStage(initial.to_string()));
                };
                if let Some(action) = event_action(stage, EventKind::TaskCreated) {
                    run_script(
                        ctx,
                        &self.body,
                        stage,
                        action,
                        EventAccess::Event(EventKind::TaskCreated.field_set()),
                        None,
                    )?;
                }
                evaluate_transitions(ctx, &self.body, stage)?;
            }
            TaskEvent::Operation {
                operation,
                actor_id,
            } => {
                let Some(name) = ctx.task().stage.clone() else {
                    return Err(TaskError::InvalidOperation(operation.to_string()));
                };
                let Some(stage) = self.stage(name.as_str()) else {
                    return Err(TaskError::UnknownStage(name.to_string()));
                };
                if ctx.task().state == TaskState::Pending {
                    ctx.set_state(TaskState::InProgress)?;
                }
                let spec = stage
                    .operations
                    .iter()
                    .find(|spec| spec.name == operation.as_str())
                    .ok_or_else(|| TaskError::InvalidOperation(operation.to_string()))?;
                run_script(
                    ctx,
                    &self.body,
                    stage,
                    &spec.action,
                    EventAccess::Operation,
                    Some(EventData::Operation { actor: actor_id }),
                )?;
                evaluate_transitions(ctx, &self.body, stage)?;
            }
            TaskEvent::ActorBound { .. } => {
                ctx.refresh_active_actors()?;
            }
            TaskEvent::TaskStateChanged { state } => {
                if state == TaskState::Completed {
                    return Ok(());
                }
                self.handle_event(
                    ctx,
                    EventKind::TaskStateChanged,
                    Some(EventData::TaskStateChanged {
                        state: state.as_str(),
                    }),
                )?;
            }
            TaskEvent::ChildCreated {
                child_id,
                child_workflow,
                child_title,
            } => {
                let child_id = child_id.to_string();
                self.handle_event(
                    ctx,
                    EventKind::ChildCreated,
                    Some(EventData::ChildCreated {
                        id: &child_id,
                        workflow: child_workflow.as_str(),
                        title: &child_title,
                    }),
                )?;
            }
            TaskEvent::ChildStateChanged {
                child_id,
                child_workflow,
                child_state,
                result,
            } => {
                let child_id = child_id.to_string();
                let result = result.as_ref().map(|result| result.as_str());
                self.handle_event(
                    ctx,
                    EventKind::ChildStateChanged,
                    Some(EventData::ChildStateChanged {
                        id: &child_id,
                        workflow: child_workflow.as_str(),
                        state: child_state.as_str(),
                        result,
                    }),
                )?;
            }
            TaskEvent::DispatchRequested => {}
            TaskEvent::Reevaluate => {
                if ctx.task().state != TaskState::InProgress {
                    return Ok(());
                }
                let Some(name) = ctx.task().stage.clone() else {
                    return Ok(());
                };
                let Some(stage) = self.stage(name.as_str()) else {
                    return Ok(());
                };
                evaluate_transitions(ctx, &self.body, stage)?;
            }
        }
        Ok(())
    }
}

impl DefinitionWorkflow {
    fn handle_event(
        &self,
        ctx: &mut WorkflowContext<'_>,
        kind: EventKind,
        event: Option<EventData<'_>>,
    ) -> Result<(), TaskError> {
        let Some(name) = ctx.task().stage.clone() else {
            return Ok(());
        };
        let Some(stage) = self.stage(name.as_str()) else {
            return Ok(());
        };
        let Some(action) = event_action(stage, kind) else {
            return Ok(());
        };
        run_script(
            ctx,
            &self.body,
            stage,
            action,
            EventAccess::Event(kind.field_set()),
            event,
        )?;
        evaluate_transitions(ctx, &self.body, stage)
    }
}

fn event_action(stage: &WorkflowStage, kind: EventKind) -> Option<&str> {
    stage
        .events
        .iter()
        .find(|event| event.event == kind)
        .map(|event| event.action.as_str())
}

/// Variable types visible to a script: task variables, the reserved `result`
/// variable and the current stage's variables.
fn script_variables(body: &WorkflowBody, stage: &WorkflowStage) -> BTreeMap<String, VarType> {
    let mut variables: BTreeMap<String, VarType> = body
        .vars
        .iter()
        .map(|(name, spec)| (name.clone(), spec.var_type))
        .collect();
    variables.insert(RESULT_VARIABLE.to_string(), VarType::String);
    for (name, spec) in &stage.vars {
        variables.insert(name.clone(), spec.var_type);
    }
    variables
}

fn merged_values(ctx: &WorkflowContext<'_>) -> BTreeMap<String, VariableValue> {
    let mut values = ctx.task_variables().clone();
    for (name, value) in ctx.stage_variables() {
        values.insert(name.clone(), value.clone());
    }
    values
}

/// Write script results back to the scope each variable belongs to. Writes to
/// stage variables survive until the task leaves the stage; writes to task
/// variables survive stage changes.
fn store_values(
    ctx: &mut WorkflowContext<'_>,
    body: &WorkflowBody,
    stage: &WorkflowStage,
    values: &BTreeMap<String, VariableValue>,
) {
    let mut task_values = ctx.task_variables().clone();
    let mut stage_values = ctx.stage_variables().clone();
    for (name, value) in values {
        if name == RESULT_VARIABLE || body.vars.contains_key(name) {
            task_values.insert(name.clone(), value.clone());
        }
        if stage.vars.contains_key(name) {
            stage_values.insert(name.clone(), value.clone());
        }
    }
    *ctx.task_variables_mut() = task_values;
    *ctx.stage_variables_mut() = stage_values;
}

fn run_script(
    ctx: &mut WorkflowContext<'_>,
    body: &WorkflowBody,
    stage: &WorkflowStage,
    source: &str,
    access: EventAccess,
    event: Option<EventData<'_>>,
) -> Result<(), TaskError> {
    let variables = script_variables(body, stage);
    let roles = role_names(ctx.workflow());
    let known = |_: &str| true;
    let compiled = {
        let scope = ScriptScope {
            variables: &variables,
            roles: &roles,
            active_roles: &stage.active_roles,
            known_workflow: &known,
            event: access,
        };
        compile_action(source, &scope).map_err(compile_error)?
    };

    let mut values = merged_values(ctx);
    let result = {
        let active = |role: &str| ctx.active_actors(role);
        let context = RuntimeContext {
            task_state: ctx.task().state.as_str(),
            task_title: &ctx.task().title,
            active_actors: &active,
            children: ctx.children(),
            event,
        };
        run_action(&compiled, &mut values, &context)
    };
    store_values(ctx, body, stage, &values);
    let issues = result.map_err(|error| TaskError::Workflow(error.to_string()))?;
    for issue in issues {
        ctx.create_issue(issue)?;
    }
    Ok(())
}

fn evaluate_transitions(
    ctx: &mut WorkflowContext<'_>,
    body: &WorkflowBody,
    stage: &WorkflowStage,
) -> Result<(), TaskError> {
    let variables = script_variables(body, stage);
    let roles = role_names(ctx.workflow());
    let known = |_: &str| true;
    let values = merged_values(ctx);
    let mut matched: Option<usize> = None;
    for (index, transition) in stage.transitions.iter().enumerate() {
        let compiled = {
            let scope = ScriptScope {
                variables: &variables,
                roles: &roles,
                active_roles: &stage.active_roles,
                known_workflow: &known,
                event: EventAccess::Condition,
            };
            compile_condition(&transition.on, &scope).map_err(compile_error)?
        };
        let active = |role: &str| ctx.active_actors(role);
        let context = RuntimeContext {
            task_state: ctx.task().state.as_str(),
            task_title: &ctx.task().title,
            active_actors: &active,
            children: ctx.children(),
            event: None,
        };
        let is_match = eval_condition(&compiled, &values, &context)
            .map_err(|error| TaskError::Workflow(error.to_string()))?;
        if is_match {
            matched = Some(index);
            break;
        }
    }

    let Some(index) = matched else {
        return Ok(());
    };
    let transition = &stage.transitions[index];
    if !transition.action.trim().is_empty() {
        run_script(
            ctx,
            body,
            stage,
            &transition.action,
            EventAccess::Condition,
            None,
        )?;
    }
    if transition.to == ENDED_STAGE {
        let result = match ctx.task_variables().get(RESULT_VARIABLE) {
            Some(VariableValue::String(value)) if !value.is_empty() => {
                Some(TaskResult::new(value.clone()))
            }
            _ => None,
        };
        return ctx.complete_task(result);
    }
    ctx.enter_stage(&transition.to)
}

fn role_names(workflow: &dyn TaskWorkflow) -> Vec<String> {
    workflow
        .roles()
        .iter()
        .map(|role| role.name.clone())
        .collect()
}

fn compile_error(issues: Vec<ScriptIssue>) -> TaskError {
    TaskError::Workflow(
        issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect::<Vec<_>>()
            .join("; "),
    )
}
