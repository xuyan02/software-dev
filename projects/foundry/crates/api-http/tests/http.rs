//! End-to-end HTTP tests for authentication, project isolation and roles.

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use foundry_adapter_memory::{
    InMemoryEventSink, InMemoryStore, InMemoryTaskDispatcher, MemoryAgentAdmin,
};
use foundry_adapter_policy::DemoPolicy;
use foundry_api_http::{AppState, SetupState};
use foundry_app::{
    AccountFilter, AccountRepository, AccountService, AuthService, CreateProject,
    IssueRepository, IssueService, ProjectBriefRepository, ProjectBriefService,
    ProjectBriefUseCases, ProjectRepository, ProjectService, RequirementService,
    RequirementsDocumentService, RequirementsDocumentUseCases, TaskDispatcher, TaskLinkService,
    TaskLinkUseCases, TaskRepository, TaskService, WorkflowCatalog, WorkflowService,
    WorkflowUseCases,
};
use foundry_domain::WorkflowId;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

struct Harness {
    router: Router,
    store: Arc<InMemoryStore>,
    admin_token: String,
    admin_id: Uuid,
    project_id: String,
    tasks: Arc<TaskService>,
    dispatcher: Arc<InMemoryTaskDispatcher>,
}

async fn harness() -> Harness {
    harness_with_editor(missing_editor_dir()).await
}

/// A directory that does not exist, so the editor route answers 503.
fn missing_editor_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("foundry-editor-missing-{}", Uuid::now_v7()))
}

/// Builds the console with the workflow editor served from `editor_dir`.
async fn harness_with_editor(editor_dir: std::path::PathBuf) -> Harness {
    let store = Arc::new(InMemoryStore::new());
    let events = Arc::new(InMemoryEventSink::new());
    let policy = Arc::new(DemoPolicy);
    store.seed_builtin_workflows().unwrap();

    let requirements =
        RequirementService::new(Arc::clone(&store), Arc::clone(&events), Arc::clone(&policy));
    let issues = IssueService::new(Arc::clone(&store), Arc::clone(&events), policy);
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));

    let admin = accounts
        .create_first_admin("admin", "Administrator", "admin-password")
        .await
        .unwrap();

    let catalog: Arc<dyn WorkflowCatalog> = store.clone();
    let dispatcher = Arc::new(InMemoryTaskDispatcher::new(Arc::clone(&store)));
    let projects = Arc::new(ProjectService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
        catalog.clone(),
        dispatcher.clone(),
    ));
    let auth = Arc::new(AuthService::new(Arc::clone(&store), Arc::clone(&store), 24));
    let task_repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = Arc::new(TaskService::new(
        task_repository,
        dispatcher.clone(),
        catalog,
    ));
    let workflows_admin: Arc<dyn WorkflowUseCases> = Arc::new(WorkflowService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let requirements_doc: Arc<dyn RequirementsDocumentUseCases> =
        Arc::new(RequirementsDocumentService::new(
            Arc::clone(&store),
            Arc::clone(&store),
            Arc::clone(&store),
            Arc::clone(&events),
        ));
    let project_brief: Arc<dyn ProjectBriefUseCases> = Arc::new(ProjectBriefService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let task_links: Arc<dyn TaskLinkUseCases> = Arc::new(TaskLinkService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));

    let project = projects
        .create(
            admin.actor(),
            CreateProject {
                name: "Primary".to_string(),
                description: None,
                root_workflow_id: WorkflowId::new("project"),
            },
        )
        .await
        .unwrap();
    dispatcher.dispatch_ready().await.unwrap();
    dispatcher.process_pending(100).await.unwrap();

    let router = foundry_api_http::router(AppState {
        requirements: Arc::new(requirements),
        issues: Arc::new(issues),
        accounts,
        projects,
        tasks: tasks.clone(),
        agents: Arc::new(MemoryAgentAdmin),
        agent_logs: Arc::new(foundry_shared::AgentLogFiles::new(
            std::env::temp_dir().join(format!("foundry-agent-logs-{}", Uuid::now_v7())),
        )),
        workflows: workflows_admin,
        requirements_doc,
        project_brief,
        task_links,
        auth,
        setup: Arc::new(SetupState::new(
            false,
            "admin".to_string(),
            "Administrator".to_string(),
            None,
        )),
        editor: Arc::new(foundry_api_http::EditorAssets::new(editor_dir)),
    });

    let token = login(&router, "admin", "admin-password").await;

    Harness {
        router,
        store,
        admin_token: token,
        admin_id: admin.id,
        project_id: project.id.to_string(),
        tasks,
        dispatcher,
    }
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8_lossy(&bytes).to_string())
}

#[tokio::test]
async fn suspended_accounts_lose_their_sessions() {
    let harness = harness().await;

    let member = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({
                "handle": "temporary",
                "display_name": "Temporary",
                "password": "temporary-password"
            }),
        ),
    )
    .await;
    let member_id = member["id"].as_str().unwrap().to_string();

    let token = login(&harness.router, "temporary", "temporary-password").await;
    let (status, _, _) = send(
        &harness.router,
        json_request("GET", "/api/v1/auth/me", Some(&token), None, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    body_json(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/accounts/{member_id}"),
            Some(&harness.admin_token),
            None,
            json!({ "status": "suspended" }),
        ),
    )
    .await;

    let (status, _, _) = send(
        &harness.router,
        json_request("GET", "/api/v1/auth/me", Some(&token), None, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let request = json_request(
        "POST",
        "/api/v1/auth/login",
        None,
        None,
        json!({ "handle": "temporary", "password": "temporary-password" }),
    );
    let (status, _, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn only_owners_can_delete_a_project_and_deletion_removes_its_data() {
    let harness = harness().await;
    create_account(&harness, "observer", "human").await;

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Retired", "root_workflow_id": "project" }),
        ),
    )
    .await;
    let project_id = project["id"].as_str().unwrap().to_string();
    let root_task_id = project["root_task_id"].as_str().unwrap().to_string();

    body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/projects/{project_id}/members"),
            Some(&harness.admin_token),
            None,
            json!({ "handle": "observer", "role": "member" }),
        ),
    )
    .await;
    let observer_token = login(&harness.router, "observer", "observer-password").await;

    let issue = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/issues",
            Some(&harness.admin_token),
            Some(&project_id),
            json!({ "title": "Delete with the project", "description": "scoped" }),
        ),
    )
    .await;
    let issue_id = issue["id"].as_str().unwrap().to_string();

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "DELETE",
            &format!("/api/v1/projects/{project_id}"),
            Some(&observer_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "DELETE",
            &format!("/api/v1/projects/{project_id}"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    assert!(
        IssueRepository::get(&*harness.store, Uuid::parse_str(&issue_id).unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        TaskRepository::get(&*harness.store, Uuid::parse_str(&root_task_id).unwrap())
            .await
            .unwrap()
            .is_none()
    );

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/projects/{project_id}"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/issues/{issue_id}"),
            Some(&harness.admin_token),
            Some(&project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let projects = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert!(
        projects
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value["id"] != project_id.as_str())
    );
}

#[tokio::test]
async fn project_console_delete_redirects_and_clears_the_project_cookie() {
    let harness = harness().await;
    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Console delete", "root_workflow_id": "project" }),
        ),
    )
    .await;
    let project_id = project["id"].as_str().unwrap().to_string();

    let request = Request::builder()
        .method("POST")
        .uri(format!("/projects/{project_id}/delete"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", harness.admin_token),
        )
        .header(
            header::COOKIE,
            format!("foundry_project={project_id}"),
        )
        .body(Body::from(""))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;

    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/projects");
    assert!(
        headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .any(|value| value.contains("foundry_project=;"))
    );
    assert!(ProjectRepository::get(&*harness.store, Uuid::parse_str(&project_id).unwrap())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn only_owners_manage_projects_and_archived_projects_can_be_restored() {
    let harness = harness().await;

    body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({
                "handle": "observer",
                "display_name": "Observer",
                "password": "observer-password"
            }),
        ),
    )
    .await;

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Payments", "root_workflow_id": "project" }),
        ),
    )
    .await;
    let project_id = project["id"].as_str().unwrap().to_string();

    body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/projects/{project_id}/members"),
            Some(&harness.admin_token),
            None,
            json!({ "handle": "observer", "role": "member" }),
        ),
    )
    .await;

    let observer_token = login(&harness.router, "observer", "observer-password").await;
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/projects/{project_id}"),
            Some(&observer_token),
            None,
            json!({ "name": "Hijacked" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/projects/{project_id}/members"),
            Some(&observer_token),
            None,
            json!({ "handle": "admin", "role": "viewer" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let archived = body_json(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/projects/{project_id}"),
            Some(&harness.admin_token),
            None,
            json!({ "status": "archived" }),
        ),
    )
    .await;
    assert_eq!(archived["status"], "archived");

    let restored = body_json(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/projects/{project_id}"),
            Some(&harness.admin_token),
            None,
            json!({ "name": "Payments Restored", "status": "active" }),
        ),
    )
    .await;
    assert_eq!(restored["status"], "active");
    assert_eq!(restored["name"], "Payments Restored");
}

fn json_request(
    method: &str,
    uri: &str,
    token: Option<&str>,
    project: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(project) = project {
        builder = builder.header("x-foundry-project", project);
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn login(router: &Router, handle: &str, password: &str) -> String {
    let request = json_request(
        "POST",
        "/api/v1/auth/login",
        None,
        None,
        json!({ "handle": handle, "password": password }),
    );
    let (status, _, body) = send(router, request).await;
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    value["token"].as_str().unwrap().to_string()
}

async fn body_json(router: &Router, request: Request<Body>) -> Value {
    let (status, _, body) = send(router, request).await;
    assert!(status.is_success(), "request failed {status}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn error_body(body: &str) -> Value {
    let value: Value = serde_json::from_str(body).unwrap();
    value["error"].clone()
}

async fn create_account(harness: &Harness, handle: &str, kind: &str) -> String {
    let account = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({
                "handle": handle,
                "display_name": handle,
                "kind": kind,
                "password": format!("{handle}-password"),
            }),
        ),
    )
    .await;
    account["id"].as_str().unwrap().to_string()
}

/// A request carrying an optional `If-Match: "<revision>"` precondition.
fn workflow_request(
    method: &str,
    uri: &str,
    token: &str,
    revision: Option<&str>,
    body: Value,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(revision) = revision {
        builder = builder.header(header::IF_MATCH, format!("\"{revision}\""));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

async fn copy_platform_workflow(router: &Router, token: &str, title: &str) -> String {
    let created = body_json(
        router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(token),
            None,
            json!({
                "title": title,
                "copy_from": { "workflow_id": "iteration", "version": 1 },
            }),
        ),
    )
    .await;
    created["definition"]["id"].as_str().unwrap().to_string()
}

fn simple_body() -> Value {
    json!({
        "schema_version": 1,
        "roles": [{ "name": "actor", "kind": "any", "min_actors": 1, "max_actors": null }],
        "initial_stage": "work",
        "stages": [
            {
                "name": "work",
                "active_roles": ["actor"],
                "vars": { "done": { "type": "bool", "display_name": "Done" } },
                "operations": [{
                    "name": "finish",
                    "display_name": "Finish",
                    "description": "Finish the task.",
                    "roles": ["actor"],
                    "action": "done = true"
                }],
                "transitions": [{
                    "on": "done",
                    "action": "result = 'done'",
                    "to": "ended"
                }]
            }
        ]
    })
}

fn child_body(workflow: &str) -> Value {
    json!({
        "schema_version": 1,
        "roles": [{ "name": "actor", "kind": "any", "min_actors": 1, "max_actors": null }],
        "initial_stage": "start",
        "stages": [
            {
                "name": "start",
                "active_roles": ["actor"],
                "vars": { "done": { "type": "bool", "display_name": "Done" } },
                "created": [{ "create_child": {
                    "workflow": workflow,
                    "title": format!("Child {{count_children('{workflow}')}}")
                }}],
                "operations": [{
                    "name": "run",
                    "display_name": "Run",
                    "description": "Create the child and continue.",
                    "roles": ["actor"],
                    "action": "done = true"
                }],
                "transitions": [{
                    "on": "done",
                    "action": "result = 'done'",
                    "to": "ended"
                }]
            }
        ]
    })
}

#[tokio::test]
async fn unauthenticated_requests_are_rejected() {
    let harness = harness().await;

    let (status, _, _) = send(
        &harness.router,
        json_request("GET", "/api/v1/issues", None, None, json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, headers, _) = send(
        &harness.router,
        Request::builder().uri("/").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers.get(header::LOCATION).unwrap(), "/login");
}

#[tokio::test]
async fn login_and_logout_revoke_the_session() {
    let harness = harness().await;

    let me = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/auth/me",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(me["handle"], "admin");
    assert_eq!(me["role"], "admin");

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/auth/logout",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/auth/me",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn task_console_lists_the_tree_and_links_the_issue() {
    let harness = harness().await;

    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("handle=admin&password=admin-password&next=%2F"))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let list = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let child_id = list
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["workflow"] == "project_brief_writing")
        .and_then(|task| task["id"].as_str())
        .unwrap()
        .to_string();

    let request = Request::builder()
        .uri("/tasks")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("撰写项目简报"));
    assert!(body.contains("project_brief_writing"));
    assert!(body.contains("智能体"));

    let request = Request::builder()
        .uri(format!("/tasks/{child_id}"))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("撰写项目简报"));
    assert!(body.contains("Project clarification: Primary"));
    assert!(body.contains("打开议题"));
}

#[tokio::test]
async fn project_brief_console_saves_submits_and_approves() {
    let harness = harness().await;

    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("handle=admin&password=admin-password&next=%2F"))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let project_id = harness.project_id.clone();
    let page_uri = format!("/projects/{project_id}/brief");

    let get_page = |uri: &str| {
        Request::builder()
            .uri(uri)
            .header(header::COOKIE, &cookie)
            .body(Body::empty())
            .unwrap()
    };
    let (status, _, body) = send(&harness.router, get_page(&page_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("项目简报"));
    assert!(body.contains("我的草稿"));
    assert!(body.contains("新建草稿"));

    let request = Request::builder()
        .method("POST")
        .uri(format!("{page_uri}/drafts"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &cookie)
        .body(Body::from(""))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let draft_uri = headers[header::LOCATION].to_str().unwrap().to_string();
    assert!(draft_uri.starts_with(&format!("{page_uri}?draft=")));
    let (status, _, body) = send(&harness.router, get_page(&draft_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("编辑草稿"));
    assert!(body.contains("非目标"));
    let draft_id = draft_uri
        .split_once("?draft=")
        .unwrap()
        .1
        .to_string();

    let fields = |revision: &str, action: &str| -> Vec<(String, String)> {
        vec![
            ("draft_id".to_string(), draft_id.clone()),
            ("revision".to_string(), revision.to_string()),
            ("action".to_string(), action.to_string()),
            ("problem".to_string(), "客服只能整单退款".to_string()),
            ("goals".to_string(), "支持部分退款".to_string()),
            ("in_scope".to_string(), "部分退款".to_string()),
            ("out_of_scope".to_string(), "多次退款合并".to_string()),
            ("constraints".to_string(), "兼容支付网关".to_string()),
            (
                "risks".to_string(),
                "网关风险 | medium | high | 双写校验 | 张三".to_string(),
            ),
            ("budget_amount".to_string(), "100000".to_string()),
            ("budget_currency".to_string(), "CNY".to_string()),
            ("budget_note".to_string(), "首期".to_string()),
        ]
    };
    let post_page = |revision: &str, action: &str| {
        let body = serde_urlencoded::to_string(fields(revision, action)).unwrap();
        Request::builder()
            .method("POST")
            .uri(page_uri.as_str())
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, &cookie)
            .body(Body::from(body))
            .unwrap()
    };

    let (status, headers, _) = send(&harness.router, post_page("1", "save")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        draft_uri.as_str()
    );
    let (status, _, body) = send(&harness.router, get_page(&draft_uri)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("支持部分退款"));
    assert!(body.contains("多次退款合并"));

    let (status, headers, _) = send(&harness.router, post_page("8", "submit")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION].to_str().unwrap(),
        page_uri.as_str()
    );
    let (_, _, body) = send(&harness.router, get_page(&page_uri)).await;
    assert!(body.contains("待批准"));
    assert!(body.contains("支持部分退款"));

    let annotation = serde_urlencoded::to_string([
        ("target_type", "goals"),
        ("body", "目标需要可衡量"),
    ])
    .unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("{page_uri}/annotations"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &cookie)
        .body(Body::from(annotation))
        .unwrap();
    let (status, _, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, body) = send(&harness.router, get_page(&page_uri)).await;
    assert!(body.contains("待处理批注（1）"));
    assert!(body.contains("目标需要可衡量"));

    let annotations =
        ProjectBriefRepository::annotations(&*harness.store, project_id.parse().unwrap(), true)
            .await
            .unwrap();
    let annotation_id = annotations[0].id;
    let resolution = serde_urlencoded::to_string([("resolution", "已补充可衡量目标")]).unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("{page_uri}/annotations/{annotation_id}/resolve"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &cookie)
        .body(Body::from(resolution))
        .unwrap();
    let (status, _, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, body) = send(&harness.router, get_page(&page_uri)).await;
    assert!(body.contains("待处理批注（0）"));
    assert!(body.contains("历史批注（1）"));
    assert!(body.contains("已补充可衡量目标"));

    let body = serde_urlencoded::to_string([("version", "1")]).unwrap();
    let request = Request::builder()
        .method("POST")
        .uri(format!("{page_uri}/approve"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::COOKIE, &cookie)
        .body(Body::from(body))
        .unwrap();
    let (status, _, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, _, body) = send(&harness.router, get_page(&page_uri)).await;
    assert!(body.contains("已批准"));
}

#[tokio::test]
async fn agents_page_is_admin_only() {
    let harness = harness().await;

    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("handle=admin&password=admin-password&next=%2F"))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let request = Request::builder()
        .uri("/agents")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("智能体由平台定义"));
    assert!(body.contains("平台还没有注册智能体"));

    let request = Request::builder()
        .uri(format!("/agents/{}/logs", Uuid::now_v7()))
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn web_login_sets_a_cookie_session() {
    let harness = harness().await;

    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("handle=admin&password=admin-password&next=%2F"))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(cookie.starts_with("foundry_session="));

    let request = Request::builder()
        .uri("/")
        .header(header::COOKIE, cookie.split(';').next().unwrap())
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("工作台"));
}

#[tokio::test]
async fn workflows_console_lists_the_platform_catalog() {
    let harness = harness().await;

    let request = Request::builder()
        .method("POST")
        .uri("/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(
            "handle=admin&password=admin-password&next=%2Fworkflows",
        ))
        .unwrap();
    let (status, headers, _) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let cookie = headers
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let request = Request::builder()
        .uri("/workflows")
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("新建流程"));
    assert!(body.contains("Project lifecycle"));
    assert!(body.contains("Iteration"));
    assert!(body.contains("/workflows/editor/iteration"));
}

#[tokio::test]
async fn the_editor_answers_503_until_it_is_built() {
    let harness = harness().await;
    let request = Request::get("/workflows/editor/workflows/iteration")
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(&harness.router, request).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("not built"), "{body}");
}

#[tokio::test]
async fn the_editor_serves_assets_and_falls_back_to_the_spa() {
    let dir = std::env::temp_dir().join(format!("foundry-editor-{}", Uuid::now_v7()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><div id=root>编辑器</div>",
    )
    .unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log('editor')").unwrap();
    let harness = harness_with_editor(dir.clone()).await;

    for (path, expected) in [
        ("/workflows/editor/", "<div id=root>"),
        ("/workflows/editor/assets/app.js", "console.log"),
        ("/workflows/editor/workflows/iteration", "<div id=root>"),
    ] {
        let request = Request::get(path).body(Body::empty()).unwrap();
        let (status, _, body) = send(&harness.router, request).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains(expected), "{path}: {body}");
    }

    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn issues_are_isolated_between_projects() {
    let harness = harness().await;

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Payments", "root_workflow_id": "project" }),
        ),
    )
    .await;
    let other_id = project["id"].as_str().unwrap();

    let issue = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/issues",
            Some(&harness.admin_token),
            Some(harness.project_id.as_str()),
            json!({ "title": "Only here", "description": "primary project" }),
        ),
    )
    .await;
    let issue_id = issue["id"].as_str().unwrap();
    let issue_project = issue["project_id"].as_str().unwrap();
    assert_eq!(issue_project, harness.project_id.as_str());

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/issues/{issue_id}"),
            Some(&harness.admin_token),
            Some(other_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let listed = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/issues",
            Some(&harness.admin_token),
            Some(other_id),
            json!({}),
        ),
    )
    .await;
    // The other project has its own clarification Issue, but it must not see
    // the primary project's issue.
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .all(|value| { value["project_id"] == other_id && value["id"] != issue_id })
    );

    let listed = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/issues",
            Some(&harness.admin_token),
            Some(harness.project_id.as_str()),
            json!({}),
        ),
    )
    .await;
    assert!(
        listed
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["id"] == issue_id)
    );
}

#[tokio::test]
async fn project_roles_limit_contributions_and_visibility() {
    let harness = harness().await;

    body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({
                "handle": "reviewer",
                "display_name": "Reviewer",
                "kind": "agent",
                "role": "member",
                "password": "reviewer-password"
            }),
        ),
    )
    .await;

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Payments", "root_workflow_id": "project" }),
        ),
    )
    .await;
    let project_id = project["id"].as_str().unwrap();

    body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/projects/{project_id}/members"),
            Some(&harness.admin_token),
            None,
            json!({ "handle": "reviewer", "role": "viewer" }),
        ),
    )
    .await;

    let viewer_token = login(&harness.router, "reviewer", "reviewer-password").await;

    let projects = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/projects",
            Some(&viewer_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(projects.as_array().unwrap().len(), 1);
    assert_eq!(projects[0]["name"], "Payments");

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/issues",
            Some(&viewer_token),
            Some(project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // An explicit project the account cannot see is not silently replaced by
    // another visible project.
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/issues",
            Some(&viewer_token),
            Some(harness.project_id.as_str()),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/issues",
            Some(&viewer_token),
            Some(project_id),
            json!({ "title": "Not allowed", "description": "viewer" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn only_admins_manage_accounts() {
    let harness = harness().await;

    let member = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({
                "handle": "member",
                "display_name": "Member",
                "password": "member-password"
            }),
        ),
    )
    .await;
    assert_eq!(member["role"], "member");

    let member_token = login(&harness.router, "member", "member-password").await;
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/accounts",
            Some(&member_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let accounts = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/accounts",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(accounts.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn mcp_requires_auth_and_uses_the_authenticated_project() {
    let harness = harness().await;

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            "/mcp",
            None,
            Some(harness.project_id.as_str()),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let tools = body_json(
        &harness.router,
        json_request(
            "POST",
            "/mcp",
            Some(&harness.admin_token),
            Some(harness.project_id.as_str()),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
        ),
    )
    .await;
    let schema = &tools["result"]["tools"][0]["inputSchema"]["properties"];
    assert!(schema.get("actor").is_none());
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"list_workflows"));
    assert!(names.contains(&"get_workflow"));

    let listed = body_json(
        &harness.router,
        json_request(
            "POST",
            "/mcp",
            Some(&harness.admin_token),
            Some(harness.project_id.as_str()),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tools/call",
                "params": { "name": "list_workflows", "arguments": {} }
            }),
        ),
    )
    .await;
    assert_eq!(listed["result"]["isError"], false);
    assert_eq!(
        listed["result"]["structuredContent"]
            .as_array()
            .unwrap()
            .len(),
        9
    );

    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/mcp",
            Some(&harness.admin_token),
            Some(harness.project_id.as_str()),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "create_issue",
                    "arguments": { "title": "From MCP", "description": "agent work" }
                }
            }),
        ),
    )
    .await;
    assert_eq!(created["result"]["isError"], false);
    assert_eq!(
        created["result"]["structuredContent"]["project_id"],
        harness.project_id.as_str()
    );
}

#[tokio::test]
async fn setup_creates_the_first_admin_and_is_unreachable_afterwards() {
    let store = Arc::new(InMemoryStore::new());
    store.seed_builtin_workflows().unwrap();
    let events = Arc::new(InMemoryEventSink::new());
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));
    let catalog: Arc<dyn WorkflowCatalog> = store.clone();
    let dispatcher = Arc::new(InMemoryTaskDispatcher::new(Arc::clone(&store)));
    let projects = Arc::new(ProjectService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
        catalog.clone(),
        dispatcher.clone(),
    ));
    let auth = Arc::new(AuthService::new(Arc::clone(&store), Arc::clone(&store), 24));
    let task_repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = Arc::new(TaskService::new(
        task_repository,
        dispatcher.clone(),
        catalog,
    ));
    let workflows_admin: Arc<dyn WorkflowUseCases> = Arc::new(WorkflowService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let requirements_doc: Arc<dyn RequirementsDocumentUseCases> =
        Arc::new(RequirementsDocumentService::new(
            Arc::clone(&store),
            Arc::clone(&store),
            Arc::clone(&store),
            Arc::clone(&events),
        ));
    let project_brief: Arc<dyn ProjectBriefUseCases> = Arc::new(ProjectBriefService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let task_links: Arc<dyn TaskLinkUseCases> = Arc::new(TaskLinkService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let requirements = RequirementService::new(
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::new(DemoPolicy),
    );
    let issues = IssueService::new(
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::new(DemoPolicy),
    );

    // Setup creates the administrator only. It must not invent a project.
    let setup = Arc::new(SetupState::new(
        true,
        "admin".to_string(),
        "Administrator".to_string(),
        Some("setup-pass"),
    ));
    let router = foundry_api_http::router(AppState {
        requirements: Arc::new(requirements),
        issues: Arc::new(issues),
        accounts: accounts.clone(),
        projects,
        tasks,
        agents: Arc::new(MemoryAgentAdmin),
        agent_logs: Arc::new(foundry_shared::AgentLogFiles::new(
            std::env::temp_dir().join(format!("foundry-agent-logs-{}", Uuid::now_v7())),
        )),
        workflows: workflows_admin,
        requirements_doc,
        project_brief,
        task_links,
        auth: auth.clone(),
        setup: Arc::clone(&setup),
        editor: Arc::new(foundry_api_http::EditorAssets::new(missing_editor_dir())),
    });

    // Every route is blocked and API calls get a 503.
    let (status, headers, _) = send(&router, Request::get("/").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/setup");

    let (status, headers, _) = send(
        &router,
        Request::get("/api/v1/issues").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");

    // Step one asks for the configured initial identity, not the administrator
    // credentials yet.
    let (status, _, body) =
        send(&router, Request::get("/setup").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("初始账号"));
    assert!(body.contains("登录初始化"));
    assert!(!body.contains("管理员密码"));

    // A wrong initial account or password is rejected and never echoed.
    let (status, _, body) = send(
        &router,
        Request::post("/setup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("handle=admin&password=not-the-password"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("初始账号或密码不正确"));
    assert!(!body.contains("not-the-password"));
    assert!(accounts.setup_required().await.unwrap());

    let (status, _, body) = send(
        &router,
        Request::post("/setup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("handle=not-admin&password=setup-pass"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("初始账号或密码不正确"));
    assert!(accounts.setup_required().await.unwrap());

    // The configured initial identity signs in and receives a short-lived
    // ticket. This step does not create an account.
    let (status, headers, _) = send(
        &router,
        Request::post("/setup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("handle=admin&password=setup-pass"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/setup");
    let cookie = headers
        .get(header::SET_COOKIE)
        .expect("setup ticket cookie")
        .to_str()
        .unwrap();
    assert!(cookie.contains("foundry_setup="));
    let ticket = cookie.split(';').next().unwrap().to_string();
    assert!(accounts.setup_required().await.unwrap());

    // With the ticket, the same page offers the real administrator form.
    let (status, _, body) = send(
        &router,
        Request::get("/setup")
            .header(header::COOKIE, ticket.as_str())
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("创建管理员"));
    assert!(body.contains("管理员密码"));
    assert!(body.contains("action=\"/setup/admin\""));

    // Without the ticket, the creation endpoint is not reachable.
    let (status, headers, _) = send(
        &router,
        Request::post("/setup/admin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(
                "handle=admin&display_name=Administrator&password=first-password&confirm_password=first-password",
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/setup");
    assert!(accounts.setup_required().await.unwrap());

    // Mismatched confirmation is rejected without creating anything.
    let (status, _, body) = send(
        &router,
        Request::post("/setup/admin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, ticket.as_str())
            .body(Body::from(
                "handle=admin&display_name=Administrator&password=first-password&confirm_password=other",
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("两次输入的密码不一致"));
    assert!(accounts.setup_required().await.unwrap());

    // A valid creation makes the real administrator and a login session and
    // closes setup. The operator chooses the first project separately.
    let (status, headers, _) = send(
        &router,
        Request::post("/setup/admin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, ticket.as_str())
            .body(Body::from(
                "handle=admin&display_name=Administrator&password=first-password&confirm_password=first-password",
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/projects");
    let cookies = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>();
    assert!(
        cookies
            .iter()
            .any(|value| value.contains("foundry_session="))
    );
    assert!(
        cookies
            .iter()
            .any(|value| value.contains("foundry_setup=") && value.contains("Max-Age=0")),
        "the setup ticket must be cleared after creation"
    );
    assert!(!setup.is_required());

    let all = AccountRepository::list(&*store, AccountFilter::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].is_admin());
    assert!(auth.login("admin", "first-password").await.is_ok());
    assert!(
        auth.login("admin", "setup-pass").await.is_err(),
        "the initial credentials must not become the admin password"
    );

    let all_projects = ProjectRepository::list(&*store, Default::default())
        .await
        .unwrap();
    assert!(
        all_projects.is_empty(),
        "setup must not create a project; the administrator creates the first one explicitly"
    );

    // Setup is gone: both steps redirect to login, and the old ticket is dead.
    let (status, headers, _) =
        send(&router, Request::get("/setup").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/login");

    let (status, headers, _) = send(
        &router,
        Request::post("/setup")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("handle=admin&password=setup-pass"))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/login");

    let (status, headers, _) = send(
        &router,
        Request::post("/setup/admin")
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(header::COOKIE, ticket.as_str())
            .body(Body::from(
                "handle=other&display_name=Other&password=second-password&confirm_password=second-password",
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/login");
    assert_eq!(
        AccountRepository::list(&*store, AccountFilter::default())
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn create_first_admin_requires_an_empty_account_table() {
    let store = Arc::new(InMemoryStore::new());
    let events = Arc::new(InMemoryEventSink::new());
    let accounts = AccountService::new(Arc::clone(&store), Arc::clone(&events));

    assert!(accounts.setup_required().await.unwrap());
    assert!(
        accounts
            .create_first_admin("admin", "Administrator", "")
            .await
            .is_err(),
        "an empty password must be rejected"
    );

    let admin = accounts
        .create_first_admin("admin", "Administrator", "first-password")
        .await
        .unwrap();
    assert!(admin.is_admin());
    assert!(!accounts.setup_required().await.unwrap());

    assert!(
        accounts
            .create_first_admin("other", "Other", "second-password")
            .await
            .is_err(),
        "setup must never run once accounts exist"
    );

    let auth = AuthService::new(Arc::clone(&store), Arc::clone(&store), 24);
    assert!(auth.login("admin", "first-password").await.is_ok());

    let all = AccountRepository::list(&*store, AccountFilter::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn project_creation_creates_the_root_and_project_brief_writing() {
    let harness = harness().await;

    let list = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let tasks = list.as_array().unwrap();
    assert_eq!(tasks.len(), 2);

    let root = tasks
        .iter()
        .find(|task| task["parent_id"].is_null())
        .expect("root task");
    assert_eq!(root["workflow"], "project");
    assert_eq!(root["workflow_version"], 1);
    assert_eq!(root["state"], "in_progress");
    assert_eq!(root["stage"], "goal");
    assert_eq!(root["dispatch_state"], "running");

    let child = tasks
        .iter()
        .find(|task| task["workflow"] == "project_brief_writing")
        .expect("goal clarification child");
    assert_eq!(child["parent_id"], root["id"]);
    assert_eq!(child["state"], "pending");
    assert_eq!(child["stage"], "clarify");
    assert_eq!(child["dispatch_state"], "ready");

    let detail = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{}", child["id"].as_str().unwrap()),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(detail["allowed_operations"], json!([]));
    assert_eq!(detail["actors"], json!([]));
    assert_eq!(detail["role_specs"].as_array().unwrap().len(), 5);

    let ready = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks?dispatch_state=ready",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(ready.as_array().unwrap().len(), 1);
    assert_eq!(ready[0]["workflow"], "project_brief_writing");

    // The child's `task.created` action creates the clarification Issue and
    // links it to the Task.
    let issues = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/issues",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let issue = issues
        .as_array()
        .unwrap()
        .iter()
        .find(|issue| issue["title"] == "Project clarification: Primary")
        .expect("clarification issue");
    assert_eq!(issue["kind"], "question");
    assert_eq!(issue["status"], "open");

    let links = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{}/links", child["id"].as_str().unwrap()),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    assert!(links.as_array().unwrap().iter().any(|link| {
        link["kind"] == "issue"
            && link["relation"] == "clarifies"
            && link["target_id"] == issue["id"]
    }));
}

#[tokio::test]
async fn project_creation_rejects_an_unknown_root_workflow() {
    let harness = harness().await;
    let (status, _, body) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Payments", "root_workflow_id": "missing" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(body["error"]["code"], "workflow_not_found");
}

#[tokio::test]
async fn task_work_acceptance_flow_advances_the_parent() {
    let harness = harness().await;
    let project_id: Uuid = harness.project_id.parse().unwrap();
    let list = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let child_id: Uuid = list
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["workflow"] == "project_brief_writing")
        .and_then(|task| task["id"].as_str())
        .unwrap()
        .parse()
        .unwrap();
    let input_uri = format!("/api/v1/tasks/{child_id}/operations");

    // No actor is bound yet, so the public API rejects the operation.
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            &input_uri,
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({ "operation": "started", "idempotency_key": "early" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    harness
        .tasks
        .bind_actor(
            project_id,
            child_id,
            foundry_domain::RoleKey::new("clarifier"),
            harness.admin_id,
            harness.admin_id,
        )
        .await
        .unwrap();
    harness
        .tasks
        .bind_actor(
            project_id,
            child_id,
            foundry_domain::RoleKey::new("brief_writer"),
            harness.admin_id,
            harness.admin_id,
        )
        .await
        .unwrap();
    harness
        .tasks
        .bind_actor(
            project_id,
            child_id,
            foundry_domain::RoleKey::new("style_reviewer"),
            harness.admin_id,
            harness.admin_id,
        )
        .await
        .unwrap();
    harness
        .tasks
        .bind_actor(
            project_id,
            child_id,
            foundry_domain::RoleKey::new("content_reviewer"),
            harness.admin_id,
            harness.admin_id,
        )
        .await
        .unwrap();
    harness
        .tasks
        .bind_actor(
            project_id,
            child_id,
            foundry_domain::RoleKey::new("acceptor"),
            harness.admin_id,
            harness.admin_id,
        )
        .await
        .unwrap();

    let allowed = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{child_id}/allowed-operations"),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let allowed_names: Vec<&str> = allowed
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| operation["name"].as_str().unwrap())
        .collect();
    assert_eq!(allowed_names, vec!["started", "completed"]);

    for (index, operation) in [
        "started",   // clarify
        "completed", // clarify → brief
        "completed", // brief → style_review
        "approved",  // style_review → content_review
        "approved",  // content_review → acceptance
        "approved",  // acceptance → completed
    ]
    .iter()
    .enumerate()
    {
        let (status, _, body) = send(
            &harness.router,
            json_request(
                "POST",
                &input_uri,
                Some(&harness.admin_token),
                Some(&harness.project_id),
                json!({ "operation": operation, "idempotency_key": format!("{child_id}:{index}:{operation}") }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{operation}: {body}");
    }

    // The same idempotency key is a successful no-op even after completion.
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            &input_uri,
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({ "operation": "approved", "idempotency_key": format!("{child_id}:5:approved") }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let detail = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{child_id}"),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(detail["state"], "completed");
    assert_eq!(detail["result"], "approved");
    assert_eq!(detail["dispatch_state"], "closed");
    assert_eq!(detail["allowed_operations"], json!([]));
    let actors = detail["actors"].as_array().unwrap();
    assert_eq!(actors.len(), 5);
    assert!(actors.iter().all(|actor| actor["active"] == false));

    let history = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{child_id}/history"),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let history = history.as_array().unwrap();
    assert_eq!(history[0]["kind"], "task_completed");
    assert!(
        history
            .iter()
            .any(|operation| operation["kind"] == "operation_received")
    );
    assert!(
        history
            .iter()
            .any(|operation| operation["kind"] == "actor_bound")
    );

    // The parent consumes ChildStateChanged, advances its stage and creates the
    // next lifecycle Task.
    harness.dispatcher.process_pending(100).await.unwrap();
    harness.dispatcher.process_pending(100).await.unwrap();

    let root = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let root = root.as_array().unwrap();
    let root = root
        .iter()
        .find(|task| task["parent_id"].is_null())
        .unwrap();
    assert_eq!(root["stage"], "requirements_analysis");

    let ready = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks?dispatch_state=ready",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let ready = ready.as_array().unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0]["workflow"], "requirements_analysis");
    assert_eq!(ready[0]["stage"], "work");

    // Comments are independent of the workflow and remain writable after
    // completion.
    let comment = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/tasks/{child_id}/comments"),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({ "body": "goals confirmed" }),
        ),
    )
    .await;
    assert_eq!(comment["comments"].as_array().unwrap().len(), 1);
    let comments = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/tasks/{child_id}/comments"),
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    assert_eq!(comments[0]["body"], "goals confirmed");
}

#[tokio::test]
async fn workflow_definitions_are_writable_only_by_their_owner() {
    let harness = harness().await;
    let author_id = create_account(&harness, "author", "human").await;
    create_account(&harness, "stranger", "human").await;
    create_account(&harness, "automation", "agent").await;
    let author = login(&harness.router, "author", "author-password").await;
    let stranger = login(&harness.router, "stranger", "stranger-password").await;
    let agent = login(&harness.router, "automation", "automation-password").await;

    // Platform definitions are readable by every human but writable only by
    // administrators.
    let platform = body_json(
        &harness.router,
        json_request("GET", "/api/v1/workflows", Some(&author), None, json!({})),
    )
    .await;
    let platform = platform.as_array().unwrap();
    assert_eq!(platform.len(), 9);
    assert!(
        platform
            .iter()
            .all(|item| item["owner"]["kind"] == "platform")
    );
    assert!(platform.iter().all(|item| item["editable"] == false));

    let (status, _, body) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows/iteration/draft",
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_body(&body)["code"], "workflow_forbidden");

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&author),
            None,
            json!({ "title": "Global", "scope": "platform" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Agents never author definitions.
    let (status, _, body) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&agent),
            None,
            json!({ "title": "Agent flow" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_body(&body)["code"], "workflow_forbidden");

    // Copying a platform version creates an account-owned workflow.
    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&author),
            None,
            json!({
                "title": "Custom iteration",
                "copy_from": { "workflow_id": "iteration", "version": 1 },
            }),
        ),
    )
    .await;
    let id = created["definition"]["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("wf_"));
    assert_eq!(created["definition"]["owner"]["kind"], "account");
    assert_eq!(created["definition"]["owner"]["account_id"], author_id);
    assert!(created["definition"]["latest_version"].is_null());
    assert_eq!(created["definition"]["draft_revision"], 1);
    assert_eq!(created["definition"]["editable"], true);
    assert_eq!(created["draft"]["revision"], 1);
    assert_eq!(
        created["draft"]["body"]["stages"].as_array().unwrap().len(),
        4
    );

    // Another account cannot see it at all.
    let (status, _, body) = send(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}"),
            Some(&stranger),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_body(&body)["code"], "workflow_not_found");

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/workflows/{id}/draft"),
            Some(&stranger),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // An administrator sees the definition and its draft, but cannot edit it.
    let admin_view = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(admin_view["editable"], false);
    // Administrators may read a private draft even though they cannot write it.
    assert_eq!(admin_view["draft_revision"], 1);

    let admin_draft = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}/draft"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(admin_draft["revision"], 1);

    let (status, _, body) = send(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/workflows/{id}"),
            Some(&harness.admin_token),
            None,
            json!({ "title": "Hijacked" }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_body(&body)["code"], "workflow_forbidden");

    // The owner edits the shared definition.
    let renamed = body_json(
        &harness.router,
        json_request(
            "PATCH",
            &format!("/api/v1/workflows/{id}"),
            Some(&author),
            None,
            json!({ "title": "Custom iteration v2" }),
        ),
    )
    .await;
    assert_eq!(renamed["title"], "Custom iteration v2");

    let own = body_json(
        &harness.router,
        json_request("GET", "/api/v1/workflows", Some(&author), None, json!({})),
    )
    .await;
    assert_eq!(own.as_array().unwrap().len(), 10);
}

#[tokio::test]
async fn draft_saves_are_idempotent_and_publish_pins_a_version() {
    let harness = harness().await;
    create_account(&harness, "author", "human").await;
    let author = login(&harness.router, "author", "author-password").await;
    let id = copy_platform_workflow(&harness.router, &author, "Authored").await;
    let draft_uri = format!("/api/v1/workflows/{id}/draft");

    let client_save_id = Uuid::now_v7();
    let save = |revision: Option<&str>, client_save_id: Uuid| {
        workflow_request(
            "PUT",
            &draft_uri,
            &author,
            revision,
            json!({ "body": simple_body(), "client_save_id": client_save_id }),
        )
    };

    let saved = body_json(&harness.router, save(Some("1"), client_save_id)).await;
    assert_eq!(saved["revision"], 2);

    // Retrying the same save id is a no-op even with the old revision.
    let retried = body_json(&harness.router, save(Some("1"), client_save_id)).await;
    assert_eq!(retried["revision"], 2);

    let (status, _, body) = send(&harness.router, save(Some("1"), Uuid::now_v7())).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let error = error_body(&body);
    assert_eq!(error["code"], "draft_conflict");
    assert_eq!(error["current_revision"], 2);

    let (status, _, body) = send(&harness.router, save(None, Uuid::now_v7())).await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(error_body(&body)["code"], "precondition_required");

    let malformed = Request::builder()
        .method("PUT")
        .uri(&draft_uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, format!("Bearer {author}"))
        .header(header::IF_MATCH, "1")
        .body(Body::from(
            json!({ "body": simple_body(), "client_save_id": Uuid::now_v7() }).to_string(),
        ))
        .unwrap();
    let (status, _, body) = send(&harness.router, malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error_body(&body)["code"], "invalid_if_match");

    let draft = body_json(
        &harness.router,
        json_request("GET", &draft_uri, Some(&author), None, json!({})),
    )
    .await;
    assert_eq!(draft["revision"], 2);
    assert_eq!(draft["body"]["stages"][0]["name"], "work");

    let layout = json!({ "positions": { "work": { "x": 12.0, "y": 34.0 } } });
    let layout_uri = format!("/api/v1/workflows/{id}/layout");
    let saved_layout = body_json(
        &harness.router,
        workflow_request("PUT", &layout_uri, &author, None, layout.clone()),
    )
    .await;
    assert_eq!(saved_layout, layout);
    let loaded_layout = body_json(
        &harness.router,
        json_request("GET", &layout_uri, Some(&author), None, json!({})),
    )
    .await;
    assert_eq!(loaded_layout, layout);

    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{id}/publish"),
            &author,
            Some("2"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);
    assert_eq!(published["revision"], 3);
    assert_eq!(published["warnings"], json!([]));

    let (status, _, body) = send(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{id}/publish"),
            &author,
            Some("2"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let error = error_body(&body);
    assert_eq!(error["code"], "draft_conflict");
    assert_eq!(error["current_revision"], 3);

    let (status, _, body) = send(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{id}/publish"),
            &author,
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(error_body(&body)["code"], "precondition_required");

    let versions = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}/versions"),
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    let versions = versions.as_array().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["version"], 1);
    assert_eq!(versions[0]["published_by_display_name"], "author");

    let version = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}/versions/1"),
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(version["body"]["stages"][0]["name"], "work");

    let definition = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}"),
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(definition["latest_version"], 1);
    assert_eq!(definition["draft_revision"], 3);
}

#[tokio::test]
async fn publish_accepts_platform_references_and_rejects_other_accounts() {
    let harness = harness().await;
    create_account(&harness, "alice", "human").await;
    create_account(&harness, "bob", "human").await;
    let alice = login(&harness.router, "alice", "alice-password").await;
    let bob = login(&harness.router, "bob", "bob-password").await;

    let alice_flow = copy_platform_workflow(&harness.router, &alice, "Alice flow").await;
    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{alice_flow}/publish"),
            &alice,
            Some("1"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);

    let bob_flow = copy_platform_workflow(&harness.router, &bob, "Bob flow").await;
    let validate_uri = format!("/api/v1/workflows/{bob_flow}/validate");

    let validation = body_json(
        &harness.router,
        json_request(
            "POST",
            &validate_uri,
            Some(&bob),
            None,
            json!({ "body": child_body(&alice_flow) }),
        ),
    )
    .await;
    assert!(
        validation["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "unknown_workflow")
    );

    let validation = body_json(
        &harness.router,
        json_request(
            "POST",
            &validate_uri,
            Some(&bob),
            None,
            json!({ "body": child_body("iteration") }),
        ),
    )
    .await;
    assert_eq!(validation["errors"], json!([]));

    // Saving is allowed; publishing enforces the owner scope.
    let draft_uri = format!("/api/v1/workflows/{bob_flow}/draft");
    let saved = body_json(
        &harness.router,
        workflow_request(
            "PUT",
            &draft_uri,
            &bob,
            Some("1"),
            json!({ "body": child_body(&alice_flow), "client_save_id": Uuid::now_v7() }),
        ),
    )
    .await;
    assert_eq!(saved["revision"], 2);

    let (status, _, body) = send(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{bob_flow}/publish"),
            &bob,
            Some("2"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let error = error_body(&body);
    assert_eq!(error["code"], "workflow_invalid");
    assert!(
        error["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue["code"] == "unknown_workflow")
    );

    let saved = body_json(
        &harness.router,
        workflow_request(
            "PUT",
            &draft_uri,
            &bob,
            Some("2"),
            json!({ "body": child_body("iteration"), "client_save_id": Uuid::now_v7() }),
        ),
    )
    .await;
    assert_eq!(saved["revision"], 3);
    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{bob_flow}/publish"),
            &bob,
            Some("3"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);
}

#[tokio::test]
async fn administrators_publish_platform_workflows_for_everyone() {
    let harness = harness().await;
    create_account(&harness, "author", "human").await;
    let author = login(&harness.router, "author", "author-password").await;

    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&harness.admin_token),
            None,
            json!({
                "title": "Platform iteration",
                "scope": "platform",
                "copy_from": { "workflow_id": "iteration", "version": 1 },
            }),
        ),
    )
    .await;
    let id = created["definition"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["definition"]["owner"]["kind"], "platform");
    assert_eq!(created["definition"]["editable"], true);

    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{id}/publish"),
            &harness.admin_token,
            Some("1"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);

    let visible = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("/api/v1/workflows/{id}"),
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(visible["editable"], false);
    assert!(visible["draft_revision"].is_null());

    let (status, _, body) = send(
        &harness.router,
        json_request(
            "POST",
            &format!("/api/v1/workflows/{id}/draft"),
            Some(&author),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_body(&body)["code"], "workflow_forbidden");
}

#[tokio::test]
async fn project_creation_requires_a_published_root_workflow() {
    let harness = harness().await;
    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&harness.admin_token),
            None,
            json!({ "title": "Account flow", "scope": "account" }),
        ),
    )
    .await;
    let id = created["definition"]["id"].as_str().unwrap();

    let (status, _, body) = send(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Blocked", "root_workflow_id": id }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_body(&body)["code"], "workflow_unpublished");

    let projects = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(
        projects.as_array().unwrap().len(),
        1,
        "no project is created"
    );
}

#[tokio::test]
async fn a_published_account_workflow_can_root_a_project() {
    let harness = harness().await;
    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&harness.admin_token),
            None,
            json!({ "title": "Account flow", "scope": "account" }),
        ),
    )
    .await;
    let id = created["definition"]["id"].as_str().unwrap();
    let draft_uri = format!("/api/v1/workflows/{id}/draft");

    body_json(
        &harness.router,
        workflow_request(
            "PUT",
            &draft_uri,
            &harness.admin_token,
            Some("1"),
            json!({ "body": simple_body(), "client_save_id": Uuid::now_v7() }),
        ),
    )
    .await;
    let patch = |revision: &str, uri: &str| {
        workflow_request("POST", uri, &harness.admin_token, Some(revision), json!({}))
    };
    body_json(
        &harness.router,
        patch("2", &format!("/api/v1/workflows/{id}/publish")),
    )
    .await;
    let published = body_json(
        &harness.router,
        patch("3", &format!("/api/v1/workflows/{id}/publish")),
    )
    .await;
    assert_eq!(published["version"], 2);

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&harness.admin_token),
            None,
            json!({ "name": "Account project", "root_workflow_id": id }),
        ),
    )
    .await;
    assert!(project["root_task_id"].is_string());

    let tasks = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(project["id"].as_str().unwrap()),
            json!({}),
        ),
    )
    .await;
    let root = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["parent_id"].is_null())
        .expect("root task");
    assert_eq!(root["workflow"], id);
    assert_eq!(root["workflow_version"], 2);
}

/// The editor's lifecycle at the API level: create, edit, save, conflict,
/// publish and finally root a project with the published account workflow.
#[tokio::test]
async fn an_account_workflow_goes_from_creation_to_a_project_root() {
    let harness = harness().await;
    create_account(&harness, "flow_owner", "human").await;
    let token = login(&harness.router, "flow_owner", "flow_owner-password").await;

    let created = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/workflows",
            Some(&token),
            None,
            json!({ "title": "Lifecycle", "scope": "account" }),
        ),
    )
    .await;
    let id = created["definition"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["draft"]["revision"], 1);
    assert_eq!(created["draft"]["body"]["stages"], json!([]));

    let draft_uri = format!("/api/v1/workflows/{id}/draft");
    let can_edit = |revision: &str| {
        workflow_request(
            "PUT",
            &draft_uri,
            &token,
            Some(revision),
            json!({ "body": simple_body(), "client_save_id": Uuid::now_v7() }),
        )
    };
    let saved = body_json(&harness.router, can_edit("1")).await;
    assert_eq!(saved["revision"], 2);

    let (status, _, body) = send(&harness.router, can_edit("1")).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_body(&body)["code"], "draft_conflict");
    assert_eq!(error_body(&body)["current_revision"], 2);

    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("/api/v1/workflows/{id}/publish"),
            &token,
            Some("2"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);
    assert_eq!(published["revision"], 3);

    let project = body_json(
        &harness.router,
        json_request(
            "POST",
            "/api/v1/projects",
            Some(&token),
            None,
            json!({ "name": "Lifecycle project", "root_workflow_id": id }),
        ),
    )
    .await;
    let project_id = project["id"].as_str().unwrap();
    let root_task_id = project["root_task_id"].as_str().unwrap();

    let tasks = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&token),
            Some(project_id),
            json!({}),
        ),
    )
    .await;
    let root = tasks
        .as_array()
        .unwrap()
        .iter()
        .find(|task| task["id"] == root_task_id)
        .expect("root task");
    assert_eq!(root["workflow"], id);
    assert_eq!(root["workflow_version"], 1);
}

#[tokio::test]
async fn requirements_document_round_trips_over_http() {
    let harness = harness().await;
    let base = format!(
        "/api/v1/projects/{}/requirements-document",
        harness.project_id
    );

    let draft = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/draft"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(draft["revision"], 1);

    let user_id = "persona-customer";
    let function_id = "func-shopping";
    let operation_id = "op-search";
    let story_id = "story-search";
    let operations = json!({
        "operations": [
            {
                "op_id": Uuid::now_v7(),
                "type": "user_add",
                "user": {
                    "id": user_id,
                    "name": "张先生",
                    "goals": ["快速找到商品"],
                    "pain_points": ["搜索结果不准确"]
                }
            },
            {
                "op_id": Uuid::now_v7(),
                "type": "function_add",
                "function": { "id": function_id, "name": "购物" }
            },
            {
                "op_id": Uuid::now_v7(),
                "type": "operation_add",
                "function_id": function_id,
                "operation": {
                    "id": operation_id,
                    "name": "搜索",
                    "actor_user_ids": [user_id]
                }
            },
            {
                "op_id": Uuid::now_v7(),
                "type": "story_add",
                "function_id": function_id,
                "story": {
                    "id": story_id,
                    "title": "搜索商品",
                    "user_ids": [user_id],
                    "scenario": "客户想买某件商品",
                    "goal": "找到商品",
                    "operation_ids": [operation_id]
                }
            }
        ]
    });

    let saved = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/draft/operations"),
            &harness.admin_token,
            Some("1"),
            operations.clone(),
        ),
    )
    .await;
    assert_eq!(saved["revision"], 2);
    assert_eq!(saved["body"]["users"][0]["id"], user_id);
    assert_eq!(saved["body"]["functions"][0]["stories"][0]["id"], story_id);

    // Replaying the same operation ids is idempotent even with a stale revision.
    let replay = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/draft/operations"),
            &harness.admin_token,
            Some("1"),
            operations,
        ),
    )
    .await;
    assert_eq!(replay["revision"], 2);

    let validation = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/draft/validate"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(validation["errors"].as_array().unwrap().len(), 0);

    let published = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/draft/publish"),
            &harness.admin_token,
            Some("2"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(published["version"], 1);
    assert_eq!(published["revision"], 3);

    let document = body_json(
        &harness.router,
        json_request("GET", &base, Some(&harness.admin_token), None, json!({})),
    )
    .await;
    assert_eq!(document["current_version"], 1);
    assert_eq!(document["body"]["functions"][0]["name"], "购物");

    let version = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/versions/1"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(version["version"], 1);
    assert_eq!(version["body"]["users"][0]["name"], "张先生");

    let stale_result = send(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/draft/operations"),
            &harness.admin_token,
            Some("1"),
            json!({
                "operations": [{
                    "op_id": Uuid::now_v7(),
                    "type": "user_add",
                    "user": { "id": "persona-second", "name": "李女士" }
                }]
            }),
        ),
    )
    .await;
    assert_eq!(stale_result.0, StatusCode::CONFLICT);
    assert_eq!(error_body(&stale_result.2)["code"], "draft_conflict");
}

#[tokio::test]
async fn project_brief_round_trips_over_http() {
    let harness = harness().await;
    let base = format!("/api/v1/projects/{}/brief", harness.project_id);

    let first = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/drafts"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    let second = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/drafts"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_ne!(first["id"], second["id"]);
    assert_eq!(first["revision"], 1);

    let draft_id = first["id"].as_str().unwrap().to_string();
    let mut revision = 1;
    let operations = [
        json!({ "type": "set_problem", "value": "客服只能整单退款" }),
        json!({ "type": "set_goals", "values": ["支持部分退款"] }),
        json!({
            "type": "set_scope",
            "in_scope": ["部分退款"],
            "out_of_scope": ["多次退款合并"]
        }),
        json!({ "type": "set_constraints", "values": ["兼容支付网关"] }),
        json!({
            "type": "set_terms",
            "values": [{
                "term": "SLA",
                "definition": "服务等级协议"
            }]
        }),
    ];
    for operation in operations {
        let updated = body_json(
            &harness.router,
            json_request(
                "POST",
                &format!("{base}/drafts/{draft_id}/operations"),
                Some(&harness.admin_token),
                None,
                json!({
                    "op_id": Uuid::now_v7(),
                    "base_revision": revision,
                    "operation": operation,
                }),
            ),
        )
        .await;
        revision += 1;
        assert_eq!(updated["revision"], revision);
    }

    let drafts = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/drafts"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(drafts.as_array().unwrap().len(), 2);

    create_account(&harness, "brief-observer", "human").await;
    body_json(
        &harness.router,
        json_request(
            "POST",
            &format!(
                "/api/v1/projects/{}/members",
                harness.project_id
            ),
            Some(&harness.admin_token),
            None,
            json!({ "handle": "brief-observer", "role": "member" }),
        ),
    )
    .await;
    let observer_token = login(&harness.router, "brief-observer", "brief-observer-password").await;
    let (status, _, _) = send(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/drafts/{}", second["id"].as_str().unwrap()),
            Some(&observer_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let observer_drafts = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/drafts"),
            Some(&observer_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(observer_drafts.as_array().unwrap().len(), 0);

    let submitted = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/drafts/{draft_id}/submit"),
            &harness.admin_token,
            Some(&revision.to_string()),
            json!({}),
        ),
    )
    .await;
    assert_eq!(submitted["status"], "in_review");
    assert_eq!(submitted["version"], 1);

    let current = body_json(
        &harness.router,
        json_request(
            "GET",
            &base,
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(current["version"], 1);
    assert_eq!(current["status"], "in_review");

    let versions = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/versions"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(versions.as_array().unwrap().len(), 1);

    let annotation = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/annotations"),
            Some(&harness.admin_token),
            None,
            json!({
                "target_type": "terms",
                "target_id": "SLA",
                "field": "definition",
                "body": "解释需要更清楚",
            }),
        ),
    )
    .await;
    assert_eq!(annotation["status"], "open");
    assert_eq!(annotation["target_id"], "SLA");
    let annotation_id = annotation["id"].as_str().unwrap().to_string();

    let open = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/annotations"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(open.as_array().unwrap().len(), 1);

    let resolved = body_json(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/annotations/{annotation_id}/resolve"),
            Some(&harness.admin_token),
            None,
            json!({ "resolution": "补充了基线说明" }),
        ),
    )
    .await;
    assert_eq!(resolved["status"], "resolved");

    let open = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/annotations"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(open.as_array().unwrap().len(), 0);

    let all = body_json(
        &harness.router,
        json_request(
            "GET",
            &format!("{base}/annotations?include_resolved=true"),
            Some(&harness.admin_token),
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(all.as_array().unwrap().len(), 1);

    let (status, _, _) = send(
        &harness.router,
        json_request(
            "POST",
            &format!("{base}/annotations"),
            Some(&harness.admin_token),
            None,
            json!({
                "target_type": "risks",
                "target_id": "missing",
                "body": "unknown risk",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let approved = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &format!("{base}/approve"),
            &harness.admin_token,
            Some("1"),
            json!({}),
        ),
    )
    .await;
    assert_eq!(approved["status"], "approved");
    assert_eq!(approved["version"], 1);
    assert!(approved["approved_by"].is_string());
}

#[tokio::test]
async fn task_links_round_trip_over_http() {
    let harness = harness().await;
    let tasks = body_json(
        &harness.router,
        json_request(
            "GET",
            "/api/v1/tasks",
            Some(&harness.admin_token),
            Some(&harness.project_id),
            json!({}),
        ),
    )
    .await;
    let task_id = tasks[0]["id"].as_str().unwrap().to_string();
    let base = format!("/api/v1/tasks/{task_id}/links");

    let created = body_json(
        &harness.router,
        workflow_request(
            "POST",
            &base,
            &harness.admin_token,
            None,
            json!({
                "kind": "url",
                "url": "https://example.com/refund-spec",
                "title": "退款规范",
                "relation": "references"
            }),
        ),
    )
    .await;
    assert_eq!(created["kind"], "url");
    assert_eq!(created["title"], "退款规范");
    let link_id = created["id"].as_str().unwrap().to_string();

    let list = body_json(
        &harness.router,
        json_request("GET", &base, Some(&harness.admin_token), None, json!({})),
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);

    let duplicate = send(
        &harness.router,
        workflow_request(
            "POST",
            &base,
            &harness.admin_token,
            None,
            json!({
                "kind": "url",
                "url": "https://example.com/refund-spec",
                "title": "退款规范",
                "relation": "references"
            }),
        ),
    )
    .await;
    assert_eq!(duplicate.0, StatusCode::CONFLICT);

    let removed = send(
        &harness.router,
        workflow_request(
            "DELETE",
            &format!("{base}/{link_id}"),
            &harness.admin_token,
            None,
            json!({}),
        ),
    )
    .await;
    assert_eq!(removed.0, StatusCode::NO_CONTENT);

    let list = body_json(
        &harness.router,
        json_request("GET", &base, Some(&harness.admin_token), None, json!({})),
    )
    .await;
    assert!(list.as_array().unwrap().is_empty());
}
