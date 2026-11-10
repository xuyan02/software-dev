use super::definition::{DefinitionWorkflow, WorkflowBody, builtin_workflows, validate};
use crate::task::script::VariableValue;
use crate::{
    ACCEPT_OPERATION, ChildState, DependencyState, DomainEventKind, OperationName,
    REJECT_OPERATION, RoleKey, START_OPERATION, StageKey, TaskActorDAO, TaskAggregate, TaskDAO,
    TaskError, TaskEvent,
    TaskMutation, TaskResult, TaskState, TaskWorkflow, WorkflowContext, WorkflowId, apply,
};
use time::OffsetDateTime;
use uuid::Uuid;

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn builtin(id: &str) -> DefinitionWorkflow {
    builtin_workflows()
        .into_iter()
        .find(|workflow| workflow.id() == id)
        .unwrap_or_else(|| panic!("missing builtin workflow {id}"))
}

fn task(workflow: &str, state: TaskState, stage: Option<&str>) -> TaskDAO {
    let mut task = TaskDAO::new(
        Uuid::now_v7(),
        None,
        WorkflowId::new(workflow),
        1,
        "Task",
        now(),
    );
    task.state = state;
    task.stage = stage.map(StageKey::new);
    task
}

fn actor(task_id: Uuid, role: &str, account_id: Uuid, active: bool) -> TaskActorDAO {
    TaskActorDAO {
        task_id,
        role_key: RoleKey::new(role),
        account_id,
        active,
        work: None,
    }
}

fn aggregate(
    task: TaskDAO,
    actors: Vec<TaskActorDAO>,
    dependencies: Vec<DependencyState>,
    children: Vec<ChildState>,
) -> TaskAggregate {
    TaskAggregate {
        task,
        actors,
        dependencies,
        children,
    }
}

fn child_state(child: &crate::TaskChild) -> ChildState {
    ChildState {
        task_id: child.task.id,
        workflow: child.task.workflow.clone(),
        state: child.task.state,
        result: child.task.result.clone(),
    }
}

fn aggregate_of(mutation: TaskMutation) -> TaskAggregate {
    TaskAggregate {
        task: mutation.task,
        actors: mutation.actors,
        dependencies: mutation.dependencies,
        children: mutation.children.iter().map(child_state).collect(),
    }
}

/// Apply a mutation on top of an existing aggregate, keeping every previously
/// known child and appending the ones the mutation created.
fn absorb(state: TaskAggregate, mutation: TaskMutation) -> TaskAggregate {
    let mut children = state.children;
    children.extend(mutation.children.iter().map(child_state));
    TaskAggregate {
        task: mutation.task,
        actors: mutation.actors,
        dependencies: mutation.dependencies,
        children,
    }
}

fn operation(
    workflow: &dyn TaskWorkflow,
    state: TaskAggregate,
    name: &str,
    actor_id: Uuid,
) -> TaskMutation {
    apply(
        workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new(name),
            actor_id,
        },
        now(),
    )
    .unwrap()
}

fn child_changed(
    workflow: &dyn TaskWorkflow,
    mut state: TaskAggregate,
    child_id: Uuid,
    child_workflow: &str,
    result: &str,
) -> (TaskAggregate, TaskMutation) {
    if let Some(child) = state
        .children
        .iter_mut()
        .find(|child| child.task_id == child_id)
    {
        child.state = TaskState::Completed;
        child.result = Some(TaskResult::new(result));
    }
    let mutation = apply(
        workflow,
        state.clone(),
        TaskEvent::ChildStateChanged {
            child_id,
            child_workflow: WorkflowId::new(child_workflow),
            child_state: TaskState::Completed,
            result: Some(TaskResult::new(result)),
        },
        now(),
    )
    .unwrap();
    (state, mutation)
}

#[test]
fn builtin_definitions_parse_validate_and_round_trip() {
    let workflows = builtin_workflows();
    let mut ids: Vec<&str> = workflows.iter().map(|workflow| workflow.id()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "architecture_design",
            "iteration",
            "project",
            "project_brief_writing",
            "project_review",
            "release",
            "requirements_analysis",
            "requirements_discovery",
            "roadmap",
        ]
    );

    let is_known = |id: &str| ids.contains(&id);
    for workflow in &workflows {
        let body = workflow.body_ref();
        let validation = validate(body, &is_known);
        assert!(
            validation.errors.is_empty(),
            "{}: {:?}",
            workflow.id(),
            validation.errors
        );
        let json = serde_json::to_string(body).unwrap();
        let round_trip: WorkflowBody = serde_json::from_str(&json).unwrap();
        assert_eq!(&round_trip, body);
    }
}

#[test]
fn project_root_starts_and_creates_the_first_child() {
    let workflow = builtin("project");
    let root = task("project", TaskState::Pending, None);
    let mutation = apply(
        &workflow,
        aggregate(root, vec![], vec![], vec![]),
        TaskEvent::Created,
        now(),
    )
    .unwrap();

    assert_eq!(mutation.task.state, TaskState::InProgress);
    assert_eq!(mutation.task.stage.as_deref(), Some("goal"));
    assert_eq!(mutation.children.len(), 1);
    assert_eq!(
        mutation.children[0].task.workflow.as_str(),
        "project_brief_writing"
    );
    assert_eq!(mutation.children[0].task.parent_id, Some(mutation.task.id));
    assert!(mutation.events.iter().any(|event| {
        event.aggregate_id == mutation.children[0].task.id
            && event.kind == DomainEventKind::TaskCreated
    }));
    assert!(
        mutation
            .events
            .iter()
            .any(|event| event.kind == DomainEventKind::ChildCreated)
    );
}

#[test]
fn leaf_workflow_moves_through_reviews_to_completed() {
    let workflow = builtin("project_brief_writing");
    let parent = Uuid::now_v7();
    let mut leaf = task("project_brief_writing", TaskState::Pending, None);
    leaf.parent_id = Some(parent);
    let task_id = leaf.id;
    let clarifier = Uuid::now_v7();
    let brief_writer = Uuid::now_v7();
    let style_reviewer = Uuid::now_v7();
    let content_reviewer = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let mut state = aggregate(
        leaf,
        vec![
            actor(task_id, "clarifier", clarifier, false),
            actor(task_id, "brief_writer", brief_writer, false),
            actor(task_id, "style_reviewer", style_reviewer, false),
            actor(task_id, "content_reviewer", content_reviewer, false),
            actor(task_id, "acceptor", acceptor, false),
        ],
        vec![],
        vec![],
    );

    let created = apply(&workflow, state, TaskEvent::Created, now()).unwrap();
    assert_eq!(created.task.state, TaskState::Pending);
    assert_eq!(created.task.stage.as_deref(), Some("clarify"));
    assert!(
        created
            .actors
            .iter()
            .any(|a| a.account_id == clarifier && a.active)
    );
    state = aggregate_of(created);

    let started = operation(&workflow, state, "started", clarifier);
    assert_eq!(started.task.state, TaskState::InProgress);
    state = aggregate_of(started);

    // Clarification is finished; the brief writer takes over.
    let clarified = operation(&workflow, state, "completed", clarifier);
    assert_eq!(clarified.task.stage.as_deref(), Some("brief"));
    assert!(
        clarified
            .actors
            .iter()
            .any(|a| a.account_id == brief_writer && a.active)
    );
    assert!(
        !clarified
            .actors
            .iter()
            .any(|a| a.account_id == clarifier && a.active)
    );
    state = aggregate_of(clarified);

    // The brief is written and submitted for style review.
    let completed = operation(&workflow, state, "completed", brief_writer);
    assert_eq!(completed.task.stage.as_deref(), Some("style_review"));
    assert!(
        completed
            .actors
            .iter()
            .any(|a| a.account_id == style_reviewer && a.active)
    );
    assert!(
        !completed
            .actors
            .iter()
            .any(|a| a.account_id == brief_writer && a.active)
    );
    state = aggregate_of(completed);

    state = aggregate_of(operation(&workflow, state, "approved", style_reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("content_review"));
    assert!(
        state
            .actors
            .iter()
            .any(|a| a.account_id == content_reviewer && a.active)
    );

    state = aggregate_of(operation(&workflow, state, "approved", content_reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("acceptance"));
    assert!(
        state
            .actors
            .iter()
            .any(|a| a.account_id == acceptor && a.active)
    );

    let approved = operation(&workflow, state, "approved", acceptor);
    assert_eq!(approved.task.state, TaskState::Completed);
    assert_eq!(approved.task.result, Some(TaskResult::new("approved")));
    assert!(
        approved.task.stage_variables.is_empty(),
        "stage variables are cleared when the task exits the last stage"
    );
    assert!(approved.actors.iter().all(|a| !a.active));
    assert!(approved.events.iter().any(|event| {
        event.kind == DomainEventKind::ChildStateChanged && event.aggregate_id == parent
    }));
}

#[test]
fn reevaluate_applies_a_pending_transition_from_persisted_variables() {
    let workflow = builtin("project_brief_writing");
    let mut leaf = task("project_brief_writing", TaskState::InProgress, Some("brief"));
    leaf.stage_variables.insert(
        "outcome".to_string(),
        VariableValue::String("completed".to_string()),
    );
    let task_id = leaf.id;
    let brief_writer = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "brief_writer", brief_writer, true)],
        vec![],
        vec![],
    );

    let mutation = apply(&workflow, state, TaskEvent::Reevaluate, now()).unwrap();

    assert_eq!(mutation.task.stage.as_deref(), Some("style_review"));
    assert!(
        mutation
            .events
            .iter()
            .any(|event| event.kind == DomainEventKind::TaskStageChanged)
    );
}

#[test]
fn stage_entry_bumps_task_updated_at() {
    let workflow = builtin("project_brief_writing");
    let mut leaf = task("project_brief_writing", TaskState::InProgress, Some("brief"));
    leaf.stage_variables.insert(
        "outcome".to_string(),
        VariableValue::String("completed".to_string()),
    );
    let old = time::OffsetDateTime::now_utc() - time::Duration::seconds(60);
    leaf.updated_at = old;
    let task_id = leaf.id;
    let brief_writer = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "brief_writer", brief_writer, true)],
        vec![],
        vec![],
    );

    let mutation = apply(&workflow, state, TaskEvent::Reevaluate, now()).unwrap();

    assert_eq!(mutation.task.stage.as_deref(), Some("style_review"));
    assert!(mutation.task.updated_at > old);
}

#[test]
fn brief_review_rejection_returns_to_the_brief_writer() {
    let workflow = builtin("project_brief_writing");
    let leaf = task(
        "project_brief_writing",
        TaskState::InProgress,
        Some("style_review"),
    );
    let task_id = leaf.id;
    let brief_writer = Uuid::now_v7();
    let style_reviewer = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![
            actor(task_id, "brief_writer", brief_writer, false),
            actor(task_id, "style_reviewer", style_reviewer, true),
        ],
        vec![],
        vec![],
    );

    let rejected = operation(&workflow, state, "rejected", style_reviewer);
    assert_eq!(rejected.task.stage.as_deref(), Some("brief"));
    assert!(
        rejected
            .actors
            .iter()
            .any(|a| a.account_id == brief_writer && a.active)
    );
    assert!(
        !rejected
            .actors
            .iter()
            .any(|a| a.account_id == style_reviewer && a.active)
    );
}

#[test]
fn acceptance_rejection_returns_to_the_brief_writer() {
    let workflow = builtin("project_brief_writing");
    let leaf = task(
        "project_brief_writing",
        TaskState::InProgress,
        Some("acceptance"),
    );
    let task_id = leaf.id;
    let brief_writer = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![
            actor(task_id, "brief_writer", brief_writer, false),
            actor(task_id, "acceptor", acceptor, true),
        ],
        vec![],
        vec![],
    );

    let rejected = operation(&workflow, state, "rejected", acceptor);
    assert_eq!(rejected.task.stage.as_deref(), Some("brief"));
    assert_eq!(rejected.task.state, TaskState::InProgress);
    assert!(
        rejected
            .actors
            .iter()
            .any(|a| a.account_id == brief_writer && a.active)
    );
    assert!(
        !rejected
            .actors
            .iter()
            .any(|a| a.account_id == acceptor && a.active)
    );
}

#[test]
fn human_reject_uses_the_stage_rejection_transition() {
    let workflow = builtin("project_brief_writing");
    let leaf = task(
        "project_brief_writing",
        TaskState::InProgress,
        Some("acceptance"),
    );
    let state = aggregate(leaf, vec![], vec![], vec![]);

    let rejected = operation(&workflow, state, REJECT_OPERATION, Uuid::now_v7());

    assert_eq!(rejected.task.stage.as_deref(), Some("brief"));
    assert_eq!(rejected.task.state, TaskState::InProgress);
}

#[test]
fn dependencies_gate_the_start_transition() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::Pending, Some("clarify"));
    let task_id = leaf.id;
    let clarifier = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "clarifier", clarifier, true)],
        vec![DependencyState {
            task_id: Uuid::now_v7(),
            state: TaskState::Pending,
            result: None,
        }],
        vec![],
    );

    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("started"),
            actor_id: clarifier,
        },
        now(),
    )
    .unwrap_err();
    assert_eq!(error, TaskError::DependencyBlocked);
}

#[test]
fn parent_cannot_complete_above_an_active_child() {
    let workflow = builtin("project_brief_writing");
    let leaf = task(
        "project_brief_writing",
        TaskState::InProgress,
        Some("acceptance"),
    );
    let task_id = leaf.id;
    let acceptor = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "acceptor", acceptor, true)],
        vec![],
        vec![ChildState {
            task_id: Uuid::now_v7(),
            workflow: WorkflowId::new("iteration"),
            state: TaskState::Pending,
            result: None,
        }],
    );

    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("approved"),
            actor_id: acceptor,
        },
        now(),
    )
    .unwrap_err();
    assert_eq!(error, TaskError::ChildrenNotCompleted);
}

#[test]
fn unauthorized_operation_is_rejected_before_the_workflow_runs() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::InProgress, Some("clarify"));
    let outsider = Uuid::now_v7();
    let state = aggregate(leaf, vec![], vec![], vec![]);

    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("completed"),
            actor_id: outsider,
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::Unauthorized(_)));
}

#[test]
fn authorization_is_stage_level_and_requires_an_active_actor() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::Pending, Some("clarify"));
    let task_id = leaf.id;
    let clarifier = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![
            actor(task_id, "clarifier", clarifier, true),
            actor(task_id, "acceptor", acceptor, false),
        ],
        vec![],
        vec![],
    );

    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("started"),
            actor_id: acceptor,
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::Unauthorized(_)));
}

#[test]
fn undeclared_operation_is_rejected() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::InProgress, Some("clarify"));
    let task_id = leaf.id;
    let clarifier = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "clarifier", clarifier, true)],
        vec![],
        vec![],
    );

    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("unknown"),
            actor_id: clarifier,
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::InvalidOperation(_)));
}

#[test]
fn architecture_review_flow_round_trips_through_review() {
    let workflow = builtin("architecture_design");
    let leaf = task("architecture_design", TaskState::Pending, None);
    let task_id = leaf.id;
    let architect = Uuid::now_v7();
    let reviewer = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let mut state = aggregate(
        leaf,
        vec![
            actor(task_id, "architect", architect, false),
            actor(task_id, "reviewer", reviewer, false),
            actor(task_id, "acceptor", acceptor, false),
        ],
        vec![],
        vec![],
    );

    state = aggregate_of(apply(&workflow, state, TaskEvent::Created, now()).unwrap());
    assert_eq!(state.task.stage.as_deref(), Some("work"));
    assert!(
        state
            .actors
            .iter()
            .any(|a| a.account_id == architect && a.active)
    );

    state = aggregate_of(operation(&workflow, state, "started", architect));
    state = aggregate_of(operation(&workflow, state, "completed", architect));
    assert_eq!(state.task.stage.as_deref(), Some("review"));
    assert!(
        state
            .actors
            .iter()
            .any(|a| a.account_id == reviewer && a.active)
    );

    state = aggregate_of(operation(&workflow, state, "review_rejected", reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("work"));

    state = aggregate_of(operation(&workflow, state, "completed", architect));
    state = aggregate_of(operation(&workflow, state, "review_approved", reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("awaiting_acceptance"));

    let approved = operation(&workflow, state, "approved", acceptor);
    assert_eq!(approved.task.state, TaskState::Completed);
    assert_eq!(approved.task.result.as_deref(), Some("approved"));
}

#[test]
fn iteration_flow_handles_changes_and_rejection() {
    let workflow = builtin("iteration");
    let leaf = task("iteration", TaskState::Pending, None);
    let task_id = leaf.id;
    let planner = Uuid::now_v7();
    let implementer = Uuid::now_v7();
    let reviewer = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let mut state = aggregate(
        leaf,
        vec![
            actor(task_id, "planner", planner, false),
            actor(task_id, "implementer", implementer, false),
            actor(task_id, "reviewer", reviewer, false),
            actor(task_id, "acceptor", acceptor, false),
        ],
        vec![],
        vec![],
    );

    state = aggregate_of(apply(&workflow, state, TaskEvent::Created, now()).unwrap());
    state = aggregate_of(operation(&workflow, state, "started", planner));
    state = aggregate_of(operation(&workflow, state, "plan_ready", planner));
    assert_eq!(state.task.stage.as_deref(), Some("implementation"));
    assert!(
        state
            .actors
            .iter()
            .any(|a| a.account_id == implementer && a.active)
    );

    state = aggregate_of(operation(&workflow, state, "started", implementer));
    state = aggregate_of(operation(
        &workflow,
        state,
        "implementation_complete",
        implementer,
    ));
    assert_eq!(state.task.stage.as_deref(), Some("review"));

    state = aggregate_of(operation(&workflow, state, "changes_requested", reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("implementation"));

    state = aggregate_of(operation(
        &workflow,
        state,
        "implementation_complete",
        implementer,
    ));
    state = aggregate_of(operation(&workflow, state, "review_approved", reviewer));
    assert_eq!(state.task.stage.as_deref(), Some("acceptance"));

    state = aggregate_of(operation(&workflow, state, "rejected", acceptor));
    assert_eq!(state.task.stage.as_deref(), Some("implementation"));

    state = aggregate_of(operation(
        &workflow,
        state,
        "implementation_complete",
        implementer,
    ));
    state = aggregate_of(operation(&workflow, state, "review_approved", reviewer));
    let completed = operation(&workflow, state, "approved", acceptor);
    assert_eq!(completed.task.state, TaskState::Completed);
    assert_eq!(completed.task.result.as_deref(), Some("approved"));
}

#[test]
fn release_flow_returns_to_candidate_until_healthy() {
    let workflow = builtin("release");
    let leaf = task("release", TaskState::Pending, None);
    let task_id = leaf.id;
    let release_manager = Uuid::now_v7();
    let verifier = Uuid::now_v7();
    let approver = Uuid::now_v7();
    let deployer = Uuid::now_v7();
    let observer = Uuid::now_v7();
    let mut state = aggregate(
        leaf,
        vec![
            actor(task_id, "release_manager", release_manager, false),
            actor(task_id, "verifier", verifier, false),
            actor(task_id, "approver", approver, false),
            actor(task_id, "deployer", deployer, false),
            actor(task_id, "observer", observer, false),
        ],
        vec![],
        vec![],
    );

    state = aggregate_of(apply(&workflow, state, TaskEvent::Created, now()).unwrap());
    state = aggregate_of(operation(&workflow, state, "started", release_manager));
    state = aggregate_of(operation(&workflow, state, "completed", release_manager));
    state = aggregate_of(operation(&workflow, state, "verification_failed", verifier));
    assert_eq!(state.task.stage.as_deref(), Some("candidate"));

    state = aggregate_of(operation(&workflow, state, "completed", release_manager));
    state = aggregate_of(operation(&workflow, state, "verified", verifier));
    state = aggregate_of(operation(&workflow, state, "approved", approver));
    state = aggregate_of(operation(&workflow, state, "deploy_failed", deployer));
    assert_eq!(state.task.stage.as_deref(), Some("candidate"));

    state = aggregate_of(operation(&workflow, state, "completed", release_manager));
    state = aggregate_of(operation(&workflow, state, "verified", verifier));
    state = aggregate_of(operation(&workflow, state, "approved", approver));
    state = aggregate_of(operation(&workflow, state, "deployed", deployer));
    state = aggregate_of(operation(&workflow, state, "unhealthy", observer));
    assert_eq!(state.task.stage.as_deref(), Some("candidate"));

    state = aggregate_of(operation(&workflow, state, "completed", release_manager));
    state = aggregate_of(operation(&workflow, state, "verified", verifier));
    state = aggregate_of(operation(&workflow, state, "approved", approver));
    state = aggregate_of(operation(&workflow, state, "deployed", deployer));
    let completed = operation(&workflow, state, "healthy", observer);
    assert_eq!(completed.task.state, TaskState::Completed);
    assert_eq!(completed.task.stage.as_deref(), Some("observation"));
    assert_eq!(completed.task.result.as_deref(), Some("released"));
}

#[test]
fn project_walks_lifecycle_numbers_iterations_and_completes() {
    let workflow = builtin("project");
    let root = task("project", TaskState::Pending, None);
    let mut state = aggregate(root, vec![], vec![], vec![]);

    let mutation = apply(&workflow, state.clone(), TaskEvent::Created, now()).unwrap();
    assert_eq!(mutation.task.stage.as_deref(), Some("goal"));
    assert_eq!(mutation.children[0].task.title, "撰写项目简报");
    state = absorb(state, mutation);

    let steps = [
        ("project_brief_writing", "requirements_analysis"),
        ("requirements_analysis", "architecture"),
        ("architecture_design", "roadmap"),
        ("roadmap", "iteration"),
    ];
    for (child_workflow, expected_stage) in steps {
        let child_id = state.children.last().unwrap().task_id;
        let (updated, mutation) =
            child_changed(&workflow, state, child_id, child_workflow, "approved");
        assert_eq!(mutation.task.stage.as_deref(), Some(expected_stage));
        state = absorb(updated, mutation);
    }
    assert_eq!(
        state.children.last().map(|child| child.workflow.as_str()),
        Some("iteration")
    );
    assert_eq!(state.children.last().unwrap().state, TaskState::Pending);
    let first_iteration = state.children.last().unwrap().task_id;
    assert_eq!(state.children.last().unwrap().task_id, first_iteration);

    let (updated, mutation) =
        child_changed(&workflow, state, first_iteration, "iteration", "needs_more");
    assert_eq!(mutation.task.stage.as_deref(), Some("iteration"));
    assert_eq!(mutation.children.len(), 1);
    assert_eq!(mutation.children[0].task.title, "Iteration 2");
    state = absorb(updated, mutation);

    let last_iteration = state.children.last().unwrap().task_id;
    let (_, mutation) = child_changed(&workflow, state, last_iteration, "iteration", "approved");
    assert_eq!(mutation.task.state, TaskState::Completed);
    assert_eq!(mutation.task.result.as_deref(), Some("approved"));
}

#[test]
fn project_failure_branch_completes_with_failed_result() {
    let workflow = builtin("project");
    let root = task("project", TaskState::InProgress, Some("goal"));
    let child_id = Uuid::now_v7();
    let (_, mutation) = child_changed(
        &workflow,
        aggregate(root, vec![], vec![], vec![]),
        child_id,
        "project_brief_writing",
        "failed",
    );
    assert_eq!(mutation.task.stage.as_deref(), Some("goal"));
    assert_eq!(mutation.task.state, TaskState::Completed);
    assert_eq!(mutation.task.result.as_deref(), Some("failed"));
    assert!(mutation.children.is_empty());
}

#[test]
fn operation_on_a_pending_task_starts_it() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [{"name": "worker", "kind": "any", "min_actors": 1, "max_actors": null}],
            "initial_stage": "work",
            "stages": [{
                "name": "work",
                "active_roles": ["worker"],
                "operations": [{
                    "name": "go",
                    "display_name": "Go",
                    "description": "Start working.",
                    "roles": ["worker"],
                    "action": ""
                }],
                "transitions": []
            }]
        }"#,
    )
    .unwrap();
    let workflow = DefinitionWorkflow::new("custom", body);
    let leaf = task("custom", TaskState::Pending, Some("work"));
    let task_id = leaf.id;
    let worker = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![actor(task_id, "worker", worker, true)],
        vec![],
        vec![],
    );

    let mutation = operation(&workflow, state, "go", worker);
    assert_eq!(mutation.task.state, TaskState::InProgress);
    assert_eq!(mutation.task.stage.as_deref(), Some("work"));
}

#[test]
fn platform_start_requires_bound_agent_roles() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [{"name": "worker", "kind": "agent", "min_actors": 1}],
            "initial_stage": "work",
            "stages": [{
                "name": "work",
                "active_roles": ["worker"],
                "operations": [],
                "transitions": []
            }]
        }"#,
    )
    .unwrap();
    let workflow = DefinitionWorkflow::new("custom", body);
    let leaf = task("custom", TaskState::Pending, Some("work"));
    let task_id = leaf.id;
    let worker = Uuid::now_v7();
    let state = aggregate(leaf, vec![], vec![], vec![]);

    let error = apply(
        &workflow,
        state.clone(),
        TaskEvent::Operation {
            operation: OperationName::new(START_OPERATION),
            actor_id: worker,
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::UnboundRoles(_)));

    let mutation = operation(
        &workflow,
        aggregate(
            state.task,
            vec![actor(task_id, "worker", worker, true)],
            vec![],
            vec![],
        ),
        START_OPERATION,
        worker,
    );
    assert_eq!(mutation.task.state, TaskState::InProgress);
}

#[test]
fn platform_accept_completes_with_approved_result() {
    let workflow = builtin("project_brief_writing");
    let leaf = task(
        "project_brief_writing",
        TaskState::InProgress,
        Some("acceptance"),
    );
    let state = aggregate(leaf, vec![], vec![], vec![]);

    let mutation = operation(&workflow, state, ACCEPT_OPERATION, Uuid::now_v7());

    assert_eq!(mutation.task.state, TaskState::Completed);
    assert_eq!(mutation.task.result, Some(TaskResult::new("approved")));
    assert!(
        mutation
            .events
            .iter()
            .any(|event| event.kind == DomainEventKind::TaskCompleted)
    );
}

#[test]
fn stage_entry_resets_variables() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [
                {"name": "worker", "kind": "any", "min_actors": 1},
                {"name": "reviewer", "kind": "any", "min_actors": 1}
            ],
            "initial_stage": "work",
            "stages": [
                {
                    "name": "work",
                    "active_roles": ["worker"],
                    "vars": {"completed": {"type": "bool", "display_name": "Completed"}},
                    "operations": [{
                        "name": "finish",
                        "display_name": "Finish",
                        "description": "Finish the work.",
                        "roles": ["worker"],
                        "action": "completed = true"
                    }],
                    "transitions": [{"on": "completed", "to": "review"}]
                },
                {
                    "name": "review",
                    "active_roles": ["reviewer"],
                    "vars": {"approved": {"type": "bool", "display_name": "Approved"}},
                    "operations": [{
                        "name": "approve",
                        "display_name": "Approve",
                        "description": "Approve the work.",
                        "roles": ["reviewer"],
                        "action": "approved = true"
                    }],
                    "transitions": [
                        {"on": "approved", "to": "review"},
                        {"on": "not approved", "to": "work"}
                    ]
                }
            ]
        }"#,
    )
    .unwrap();
    let workflow = DefinitionWorkflow::new("reset", body);
    let mut leaf = task("reset", TaskState::InProgress, Some("work"));
    leaf.stage_variables.insert(
        "leftover".to_string(),
        VariableValue::String("stale".to_string()),
    );
    let task_id = leaf.id;
    let worker = Uuid::now_v7();
    let reviewer = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![
            actor(task_id, "worker", worker, true),
            actor(task_id, "reviewer", reviewer, true),
        ],
        vec![],
        vec![],
    );

    let mutation = operation(&workflow, state, "finish", worker);
    assert_eq!(mutation.task.stage.as_deref(), Some("review"));
    assert_eq!(mutation.task.stage_variables.len(), 1);
    assert_eq!(
        mutation.task.stage_variables.get("approved"),
        Some(&VariableValue::Bool(false))
    );

    let mutation = apply(
        &workflow,
        aggregate_of(mutation),
        TaskEvent::Operation {
            operation: OperationName::new("approve"),
            actor_id: reviewer,
        },
        now(),
    )
    .unwrap();
    assert_eq!(mutation.task.stage.as_deref(), Some("review"));
    assert_eq!(
        mutation.task.stage_variables.get("approved"),
        Some(&VariableValue::Bool(false))
    );
}

#[test]
fn actor_bound_only_reactivates_roles_of_the_current_stage() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::InProgress, Some("clarify"));
    let task_id = leaf.id;
    let clarifier = Uuid::now_v7();
    let acceptor = Uuid::now_v7();
    let state = aggregate(
        leaf,
        vec![
            actor(task_id, "clarifier", clarifier, true),
            actor(task_id, "acceptor", acceptor, false),
        ],
        vec![],
        vec![],
    );

    let mutation = apply(
        &workflow,
        state,
        TaskEvent::ActorBound {
            role: RoleKey::new("acceptor"),
            account_id: acceptor,
        },
        now(),
    )
    .unwrap();
    assert!(
        !mutation
            .actors
            .iter()
            .any(|actor| actor.account_id == acceptor && actor.active)
    );
}

#[test]
fn title_template_renders_fields_and_iteration_numbers() {
    let template =
        crate::TitleTemplate::parse("Iteration {count_children('iteration') + 1}").unwrap();
    let children = vec![
        ChildState {
            task_id: Uuid::now_v7(),
            workflow: WorkflowId::new("iteration"),
            state: TaskState::Completed,
            result: None,
        },
        ChildState {
            task_id: Uuid::now_v7(),
            workflow: WorkflowId::new("iteration"),
            state: TaskState::Completed,
            result: None,
        },
    ];
    let title = template
        .render(&crate::TitleContext {
            task_title: "Root",
            children: &children,
        })
        .unwrap();
    assert_eq!(title, "Iteration 3");

    let template = crate::TitleTemplate::parse("{{literal}} {task.title}").unwrap();
    let title = template
        .render(&crate::TitleContext {
            task_title: "Root",
            children: &[],
        })
        .unwrap();
    assert_eq!(title, "{literal} Root");
}

#[test]
fn validator_rejects_unknown_references() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [],
            "initial_stage": "work",
            "stages": [{
                "name": "work",
                "created": [{"create_child": {"workflow": "missing", "title": "x"}}],
                "transitions": [{"on": "true", "to": "nowhere"}]
            }]
        }"#,
    )
    .unwrap();
    let validation = validate(&body, &|_| false);
    let codes: Vec<&str> = validation
        .errors
        .iter()
        .map(|issue| issue.code.as_str())
        .collect();
    assert!(codes.contains(&"unknown_child_workflow"));
    assert!(codes.contains(&"unknown_stage"));
}

#[test]
fn validator_rejects_operations_with_inactive_roles() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [{"name": "worker", "kind": "any", "min_actors": 1}],
            "initial_stage": "work",
            "stages": [{
                "name": "work",
                "active_roles": [],
                "operations": [{
                    "name": "go",
                    "display_name": "Go",
                    "description": "Start working.",
                    "roles": ["worker"],
                    "action": ""
                }]
            }]
        }"#,
    )
    .unwrap();
    let validation = validate(&body, &|_| true);
    let codes: Vec<&str> = validation
        .errors
        .iter()
        .map(|issue| issue.code.as_str())
        .collect();
    assert!(codes.contains(&"operation_role_not_active"));
    assert!(
        validation
            .warnings
            .iter()
            .any(|issue| issue.code == "operations_without_actors")
    );
}

#[test]
fn validator_reports_script_errors() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [],
            "initial_stage": "work",
            "stages": [{
                "name": "work",
                "operations": [{
                    "name": "go",
                    "display_name": "Go",
                    "description": "Start working.",
                    "roles": [],
                    "action": "missing = true"
                }]
            }]
        }"#,
    )
    .unwrap();
    let validation = validate(&body, &|_| true);
    assert!(
        validation
            .errors
            .iter()
            .any(|issue| issue.code == "script_unknown_variable")
    );
}

#[test]
fn validator_reports_never_triggered_transitions() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [],
            "initial_stage": "work",
            "stages": [
                {
                    "name": "work",
                    "vars": {"done": {"type": "bool", "display_name": "Done"}},
                    "transitions": [{"on": "done", "to": "ended"}]
                }
            ]
        }"#,
    )
    .unwrap();
    let validation = validate(&body, &|_| true);
    assert!(
        validation
            .warnings
            .iter()
            .any(|issue| issue.code == "transition_never_triggered")
    );
}

#[test]
fn pending_task_without_dependencies_can_complete_directly() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "initial_stage": "only",
            "stages": [{
                "name": "only",
                "transitions": [
                    {"on": "true", "action": "result = 'completed'", "to": "ended"}
                ]
            }]
        }"#,
    )
    .unwrap();
    let workflow = DefinitionWorkflow::new("immediate", body);
    let leaf = task("immediate", TaskState::Pending, None);
    let mutation = apply(
        &workflow,
        aggregate(leaf, vec![], vec![], vec![]),
        TaskEvent::Created,
        now(),
    )
    .unwrap();
    assert_eq!(mutation.task.state, TaskState::Completed);
    assert_eq!(mutation.task.result.as_deref(), Some("completed"));
    assert_eq!(mutation.task.stage.as_deref(), Some("only"));
}

struct BadOrder;

impl TaskWorkflow for BadOrder {
    fn id(&self) -> &str {
        "bad-order"
    }

    fn on_event(&self, ctx: &mut WorkflowContext<'_>, _event: TaskEvent) -> Result<(), TaskError> {
        ctx.complete_task(None)?;
        ctx.set_state(TaskState::InProgress)
    }
}

#[test]
fn effects_after_complete_are_rejected() {
    let leaf = task("bad-order", TaskState::Pending, None);
    let error = apply(
        &BadOrder,
        aggregate(leaf, vec![], vec![], vec![]),
        TaskEvent::Created,
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::InvalidEffect(_)));
}

#[test]
fn unknown_role_binding_is_rejected() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::Pending, Some("clarify"));
    let state = aggregate(leaf, vec![], vec![], vec![]);
    let error = apply(
        &workflow,
        state,
        TaskEvent::ActorBound {
            role: RoleKey::new("nonexistent"),
            account_id: Uuid::now_v7(),
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::UnknownRole(_)));
}

#[test]
fn activation_requires_a_bound_account() {
    let workflow = builtin("project_brief_writing");
    let leaf = task("project_brief_writing", TaskState::Pending, Some("clarify"));
    let state = aggregate(leaf, vec![], vec![], vec![]);
    let error = apply(
        &workflow,
        state,
        TaskEvent::Operation {
            operation: OperationName::new("started"),
            actor_id: Uuid::now_v7(),
        },
        now(),
    )
    .unwrap_err();
    assert!(matches!(error, TaskError::Unauthorized(_)));
}

#[test]
fn create_issue_action_emits_issue_create() {
    let body: WorkflowBody = serde_json::from_str(
        r#"{
            "schema_version": 1,
            "roles": [],
            "initial_stage": "clarify",
            "stages": [{
                "name": "clarify",
                "events": [{
                    "event": "task.created",
                    "action": "create_issue(kind = 'question', title = 'Project clarification: {task.title}', description = 'Discuss the project.', relation = 'clarifies', task_workflow = 'project_brief_writing', task_title = 'Clarify project goals')"
                }]
            }]
        }"#,
    )
    .unwrap();
    let workflow = DefinitionWorkflow::new("clarification", body);
    let mutation = apply(
        &workflow,
        aggregate(
            task("clarification", TaskState::Pending, None),
            vec![],
            vec![],
            vec![],
        ),
        TaskEvent::Created,
        now(),
    )
    .unwrap();
    assert_eq!(mutation.issues.len(), 1);
    let issue = &mutation.issues[0];
    assert_eq!(issue.kind, "question");
    assert_eq!(issue.title, "Project clarification: Task");
    assert_eq!(issue.description, "Discuss the project.");
    assert_eq!(issue.relation, "clarifies");
    assert_eq!(mutation.children.len(), 1);
    let child = &mutation.children[0].task;
    assert_eq!(child.workflow.as_str(), "project_brief_writing");
    assert_eq!(child.title, "Clarify project goals");
    assert_eq!(issue.link_task_id, Some(child.id));
}
