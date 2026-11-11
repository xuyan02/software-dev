//! Integration tests against a real Postgres database.
//!
//! Set `TEST_DATABASE_URL` to run them:
//!
//! ```bash
//! TEST_DATABASE_URL=postgres://foundry:foundry@localhost:5432/foundry_test \
//!   cargo test -p foundry-adapter-postgres
//! ```
//!
//! Without the variable the tests report that they were skipped and pass, so
//! `cargo test --workspace` still works on a machine without Postgres. Every
//! test uses unique handles and names, so tests can run in parallel without
//! truncating shared tables.

use foundry_adapter_postgres::{
    PostgresEventSink, PostgresStore, PostgresTaskDispatcher, PostgresWorkflowCatalog,
};
use foundry_app::{
    AccountRepository, EventSink, IssueFilter, IssueRepository, ProjectRepository, RepoError,
    RequirementsApplyOutcome, RequirementsApprovalOutcome, RequirementsDocumentRepository,
    TaskDispatcher, TaskFilter, TaskLinkRepository, TaskRepository, TaskService, TaskViewer,
    WorkflowCatalog,
};
use foundry_domain::{
    Account, AccountKind, AccountRole, Actor, Confidence, CreateAgentDefinition, DomainEvent,
    DomainEventKind, EntityStatus, Function, Issue, IssueComment, IssueKind, IssueSource,
    IssueSourceKind, Operation, OperationName, Project, ProjectRole, RequirementOp,
    RequirementsOperation, RoleKey, Story, StoryPriority, TaskDAO, TaskLink, TaskLinkKind,
    TaskLinkRelation, TaskState, UserModel, WorkflowId,
};
use serde_json::json;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

async fn test_store() -> Option<PostgresStore> {
    let url = std::env::var("TEST_DATABASE_URL").ok()?;
    let database = url
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .split('?')
        .next()
        .unwrap_or_default();
    assert_ne!(
        database, "foundry",
        "TEST_DATABASE_URL must point at a dedicated test database, not the runtime database"
    );
    let store = PostgresStore::connect(&url)
        .await
        .expect("connect to TEST_DATABASE_URL");
    store.migrate().await.expect("apply migrations");
    store
        .seed_builtin_workflows()
        .await
        .expect("seed builtin workflows");
    Some(store)
}

fn suffix() -> String {
    let id = Uuid::now_v7().simple().to_string();
    id[id.len() - 8..].to_string()
}

fn account(prefix: &str) -> Account {
    Account::new(
        format!("{prefix}{}", suffix()),
        format!("{prefix} test"),
        AccountKind::Human,
        AccountRole::Member,
        None,
        Some("hash".to_string()),
    )
    .expect("valid account")
}

fn issue(project_id: Uuid, title: &str) -> Issue {
    Issue::new(
        project_id,
        title,
        "描述",
        IssueKind::Bug,
        IssueSource {
            kind: IssueSourceKind::Customer,
            reference: None,
        },
        Actor::human("reporter"),
    )
    .expect("valid issue")
}

#[tokio::test]
async fn aggregates_round_trip_with_optimistic_concurrency() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };

    let alice = account("alice");
    AccountRepository::insert(&store, &alice).await.unwrap();
    let loaded = AccountRepository::get(&store, alice.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.handle, alice.handle);
    assert_eq!(
        AccountRepository::get_by_handle(&store, &alice.handle.to_uppercase())
            .await
            .unwrap()
            .unwrap()
            .id,
        alice.id
    );
    assert!(matches!(
        AccountRepository::insert(&store, &alice).await,
        Err(RepoError::Duplicate(_))
    ));

    let project = Project::new(
        format!("Database project {}", suffix()),
        Some("persisted".to_string()),
        alice.id,
    )
    .unwrap();
    ProjectRepository::insert(&store, &project).await.unwrap();
    let loaded = ProjectRepository::get(&store, project.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.role_of(alice.id), Some(ProjectRole::Owner));
    assert_eq!(loaded.description.as_deref(), Some("persisted"));

    let mut issue = issue(project.id, "退款太慢");
    issue.comments.push(IssueComment {
        id: Uuid::now_v7(),
        author: Actor::human("reviewer"),
        body: "已复现".to_string(),
        internal: false,
        created_at: OffsetDateTime::now_utc(),
    });
    IssueRepository::insert(&store, &issue).await.unwrap();
    issue.title = "退款很慢".to_string();
    issue.version = 2;
    IssueRepository::save(&store, &issue, 1).await.unwrap();
    let stale = IssueRepository::save(&store, &issue, 1).await;
    assert!(matches!(stale, Err(RepoError::Conflict { .. })));

    let loaded = IssueRepository::get(&store, issue.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.title, "退款很慢");
    assert_eq!(loaded.comments.len(), 1);
    assert_eq!(loaded.comments[0].body, "已复现");

    let projected_comments: i64 =
        sqlx::query_scalar("select count(*) from issue_comments where issue_id = $1")
            .bind(issue.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert_eq!(projected_comments, 1);

    let bugs = IssueRepository::list(
        &store,
        project.id,
        IssueFilter {
            kind: Some(IssueKind::Bug),
            text: Some("很慢".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(bugs.len(), 1);
    assert_eq!(bugs[0].id, issue.id);
}

#[tokio::test]
async fn events_are_written() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };
    let sink = PostgresEventSink::new(store.pool().clone());
    let event = DomainEvent::new(
        DomainEventKind::IssueCreated,
        Uuid::now_v7(),
        Actor::human("reporter"),
        json!({ "title": "persisted" }),
    );
    sink.publish(std::slice::from_ref(&event)).await.unwrap();

    let events: i64 = sqlx::query_scalar("select count(*) from events where id = $1")
        .bind(event.id)
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(events, 1);
}

#[tokio::test]
async fn task_workflow_persists_effects_events_and_input_idempotency() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };
    let store = Arc::new(store);
    let workflows: Arc<dyn WorkflowCatalog> =
        Arc::new(PostgresWorkflowCatalog::new(store.pool().clone()));
    let dispatcher = Arc::new(PostgresTaskDispatcher::new(
        store.pool().clone(),
        workflows.clone(),
    ));
    let repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = TaskService::new(repository, dispatcher.clone(), workflows);

    let owner = account("owner");
    AccountRepository::insert(&*store, &owner).await.unwrap();
    let mut project = Project::new(format!("Tasks {}", suffix()), None, owner.id).unwrap();
    let mut root = TaskDAO::new(
        project.id,
        None,
        WorkflowId::new("project"),
        1,
        "Project goal",
        OffsetDateTime::now_utc(),
    );
    project.root_task_id = Some(root.id);
    root.state = TaskState::Pending;
    let event = DomainEvent::new(
        DomainEventKind::TaskCreated,
        root.id,
        Actor::System,
        json!({
            "workflow": root.workflow,
            "workflow_version": root.workflow_version,
            "title": root.title,
            "parent_id": null,
            "dependencies": [],
        }),
    );
    ProjectRepository::insert_with_root(&*store, &project, &root, &[event])
        .await
        .unwrap();
    dispatcher.process_pending(100).await.unwrap();

    let children = TaskRepository::list(
        &*store,
        project.id,
        TaskFilter {
            parent_id: Some(root.id),
            ..TaskFilter::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(children.len(), 1);
    let child = children[0].task.clone();
    assert_eq!(child.workflow.as_str(), "goal_clarification");

    // The child has no dependencies, so the scheduler wakes it once.
    dispatcher.dispatch_ready().await.unwrap();
    dispatcher.process_pending(100).await.unwrap();
    let child_after = TaskRepository::get(&*store, child.id)
        .await
        .unwrap()
        .expect("child task");
    assert_eq!(child_after.stage.as_deref(), Some("clarification"));
    let child_view = tasks
        .get(project.id, child.id, TaskViewer::system())
        .await
        .unwrap();
    assert!(
        child_view
            .operations
            .iter()
            .any(|operation| operation.kind == DomainEventKind::DispatchRequested)
    );

    // The child's `task.created` action creates the clarification Issue and
    // links it to the Task in the same transaction.
    let issues = IssueRepository::list(&*store, project.id, IssueFilter::default())
        .await
        .unwrap();
    let issue = issues
        .iter()
        .find(|issue| issue.title == "Project clarification: Project goal")
        .expect("clarification issue");
    assert_eq!(issue.kind, IssueKind::Question);
    let links = TaskLinkRepository::list(&*store, child.id).await.unwrap();
    assert!(links.iter().any(|link| {
        link.kind == TaskLinkKind::Issue
            && link.relation == TaskLinkRelation::Clarifies
            && link.target_id == Some(issue.id)
    }));

    // The clarifier role is an agent role with no actor bound and no
    // assignment. The PM sees the Ready task itself and decides.
    let ready = store.list_ready_tasks().await.unwrap();
    let view = ready
        .iter()
        .find(|view| view.task_id == child.id)
        .expect("ready task");
    assert!(view.roles.iter().any(|role| {
        role.name == "clarifier"
            && role.kind == foundry_domain::RoleKind::Agent
            && role.agent.is_none()
    }));
    assert!(view.roles.iter().any(|role| {
        role.name == "writer"
            && role.kind == foundry_domain::RoleKind::Agent
            && role.agent.is_none()
    }));
    assert!(view.roles.iter().any(|role| {
        role.name == "style_reviewer"
            && role.kind == foundry_domain::RoleKind::Agent
            && role.agent.is_none()
    }));
    assert!(view.roles.iter().any(|role| {
        role.name == "content_reviewer"
            && role.kind == foundry_domain::RoleKind::Agent
            && role.agent.is_none()
    }));

    // Assigning a clarifier and starting the task activates the worker run.
    let clarifier_account = Account::new(
        format!("clarifier{}", suffix()),
        "clarifier test",
        AccountKind::Agent,
        AccountRole::Member,
        None,
        Some("hash".to_string()),
    )
    .unwrap();
    AccountRepository::insert(&*store, &clarifier_account)
        .await
        .unwrap();
    let clarifier = store
        .insert_agent_definition(
            &CreateAgentDefinition {
                name: format!("Clarifier {}", suffix()),
                account_handle: clarifier_account.handle.clone(),
                role: "goal_clarification:clarifier".to_string(),
                model_base_url: "http://127.0.0.1:11434/v1".to_string(),
                model_name: "test-model".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                system_prompt: "Clarify the project.".to_string(),
                tools: Vec::new(),
                skills: Vec::new(),
                enabled: true,
            },
            clarifier_account.id,
            &format!("Clarifier {}", suffix()),
        )
        .await
        .unwrap();
    store
        .assign_task(child.id, "goal_clarification:clarifier", clarifier.id)
        .await
        .unwrap();
    let ready = store.list_ready_tasks().await.unwrap();
    let view = ready
        .iter()
        .find(|view| view.task_id == child.id)
        .expect("task stays Ready until started");
    let role = view
        .roles
        .iter()
        .find(|role| role.name == "clarifier")
        .expect("clarifier role");
    assert!(
        role.agent
            .as_ref()
            .is_some_and(|agent| agent.agent_definition_id == clarifier.id && agent.enabled)
    );
    sqlx::query("update tasks set state = 'in_progress', updated_at = now() where id = $1")
        .bind(child.id)
        .execute(store.pool())
        .await
        .unwrap();
    let assigned = store.assigned_active_tasks(clarifier.id).await.unwrap();
    assert!(assigned.iter().any(|task| task.task_id == child.id));
    let clarifier_member: Option<String> = sqlx::query_scalar(
        "select role from project_members where project_id = $1 and account_id = $2",
    )
    .bind(project.id)
    .bind(clarifier_account.id)
    .fetch_optional(store.pool())
    .await
    .unwrap();
    assert_eq!(clarifier_member.as_deref(), Some("member"));

    tasks
        .bind_actor(
            project.id,
            child.id,
            RoleKey::new("clarifier"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();
    tasks
        .bind_actor(
            project.id,
            child.id,
            RoleKey::new("writer"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();
    tasks
        .bind_actor(
            project.id,
            child.id,
            RoleKey::new("style_reviewer"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();
    tasks
        .bind_actor(
            project.id,
            child.id,
            RoleKey::new("content_reviewer"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();
    tasks
        .bind_actor(
            project.id,
            child.id,
            RoleKey::new("acceptor"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();

    for (index, operation) in [
        "started",
        "completed",
        "started",
        "completed",
        "approved",
        "approved",
        "approved",
    ]
    .iter()
    .enumerate()
    {
        tasks
            .send_operation(
                project.id,
                child.id,
                owner.id,
                OperationName::new(*operation),
                format!("{}:{index}:{operation}", child.id),
            )
            .await
            .unwrap();
    }
    // Replaying the accepted idempotency key is a successful no-op.
    tasks
        .send_operation(
            project.id,
            child.id,
            owner.id,
            OperationName::new("approved"),
            format!("{}:6:approved", child.id),
        )
        .await
        .unwrap();

    let done = tasks
        .get(project.id, child.id, TaskViewer::system())
        .await
        .unwrap();
    assert_eq!(done.state, TaskState::Completed);
    assert_eq!(done.result.as_deref(), Some("approved"));
    assert!(done.actors.iter().all(|actor| !actor.active));
    assert_eq!(done.operations[0].kind, DomainEventKind::TaskCompleted);

    dispatcher.process_pending(100).await.unwrap();

    let root = tasks
        .get(project.id, root.id, TaskViewer::system())
        .await
        .unwrap();
    assert_eq!(root.stage.as_deref(), Some("requirements_analysis"));
    assert_eq!(root.state, TaskState::InProgress);
}

fn reqdoc_user(id: &str) -> UserModel {
    UserModel {
        id: id.to_string(),
        name: "Analyst".to_string(),
        age: Some(30),
        gender: None,
        occupation: None,
        habits: vec![],
        goals: vec!["understand the requirement".to_string()],
        pain_points: vec!["the current process is manual".to_string()],
        permissions: vec![],
        scenarios: vec![],
        devices: vec![],
        accessibility: vec![],
        evidence: vec!["interview".to_string()],
        confidence: Confidence::Medium,
        status: EntityStatus::Draft,
    }
}

fn reqdoc_function(id: &str) -> Function {
    Function {
        id: id.to_string(),
        name: "Shopping".to_string(),
        description: None,
        owner: None,
        status: EntityStatus::Draft,
        operations: vec![],
        stories: vec![],
        interactions: vec![],
        interaction_flows: vec![],
        screens: vec![],
    }
}

fn reqdoc_operation(id: &str, user_id: &str) -> Operation {
    Operation {
        id: id.to_string(),
        name: "Search".to_string(),
        description: None,
        actor_user_ids: vec![user_id.to_string()],
        trigger: Some("user searches".to_string()),
        preconditions: vec![],
        inputs: vec!["keyword".to_string()],
        outputs: vec!["results".to_string()],
        states: vec!["idle".to_string()],
        exceptions: vec![],
        permissions: vec![],
        side_effects: vec![],
        audit: vec![],
        interaction_id: None,
        status: EntityStatus::Draft,
    }
}

fn reqdoc_story(id: &str, user_id: &str, operation_id: &str) -> Story {
    Story {
        id: id.to_string(),
        title: "Search for a product".to_string(),
        user_ids: vec![user_id.to_string()],
        scenario: "A customer wants a product".to_string(),
        goal: "Find the product".to_string(),
        preconditions: vec![],
        postconditions: vec![],
        operation_ids: vec![operation_id.to_string()],
        interaction_flow_id: None,
        screen_ids: vec![],
        priority: StoryPriority::Must,
        source_issue_ids: vec![],
        acceptance_notes: vec![],
        status: EntityStatus::Draft,
    }
}

#[tokio::test]
async fn requirements_document_round_trips_idempotently_and_conflicts() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };

    let alice = account("reqdoc");
    AccountRepository::insert(&store, &alice).await.unwrap();
    let project =
        Project::new(format!("Requirements project {}", suffix()), None, alice.id).unwrap();
    ProjectRepository::insert(&store, &project).await.unwrap();

    let now = OffsetDateTime::now_utc();
    let (_document, draft) = store
        .ensure_document(project.id, alice.id, now)
        .await
        .unwrap();
    assert_eq!(draft.revision, 1);

    let user_id = format!("user-{}", suffix());
    let function_id = format!("func-{}", suffix());
    let operation_id = format!("op-{}", suffix());
    let story_id = format!("story-{}", suffix());
    let operations = vec![
        RequirementsOperation {
            op_id: Uuid::now_v7(),
            op: RequirementOp::UserAdd {
                user: reqdoc_user(&user_id),
            },
        },
        RequirementsOperation {
            op_id: Uuid::now_v7(),
            op: RequirementOp::FunctionAdd {
                function: reqdoc_function(&function_id),
            },
        },
        RequirementsOperation {
            op_id: Uuid::now_v7(),
            op: RequirementOp::OperationAdd {
                function_id: function_id.clone(),
                operation: reqdoc_operation(&operation_id, &user_id),
            },
        },
        RequirementsOperation {
            op_id: Uuid::now_v7(),
            op: RequirementOp::StoryAdd {
                function_id: function_id.clone(),
                story: reqdoc_story(&story_id, &user_id, &operation_id),
            },
        },
    ];

    match store
        .apply_operations(project.id, 1, &operations, alice.id, now)
        .await
        .unwrap()
    {
        RequirementsApplyOutcome::Applied { revision, .. } => assert_eq!(revision, 2),
        other => panic!("unexpected apply outcome: {other:?}"),
    }

    // Replaying the same op ids with a stale revision is idempotent.
    match store
        .apply_operations(project.id, 1, &operations, alice.id, now)
        .await
        .unwrap()
    {
        RequirementsApplyOutcome::Applied { revision, .. } => assert_eq!(revision, 2),
        other => panic!("unexpected replay outcome: {other:?}"),
    }

    match store
        .approve(project.id, 2, alice.id, Some("looks good"), now)
        .await
        .unwrap()
    {
        RequirementsApprovalOutcome::Approved {
            version, revision, ..
        } => {
            assert_eq!(version, 1);
            assert_eq!(revision, 2);
        }
        other => panic!("unexpected approve outcome: {other:?}"),
    }

    let version = RequirementsDocumentRepository::version(&store, project.id, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(version.body.users.len(), 1);
    assert_eq!(version.body.functions.len(), 1);
    assert_eq!(version.body.functions[0].stories.len(), 1);

    let second = RequirementsOperation {
        op_id: Uuid::now_v7(),
        op: RequirementOp::UserAdd {
            user: reqdoc_user(&format!("user-{}", suffix())),
        },
    };
    match store
        .apply_operations(project.id, 1, &[second], alice.id, now)
        .await
        .unwrap()
    {
        RequirementsApplyOutcome::Conflict {
            current_revision, ..
        } => assert_eq!(current_revision, 2),
        other => panic!("unexpected conflict outcome: {other:?}"),
    }
}

#[tokio::test]
async fn goal_clarification_is_valid() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };

    let body: serde_json::Value = sqlx::query_scalar(
        "select body from workflow_versions \
         where workflow_id = 'goal_clarification' and version = 1",
    )
    .fetch_one(store.pool())
    .await
    .expect("goal_clarification v1");
    let body: foundry_domain::WorkflowBody =
        serde_json::from_value(body).expect("v1 body is a workflow AST");
    let validation = foundry_domain::validate(&body, &|_| true);
    assert!(validation.is_valid(), "{:?}", validation.errors);
    assert_eq!(body.roles[0].name, "clarifier");
    assert_eq!(body.roles[0].kind, foundry_domain::RoleKind::Agent);

    let project: serde_json::Value = sqlx::query_scalar(
        "select body from workflow_versions where workflow_id = 'project' and version = 1",
    )
    .fetch_one(store.pool())
    .await
    .expect("project v1");
    let project: foundry_domain::WorkflowBody =
        serde_json::from_value(project).expect("project body is a workflow AST");
    let validation = foundry_domain::validate(&project, &|_| true);
    assert!(validation.is_valid(), "{:?}", validation.errors);
    let action = project.stages[0]
        .events
        .iter()
        .find(|event| event.event == foundry_domain::EventKind::TaskCreated)
        .map(|event| event.action.as_str())
        .expect("project task.created action");
    assert!(action.contains("create_issue("));
    assert!(action.contains("task_workflow = 'goal_clarification'"));
}

#[tokio::test]
async fn deleting_a_project_removes_scoped_rows_and_events() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };

    let owner = account("delete-owner");
    AccountRepository::insert(&store, &owner).await.unwrap();
    let mut project = Project::new(format!("Delete project {}", suffix()), None, owner.id).unwrap();
    let mut root = TaskDAO::new(
        project.id,
        None,
        WorkflowId::new("project"),
        1,
        "Project goal",
        OffsetDateTime::now_utc(),
    );
    project.root_task_id = Some(root.id);
    root.state = TaskState::Pending;
    let root_event = DomainEvent::new(
        DomainEventKind::TaskCreated,
        root.id,
        Actor::System,
        json!({
            "workflow": root.workflow,
            "workflow_version": root.workflow_version,
            "title": root.title,
            "parent_id": null,
            "dependencies": [],
        }),
    );
    ProjectRepository::insert_with_root(&store, &project, &root, &[root_event])
        .await
        .unwrap();

    let mut issue = issue(project.id, "delete with the project");
    issue.comments.push(IssueComment {
        id: Uuid::now_v7(),
        author: Actor::human("reporter"),
        body: "comment".to_string(),
        internal: false,
        created_at: OffsetDateTime::now_utc(),
    });
    IssueRepository::insert(&store, &issue).await.unwrap();

    PostgresEventSink::new(store.pool().clone())
        .publish(&[DomainEvent::new(
            DomainEventKind::IssueCreated,
            issue.id,
            Actor::System,
            json!({}),
        )])
        .await
        .unwrap();
    TaskLinkRepository::insert(
        &store,
        &TaskLink {
            id: Uuid::now_v7(),
            task_id: root.id,
            kind: TaskLinkKind::Url,
            target_id: None,
            url: Some("https://example.com/delete".to_string()),
            title: "Reference".to_string(),
            relation: TaskLinkRelation::References,
            created_by: Actor::System,
            created_at: OffsetDateTime::now_utc(),
            metadata: json!({}),
        },
    )
    .await
    .unwrap();

    let stale = ProjectRepository::delete(&store, project.id, project.version + 1).await;
    assert!(matches!(stale, Err(RepoError::Conflict { .. })));

    ProjectRepository::delete(&store, project.id, project.version)
        .await
        .unwrap();

    assert!(
        ProjectRepository::get(&store, project.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        TaskRepository::get(&store, root.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        IssueRepository::get(&store, issue.id)
            .await
            .unwrap()
            .is_none()
    );

    let remaining: i64 = sqlx::query_scalar(
        "select (select count(*) from project_members where project_id = $1) \
              + (select count(*) from issue_comments where issue_id = $2) \
              + (select count(*) from task_links where task_id = $3) \
              + (select count(*) from events \
                 where project_id = $1 or aggregate_id in ($1, $2, $3))",
    )
    .bind(project.id)
    .bind(issue.id)
    .bind(root.id)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(remaining, 0);
}

#[tokio::test]
async fn anchor_document_round_trips_with_annotations() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };

    let alice = account("anchordoc");
    AccountRepository::insert(&store, &alice).await.unwrap();
    let project = Project::new(format!("Anchor document {}", suffix()), None, alice.id).unwrap();
    ProjectRepository::insert(&store, &project).await.unwrap();

    let schema = foundry_domain::project_document_schema();
    let now = OffsetDateTime::now_utc();
    let (_document, draft) = foundry_app::AnchorDocumentRepository::ensure_document(
        &store, project.id, "project", &schema, alice.id, now,
    )
    .await
    .unwrap();
    assert_eq!(draft.revision, 1);

    let operations = vec![
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::UpdateText {
                anchor: "positioning.what_is".to_string(),
                text: "企业费用报销系统".to_string(),
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::Add {
                parent: "positioning.what_is_not".to_string(),
                occurrence_key: Some("e1".to_string()),
                text: "财务总账系统".to_string(),
                after: None,
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::UpdateText {
                anchor: "background.context".to_string(),
                text: "报销目前由员工填写纸质单。".to_string(),
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::Add {
                parent: "background.problems".to_string(),
                occurrence_key: Some("p1".to_string()),
                text: "员工无法查询报销进度，平均催问两次".to_string(),
                after: None,
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::Add {
                parent: "goals".to_string(),
                occurrence_key: Some("g1".to_string()),
                text: "员工可自助查询报销进度".to_string(),
                after: None,
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::Add {
                parent: "non_goals".to_string(),
                occurrence_key: Some("n1".to_string()),
                text: "不做移动端应用".to_string(),
                after: None,
            },
        },
        foundry_domain::AnchorOperation {
            op_id: Uuid::now_v7(),
            op: foundry_domain::AnchorOp::UpdateText {
                anchor: "non_goals/non_goal/n1.non_goal_reason".to_string(),
                text: "首期只覆盖桌面端".to_string(),
            },
        },
    ];

    match foundry_app::AnchorDocumentRepository::apply_operations(
        &store,
        project.id,
        "project",
        1,
        &operations,
        alice.id,
        now,
        &schema,
    )
    .await
    .unwrap()
    {
        foundry_app::AnchorApplyOutcome::Applied { revision, .. } => assert_eq!(revision, 2),
        other => panic!("unexpected apply outcome: {other:?}"),
    }

    let draft = foundry_app::AnchorDocumentRepository::draft(&store, project.id, "project")
        .await
        .unwrap()
        .unwrap();
    let validation = draft.snapshot.validate(&schema);
    assert!(validation.is_valid(), "{:?}", validation.errors);

    match foundry_app::AnchorDocumentRepository::approve(
        &store,
        project.id,
        "project",
        2,
        alice.id,
        Some("项目基线"),
        now,
        &schema,
    )
    .await
    .unwrap()
    {
        foundry_app::AnchorApprovalOutcome::Approved { version, .. } => assert_eq!(version, 1),
        other => panic!("unexpected approve outcome: {other:?}"),
    }

    let annotation = foundry_app::AnchorDocumentRepository::insert_annotation(
        &store,
        foundry_domain::AnchorAnnotation {
            id: Uuid::now_v7(),
            project_id: project.id,
            kind: "project".to_string(),
            anchor_key: "goals/g1".to_string(),
            anchor_id: draft.snapshot.find_path("goals/g1").unwrap().id.clone(),
            quote: None,
            body: "补充查询范围。".to_string(),
            author_id: alice.id,
            author_handle: alice.handle.clone(),
            author_name: alice.display_name.clone(),
            status: foundry_domain::AnchorAnnotationStatus::Open,
            resolution: None,
            created_at: now,
            resolved_at: None,
            resolved_by: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        annotation.status,
        foundry_domain::AnchorAnnotationStatus::Open
    );

    let annotations =
        foundry_app::AnchorDocumentRepository::annotations(&store, project.id, "project")
            .await
            .unwrap();
    assert_eq!(annotations.len(), 1);

    let resolved = foundry_app::AnchorDocumentRepository::resolve_annotation(
        &store,
        project.id,
        "project",
        annotation.id,
        "已补充。",
        alice.id,
        now,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        resolved.status,
        foundry_domain::AnchorAnnotationStatus::Resolved
    );

    let version = foundry_app::AnchorDocumentRepository::version(&store, project.id, "project", 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        version.snapshot.find_path("goals/g1").unwrap().text,
        "员工可自助查询报销进度"
    );
}
