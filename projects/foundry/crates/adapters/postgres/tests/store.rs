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
    TaskDispatcher, TaskFilter, TaskLinkRepository, TaskRepository, TaskService, TaskViewer,
    WorkflowCatalog,
};
use foundry_domain::{
    Account, AccountKind, AccountRole, Actor, CreateAgentDefinition, DomainEvent, DomainEventKind,
    Issue, IssueComment, IssueKind, IssueSource, IssueSourceKind, OperationName, Project,
    ProjectRole, RoleKey, StageKey, TaskDAO, TaskLink, TaskLinkKind, TaskLinkRelation, TaskState,
    WorkflowId,
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
        "git@example.com:test/repo.git",
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
    let mut project = Project::new(
        format!("Tasks {}", suffix()),
        None,
        "git@example.com:test/repo.git",
        owner.id,
    )
    .unwrap();
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
    // assignment. The Task Dispatcher sees the Ready task itself and decides.
    let ready = store.list_ready_tasks(Some(project.id)).await.unwrap();
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
                project_id: project.id,
                model_base_url: "http://127.0.0.1:11434/v1".to_string(),
                model_name: "test-model".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                system_prompt: "Clarify the project.".to_string(),
                tools: Vec::new(),
                shell: false,
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
    let ready = store.list_ready_tasks(Some(project.id)).await.unwrap();
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
    let mut project = Project::new(
        format!("Delete project {}", suffix()),
        None,
        "git@example.com:test/repo.git",
        owner.id,
    )
    .unwrap();
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
            active: false,
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
async fn ready_tasks_include_pending_role_less_workflows() {
    let Some(store) = test_store().await else {
        eprintln!("TEST_DATABASE_URL is unset; skipping Postgres integration test");
        return;
    };
    let store = Arc::new(store);
    let owner = account("owner");
    AccountRepository::insert(&*store, &owner).await.unwrap();
    let mut project = Project::new(
        format!("Roleless {}", suffix()),
        None,
        "git@example.com:test/repo.git",
        owner.id,
    )
    .unwrap();
    let mut root = TaskDAO::new(
        project.id,
        None,
        WorkflowId::new("software_development"),
        1,
        "Software development",
        OffsetDateTime::now_utc(),
    );
    root.state = TaskState::Pending;
    root.stage = Some(StageKey::new("running"));
    project.root_task_id = Some(root.id);
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

    let ready = store.list_ready_tasks(Some(project.id)).await.unwrap();
    let view = ready
        .iter()
        .find(|view| view.task_id == root.id)
        .expect("role-less task is ready");
    assert!(view.roles.is_empty());
    assert_eq!(view.workflow, "software_development");
}
