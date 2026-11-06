use crate::{
    ChildState, DependencyState, OperationName, TaskActorDAO, TaskDAO, TaskError, TaskEvent,
    TaskMutation, TaskWorkflow, WorkflowContext,
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
            events: Vec::new(),
        });
    }

    match &event {
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
