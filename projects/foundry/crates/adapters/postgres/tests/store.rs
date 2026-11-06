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
    RequirementFilter, RequirementRepository, TaskDispatcher, TaskFilter, TaskRepository,
    TaskService, TaskViewer, WorkflowCatalog,
};
use foundry_domain::{
    AcceptanceScenario, Account, AccountKind, AccountRole, Actor, DomainEvent, DomainEventKind,
    Issue, IssueComment, IssueKind, IssueSource, IssueSourceKind, OperationName, Priority, Project,
    ProjectRole, Requirement, RiskClass, RoleKey, TaskDAO, TaskState, WorkflowId,
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

fn requirement(project_id: Uuid) -> Requirement {
    Requirement::new(
        project_id,
        "部分退款",
        "用户只能全额退款",
        vec![AcceptanceScenario {
            id: "S-1".to_string(),
            title: "部分退款".to_string(),
            given: Vec::new(),
            when: Vec::new(),
            then: vec!["退款金额等于请求金额".to_string()],
        }],
        Actor::human("owner"),
        Priority::P1,
        RiskClass::Medium,
    )
    .expect("valid requirement")
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

    let mut requirement = requirement(project.id);
    RequirementRepository::insert(&store, &requirement)
        .await
        .unwrap();
    requirement.title = "部分退款（更新）".to_string();
    requirement.version = 2;
    RequirementRepository::save(&store, &requirement, 1)
        .await
        .unwrap();
    let stale = RequirementRepository::save(&store, &requirement, 1).await;
    assert!(matches!(
        stale,
        Err(RepoError::Conflict {
            expected: 1,
            actual: 2
        })
    ));

    let found = RequirementRepository::list(
        &store,
        project.id,
        RequirementFilter {
            text: Some("部分退款".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "部分退款（更新）");

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
    assert_eq!(child.stage.as_deref(), Some("work"));

    // The child has no dependencies, so the scheduler wakes it once.
    dispatcher.dispatch_ready().await.unwrap();
    dispatcher.process_pending(100).await.unwrap();
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
            RoleKey::new("acceptor"),
            owner.id,
            owner.id,
        )
        .await
        .unwrap();

    for operation in ["started", "completed", "approved"] {
        tasks
            .send_operation(
                project.id,
                child.id,
                owner.id,
                OperationName::new(operation),
                format!("{}:{operation}", child.id),
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
            format!("{}:approved", child.id),
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
    assert_eq!(root.stage.as_deref(), Some("requirements_discovery"));
    assert_eq!(root.state, TaskState::InProgress);
}
