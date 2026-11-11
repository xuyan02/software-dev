use crate::{
    ACCEPT_OPERATION, ChildState, DependencyState, OperationName, REJECT_OPERATION,
    START_OPERATION, TaskActorDAO, TaskDAO, TaskError, TaskEvent, TaskMutation, TaskResult,
    TaskState, TaskWorkflow, WorkflowContext,
};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone)]
pub struct TaskAggregate {
    pub task: TaskDAO,
    pub actors: Vec<TaskActorDAO>,
    pub dependencies: Vec<DependencyState>,
    pub children: Vec<ChildState>,
}

/// Apply one trigger event to a Task aggregate.
///
/// This function is pure: the caller persists the returned mutation, the child
/// tasks it contains and the appended events in one transaction.
pub fn apply(
    workflow: &dyn TaskWorkflow,
    aggregate: TaskAggregate,
    event: TaskEvent,
    now: OffsetDateTime,
) -> Result<TaskMutation, TaskError> {
    let TaskAggregate {
        task,
        actors,
        dependencies,
        children,
    } = aggregate;

    // A trigger event that arrives after completion is a no-op. The command
    // path rejects direct commands on a completed task before reaching here.
    if task.state.is_terminal() {
        return Ok(TaskMutation {
            task,
            actors,
            dependencies,
            children: Vec::new(),
            issues: Vec::new(),
            review_links: Vec::new(),
            events: Vec::new(),
        });
    }

    match &event {
        TaskEvent::Operation { operation, .. } if operation.as_str() == START_OPERATION => {
            // Platform-level start: no workflow operation, no role check. The
            // API restricts this operation to administrators.
            let mut ctx = WorkflowContext::new(workflow, task, actors, dependencies, children, now);
            ctx.set_state(TaskState::InProgress)?;
            return Ok(ctx.into_mutation());
        }
        TaskEvent::Operation { operation, .. } if operation.as_str() == ACCEPT_OPERATION => {
            // Platform-level acceptance: requirements document project-layer
            // approval is the human acceptance, so close the linked
            // clarification task with result `approved` without requiring a
            // task-actor binding.
            let mut ctx = WorkflowContext::new(workflow, task, actors, dependencies, children, now);
            ctx.complete_task(Some(TaskResult::new("approved")))?;
            return Ok(ctx.into_mutation());
        }
        TaskEvent::Operation { operation, .. } if operation.as_str() == REJECT_OPERATION => {
            // Platform-level rejection from a human review: return to the
            // workflow's initial stage (the drafting stage). Annotations carry
            // the concrete revision feedback.
            let target = workflow
                .body()
                .and_then(|body| {
                    body.stages
                        .iter()
                        .find(|stage| Some(stage.name.as_str()) == task.stage.as_deref())
                        .and_then(|stage| stage.reject_to.clone())
                        .or_else(|| body.initial_stage.clone())
                })
                .unwrap_or_else(|| "work".to_string());
            let mut ctx = WorkflowContext::new(workflow, task, actors, dependencies, children, now);
            ctx.enter_stage(&target)?;
            return Ok(ctx.into_mutation());
        }
        TaskEvent::Operation {
            operation,
            actor_id,
        } => authorize_operation(workflow, &task, &actors, operation, *actor_id)?,
        TaskEvent::ActorBound { role, .. } => {
            if workflow
                .roles()
                .iter()
                .all(|spec| spec.name != role.as_str())
            {
                return Err(TaskError::UnknownRole(role.to_string()));
            }
        }
        _ => {}
    }

    let mut ctx = WorkflowContext::new(workflow, task, actors, dependencies, children, now);
    workflow.on_event(&mut ctx, event)?;
    Ok(ctx.into_mutation())
}

fn authorize_operation(
    workflow: &dyn TaskWorkflow,
    task: &TaskDAO,
    actors: &[TaskActorDAO],
    operation: &OperationName,
    actor_id: Uuid,
) -> Result<(), TaskError> {
    let stage = task
        .stage
        .as_ref()
        .map(|stage| stage.as_str())
        .unwrap_or("");
    let Some(spec) = workflow.operation(stage, operation.as_str()) else {
        return Err(TaskError::InvalidOperation(operation.to_string()));
    };

    let allowed = actors.iter().any(|actor| {
        actor.account_id == actor_id
            && actor.active
            && spec
                .roles
                .iter()
                .any(|role| role == actor.role_key.as_str())
    });
    if allowed {
        Ok(())
    } else {
        Err(TaskError::Unauthorized(format!(
            "actor {actor_id} is not an active actor for operation {operation:?}"
        )))
    }
}
