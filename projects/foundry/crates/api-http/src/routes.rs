use crate::ApiError;
use crate::auth::{self, ApiSession, WebAccountSession, WebSession};
use crate::dto::{
    AccountUpdateForm, AddMemberForm, AddMemberRequest, AddTaskLinkRequest, AgentListQuery,
    AgentTemplateQuery, AssignAgentRequest, BindTaskActorRequest, CommentIssueForm,
    CommentIssueRequest, CreateAccountRequest, CreateAgentFromTemplateRequest,
    CreateInspectionRequest, CreateIssueRequest, CreateMergeRequestForm, CreateMergeRequestRequest,
    CreateProjectRequest, CreateTriggerRequest, CreateWorkflowRequest,
    DeleteDesignSpaceBranchRequest, FollowUpTaskRequest, IssueListQuery, IssueTransitionForm,
    LoginForm, LoginQuery, LoginRequest, LoginResponse, MemberRoleForm, MergeRequestCommentForm,
    MergeRequestOperationForm, NewAccountForm, NewInspectionForm, NewIssueForm, NewProjectForm,
    NewTriggerForm, NewWorkflowForm, PageQuery, ProjectUpdateForm, PublishWorkflowForm,
    RenameWorkflowForm, RenameWorkflowRequest, SaveDraftRequest, SelectProjectForm,
    SetMemberIdentityRequest, SetMemberRoleRequest, SetupAdminForm, SetupLoginForm,
    SpawnChildTasksRequest, TaskCommentRequest, TaskListQuery, TaskOperationForm,
    TaskOperationRequest, TransitionIssueRequest, TriageIssueRequest, UpdateAccountRequest,
    UpdateProjectRequest, UpdateTriggerRequest, ValidateWorkflowRequest,
};
use crate::error::workflow_error;
use axum::Form;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use foundry_app::{
    AccountFilter, AccountUseCases, AddTaskLink, AgentAdminUseCases, AuthUseCases, CreateTrigger,
    IssueFilter, IssueUseCases, NewTask, ProjectDetail, ProjectUseCases, PublishWorkflow,
    RemoveProjectMember, RemoveTaskLink, SaveWorkflowLayout, ServiceError, TaskFilter,
    TaskLinkUseCases, TaskUseCases, TaskViewer, UpdateTrigger, WorkflowUseCases, hash_token,
    new_session_token,
};
use foundry_domain::{
    AccountRole, AllowedOperation, MergeRequestStatus, OperationName, Project, ProjectRole, Task,
    TaskLinkKind, TaskLinkRelation, WorkflowId, WorkflowLayout,
};
use foundry_web::MemberInfo;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use time::{Duration, OffsetDateTime};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Process-local state for the first-run setup flow.
///
/// The configured initial account and password are an *authorization identity*,
/// not an account: signing in with them only proves that the caller holds the
/// configuration file. A successful initial login issues a short-lived ticket
/// so the administrator creation step is tied to that browser.
pub struct SetupState {
    required: AtomicBool,
    lock: Mutex<()>,
    /// Normalized initial account handle from the config file.
    bootstrap_handle: String,
    /// Display name prefilled on the new-administrator form.
    pub bootstrap_name: String,
    /// SHA-256 of the initial password from the config file.
    bootstrap_password_hash: Option<[u8; 32]>,
    ticket: StdMutex<Option<SetupTicket>>,
}

struct SetupTicket {
    token_hash: String,
    expires_at: OffsetDateTime,
}

impl SetupState {
    pub fn new(
        required: bool,
        bootstrap_handle: String,
        bootstrap_name: String,
        bootstrap_password: Option<&str>,
    ) -> Self {
        let bootstrap_password_hash = bootstrap_password.map(|value| {
            let digest = Sha256::digest(value.as_bytes());
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&digest);
            hash
        });
        Self {
            required: AtomicBool::new(required),
            lock: Mutex::new(()),
            bootstrap_handle: bootstrap_handle.trim().to_lowercase(),
            bootstrap_name,
            bootstrap_password_hash,
            ticket: StdMutex::new(None),
        }
    }

    pub fn is_required(&self) -> bool {
        self.required.load(Ordering::Relaxed)
    }

    pub fn mark_complete(&self) {
        self.required.store(false, Ordering::Relaxed);
    }

    /// Checks the initial account and password from the config file. This
    /// identity is never created as an account; it only authorizes setup.
    pub fn authorize_initial(&self, handle: &str, password: &str) -> bool {
        // Evaluate both sides so a wrong handle does not short-circuit the
        // password comparison.
        let password_ok = self.check_password(password);
        let handle_ok = handle.trim().to_lowercase() == self.bootstrap_handle;
        handle_ok & password_ok
    }

    fn check_password(&self, candidate: &str) -> bool {
        let Some(expected) = self.bootstrap_password_hash.as_ref() else {
            return false;
        };
        let actual = Sha256::digest(candidate.as_bytes());
        constant_time_eq(actual.as_slice(), expected)
    }

    /// Issues the short-lived ticket that unlocks the administrator creation
    /// step. Only its hash stays in memory.
    pub fn issue_ticket(&self) -> Option<String> {
        self.bootstrap_password_hash.as_ref()?;
        let token = new_session_token();
        let token_hash = hash_token(&token);
        let expires_at =
            OffsetDateTime::now_utc() + Duration::seconds(auth::SETUP_TICKET_TTL_SECONDS);
        let mut ticket = self.ticket.lock().ok()?;
        *ticket = Some(SetupTicket {
            token_hash,
            expires_at,
        });
        Some(token)
    }

    pub fn authorize_ticket(&self, token: &str) -> bool {
        let Ok(mut ticket) = self.ticket.lock() else {
            return false;
        };
        if ticket
            .as_ref()
            .is_some_and(|ticket| ticket.expires_at <= OffsetDateTime::now_utc())
        {
            *ticket = None;
            return false;
        }
        let Some(ticket) = ticket.as_ref() else {
            return false;
        };
        let actual = hash_token(token);
        actual.len() == ticket.token_hash.len()
            && constant_time_eq(actual.as_bytes(), ticket.token_hash.as_bytes())
    }

    pub fn clear_ticket(&self) {
        if let Ok(mut ticket) = self.ticket.lock() {
            *ticket = None;
        }
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= *left ^ *right;
    }
    difference == 0
}

#[derive(Clone)]
pub struct AppState {
    pub issues: Arc<dyn IssueUseCases>,
    pub accounts: Arc<dyn AccountUseCases>,
    pub projects: Arc<dyn ProjectUseCases>,
    pub tasks: Arc<dyn TaskUseCases>,
    pub triggers: Arc<dyn foundry_app::TriggerUseCases>,
    pub agents: Arc<dyn AgentAdminUseCases>,
    pub agent_logs: Arc<foundry_shared::AgentLogFiles>,
    pub workflows: Arc<dyn WorkflowUseCases>,
    pub task_links: Arc<dyn TaskLinkUseCases>,
    pub auth: Arc<dyn AuthUseCases>,
    pub setup: Arc<SetupState>,
    pub editor: Arc<EditorAssets>,
    /// Local design-space console reader.
    pub design_space: Arc<crate::design_space::DesignSpaceAccess>,
    pub merge_requests: Arc<foundry_app::MergeRequestService>,
    /// Deletes a project and its agent/design-space resources.
    pub project_purger: Option<Arc<dyn ProjectPurger>>,
}

#[async_trait::async_trait]
pub trait ProjectPurger: Send + Sync {
    async fn purge(
        &self,
        actor: foundry_domain::Actor,
        project_id: Uuid,
    ) -> Result<(), foundry_app::ServiceError>;
}

/// The built workflow-editor SPA served under `/workflows/editor/`.
#[derive(Debug, Clone)]
pub struct EditorAssets {
    pub dir: std::path::PathBuf,
}

impl EditorAssets {
    pub fn new(dir: impl Into<std::path::PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

/// Answers 503 while the SPA has not been built, so a missing `dist/` never
/// looks like a broken console or a missing route.
pub async fn editor_guard(
    State(state): State<Arc<EditorAssets>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let built = tokio::fs::metadata(&state.dir)
        .await
        .is_ok_and(|metadata| metadata.is_dir());
    if !built {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "workflow editor is not built; run `npm run build` in web/workflow-editor",
        )
            .into_response();
    }
    next.run(request).await
}

pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "foundry",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Vendored Mermaid bundle for the design-space diagram view.
pub async fn mermaid_script() -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/javascript; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        foundry_web::MERMAID_SCRIPT,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// First-run setup
// ---------------------------------------------------------------------------

/// Blocks every route except the setup page while the account table is empty.
pub async fn setup_guard(
    State(state): State<AppState>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let path = request.uri().path();
    if !state.setup.is_required() || path == "/setup" || path == "/setup/admin" || path == "/health"
    {
        return next.run(request).await;
    }
    if path.starts_with("/api/") || path == "/mcp" {
        return ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "setup_required",
            "no accounts exist; complete first-run setup at /setup",
        )
        .into_response();
    }
    Redirect::to("/setup").into_response()
}

/// Shows the initial login form, or the administrator creation form once this
/// browser has completed the initial login.
pub async fn setup_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !state.setup.is_required() {
        return Redirect::to("/login").into_response();
    }
    // The in-memory flag can be stale if accounts were created out of band.
    // Re-read the account table before offering either form.
    match state.accounts.setup_required().await {
        Ok(true) => {
            let authorized = auth::cookie_value(&headers, auth::SETUP_COOKIE)
                .as_deref()
                .is_some_and(|ticket| state.setup.authorize_ticket(ticket));
            if authorized {
                Html(foundry_web::setup_admin_page(
                    "",
                    &state.setup.bootstrap_name,
                    None,
                ))
                .into_response()
            } else {
                Html(foundry_web::setup_login_page("", None)).into_response()
            }
        }
        Ok(false) => {
            state.setup.mark_complete();
            state.setup.clear_ticket();
            Redirect::to("/login").into_response()
        }
        Err(error) => auth::web_internal(error.to_string()),
    }
}

/// Step one: verify the initial account/password from the config file. This
/// does not create an account; success only issues a short-lived setup ticket.
pub async fn setup_login_submit(
    State(state): State<AppState>,
    Form(form): Form<SetupLoginForm>,
) -> Response {
    if !state.setup.is_required() {
        return Redirect::to("/login").into_response();
    }
    match state.accounts.setup_required().await {
        Ok(true) => {}
        Ok(false) => {
            state.setup.mark_complete();
            state.setup.clear_ticket();
            return Redirect::to("/login").into_response();
        }
        Err(error) => return auth::web_internal(error.to_string()),
    }

    let handle = form.handle.trim();
    if !state.setup.authorize_initial(handle, &form.password) {
        tracing::warn!("first-run setup rejected: invalid initial account or password");
        return (
            StatusCode::UNAUTHORIZED,
            Html(foundry_web::setup_login_page(
                handle,
                Some("初始账号或密码不正确。"),
            )),
        )
            .into_response();
    }

    let Some(ticket) = state.setup.issue_ticket() else {
        return auth::web_internal("setup is not configured".to_string());
    };
    tracing::info!("first-run setup authorized with the configured initial identity");
    with_cookie(
        Redirect::to("/setup").into_response(),
        &auth::setup_cookie(&ticket),
    )
}

/// Step two: create the real administrator account. Requires the ticket issued
/// by step one, so only the browser that proved it holds the configuration can
/// create the first account.
pub async fn setup_admin_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SetupAdminForm>,
) -> Response {
    if !state.setup.is_required() {
        return Redirect::to("/login").into_response();
    }
    let Some(ticket) = auth::cookie_value(&headers, auth::SETUP_COOKIE) else {
        return Redirect::to("/setup").into_response();
    };
    if !state.setup.authorize_ticket(&ticket) {
        return Redirect::to("/setup").into_response();
    }

    // One in-flight creation at a time: two concurrent submissions must not
    // both read an empty account table and create two administrators.
    let _guard = state.setup.lock.lock().await;

    if !state.setup.is_required() {
        return Redirect::to("/login").into_response();
    }
    match state.accounts.setup_required().await {
        Ok(true) => {}
        Ok(false) => {
            state.setup.mark_complete();
            state.setup.clear_ticket();
            return Redirect::to("/login").into_response();
        }
        Err(error) => return auth::web_internal(error.to_string()),
    }

    let handle = form.handle.trim();
    let display_name = form.display_name.trim();
    let invalid = |message: &str| {
        (
            StatusCode::BAD_REQUEST,
            Html(foundry_web::setup_admin_page(
                handle,
                display_name,
                Some(message),
            )),
        )
            .into_response()
    };
    if handle.is_empty() {
        return invalid("管理员账号不能为空。");
    }
    if display_name.is_empty() {
        return invalid("显示名称不能为空。");
    }
    if form.password.is_empty() {
        return invalid("密码不能为空。");
    }
    if form.password != form.confirm_password {
        return invalid("两次输入的密码不一致。");
    }

    let admin = match state
        .accounts
        .create_first_admin(handle, display_name, &form.password)
        .await
    {
        Ok(account) => account,
        // The table became non-empty while this request was in flight. Stop
        // advertising setup so the user is not stuck re-submitting the form.
        Err(ServiceError::Forbidden(_)) => {
            state.setup.mark_complete();
            state.setup.clear_ticket();
            return Redirect::to("/login").into_response();
        }
        Err(ServiceError::Validation(message)) => return invalid(&message),
        Err(error) => return auth::web_internal(error.to_string()),
    };

    let login = match state.auth.login(handle, &form.password).await {
        Ok(result) => result,
        Err(error) => return auth::web_internal(error.to_string()),
    };

    tracing::info!(handle = %admin.handle, "first-run setup completed");
    state.setup.mark_complete();
    state.setup.clear_ticket();
    // Setup creates the administrator, not a project. The administrator
    // chooses the first project explicitly on the projects page.
    let mut response = Redirect::to("/projects").into_response();
    response = with_cookie(
        response,
        &auth::session_cookie(&login.token, login.expires_at),
    );
    with_cookie(response, &auth::clear_setup_cookie())
}

/// Adds a `Set-Cookie` header without dropping any already present there.
fn with_cookie(mut response: Response, cookie: &str) -> Response {
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

pub async fn login_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LoginQuery>,
) -> Response {
    if auth::authenticated(&state, &headers)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return Redirect::to(&auth::safe_next(query.next.as_deref().unwrap_or("/")))
            .into_response();
    }
    Html(foundry_web::login_page(
        "",
        &auth::safe_next(query.next.as_deref().unwrap_or("/")),
        None,
    ))
    .into_response()
}

pub async fn login_submit(State(state): State<AppState>, Form(form): Form<LoginForm>) -> Response {
    let next = auth::safe_next(&form.next);
    match state.auth.login(form.handle.trim(), &form.password).await {
        Ok(result) => {
            let cookie = auth::session_cookie(&result.token, result.expires_at);
            let mut response = Redirect::to(&next).into_response();
            if let Ok(value) = HeaderValue::from_str(&cookie) {
                response.headers_mut().insert(header::SET_COOKIE, value);
            }
            response
        }
        Err(ServiceError::Unauthorized(_)) => (
            StatusCode::UNAUTHORIZED,
            Html(foundry_web::login_page(
                &form.handle,
                &next,
                Some("账号或密码不正确，或账户已被停用。"),
            )),
        )
            .into_response(),
        Err(error) => auth::web_internal(error.to_string()),
    }
}

pub async fn logout_submit(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = auth::token_from_headers(&headers) {
        let _ = state.auth.logout(&token).await;
    }
    let mut response = Redirect::to("/login").into_response();
    if let Ok(value) = HeaderValue::from_str(&auth::clear_session_cookie()) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

pub async fn api_login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let result = state.auth.login(body.handle.trim(), &body.password).await?;
    Ok(Json(LoginResponse {
        token: result.token,
        expires_at: result.expires_at,
        account: result.account,
    }))
}

pub async fn api_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    if let Some(token) = auth::token_from_headers(&headers) {
        state.auth.logout(&token).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn api_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<foundry_domain::Account>, ApiError> {
    Ok(Json(auth::api_account(&state, &headers).await?))
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

pub async fn dispatch_agents(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentListQuery>,
) -> Result<Json<Vec<foundry_domain::AgentDefinition>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .agents
            .agents(account.actor(), query.project_id)
            .await?,
    ))
}

pub async fn dispatch_ready_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentListQuery>,
) -> Result<Json<Vec<foundry_domain::ReadyTaskView>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .agents
            .ready_tasks(account.actor(), query.project_id)
            .await?,
    ))
}

pub async fn dispatch_templates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AgentTemplateQuery>,
) -> Result<Json<Vec<foundry_domain::AgentTemplateView>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state.agents.templates(account.actor(), query.role).await?,
    ))
}

pub async fn dispatch_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateAgentFromTemplateRequest>,
) -> Result<Json<foundry_domain::AgentDefinition>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .agents
            .create_from_template(account.actor(), body.into_command())
            .await?,
    ))
}

pub async fn dispatch_assign(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AssignAgentRequest>,
) -> Result<Json<Value>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    state
        .agents
        .assign_task(account.actor(), body.into_command())
        .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn agents_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if session.account.role != AccountRole::Admin {
        return auth::web_forbidden("仅管理员可以管理智能体。");
    }
    agents_response(&state, &session, None).await
}

/// Agents belong to the console's current project.
fn agents_scope(session: &WebAccountSession) -> Option<Uuid> {
    session
        .context
        .current
        .as_ref()
        .and_then(|project| project.id.parse::<Uuid>().ok())
}

async fn visible_agent(
    state: &AppState,
    session: &WebAccountSession,
    id: Uuid,
) -> Result<foundry_domain::AgentDefinition, Response> {
    if session.account.role != AccountRole::Admin {
        return Err(auth::web_forbidden("仅管理员可以查看智能体日志。"));
    }
    match state
        .agents
        .overview(session.account.actor(), agents_scope(session))
        .await
    {
        Ok(overview) => overview
            .definitions
            .into_iter()
            .find(|definition| definition.id == id)
            .ok_or_else(|| auth::web_error(ServiceError::not_found("agent", id))),
        Err(error) => Err(auth::web_error(error)),
    }
}

pub async fn agent_logs_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let definition = match visible_agent(&state, &session, id).await {
        Ok(definition) => definition,
        Err(response) => return response,
    };
    let key = id.to_string();
    let fingerprint = state
        .agent_logs
        .fingerprint(&key)
        .unwrap_or_else(|_| "none".to_string());
    match state.agent_logs.tail(&key, 256 * 1024) {
        Ok(log) => no_store(foundry_web::agent_logs_page(
            &session.context,
            &definition,
            &foundry_shared::strip_agent_log_prefixes(&log),
            &fingerprint,
        )),
        Err(error) => auth::web_internal(format!("无法读取智能体日志：{error}")),
    }
}

pub async fn agent_log_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = visible_agent(&state, &session, id).await {
        return response;
    }
    match state.agent_logs.fingerprint(&id.to_string()) {
        Ok(fingerprint) => no_store_text(fingerprint),
        Err(error) => auth::web_internal(format!("无法读取智能体日志：{error}")),
    }
}

pub async fn agent_log_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = visible_agent(&state, &session, id).await {
        return response;
    }
    match state.agent_logs.tail(&id.to_string(), 256 * 1024) {
        Ok(log) => no_store_text(foundry_shared::strip_agent_log_prefixes(&log)),
        Err(error) => auth::web_internal(format!("无法读取智能体日志：{error}")),
    }
}

pub async fn start_agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    agent_action(state, headers, id, AgentAction::Start).await
}

pub async fn stop_agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    agent_action(state, headers, id, AgentAction::Stop).await
}

pub async fn restart_agent_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    agent_action(state, headers, id, AgentAction::Restart).await
}

enum AgentAction {
    Start,
    Stop,
    Restart,
}

async fn agent_action(
    state: AppState,
    headers: HeaderMap,
    id: Uuid,
    action: AgentAction,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if session.account.role != AccountRole::Admin {
        return auth::web_forbidden("仅管理员可以管理智能体。");
    }
    let actor = session.account.actor();
    match state
        .agents
        .overview(actor.clone(), agents_scope(&session))
        .await
    {
        Ok(overview)
            if overview
                .definitions
                .iter()
                .any(|definition| definition.id == id) => {}
        Ok(_) => return auth::web_error(ServiceError::not_found("agent", id)),
        Err(error) => return auth::web_error(error),
    }
    let result = match action {
        AgentAction::Start => state.agents.start(actor, id).await,
        AgentAction::Stop => state.agents.stop(actor, id).await,
        AgentAction::Restart => state.agents.restart(actor, id).await,
    };
    match result {
        Ok(()) => Redirect::to("/agents").into_response(),
        Err(error) => agents_response(&state, &session, Some(error.to_string())).await,
    }
}

async fn agents_response(
    state: &AppState,
    session: &WebAccountSession,
    error: Option<String>,
) -> Response {
    let project_id = agents_scope(session);
    match state
        .agents
        .overview(session.account.actor(), project_id)
        .await
    {
        Ok(overview) => Html(foundry_web::agents_page(
            &session.context,
            &overview,
            error.as_deref(),
        ))
        .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn accounts_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.account.is_admin() {
        return Redirect::to(&format!("/accounts/{}", session.account.id)).into_response();
    }
    match state
        .accounts
        .list(session.account.actor(), AccountFilter::default())
        .await
    {
        Ok(accounts) => Html(foundry_web::accounts_page(
            &session.context,
            &accounts,
            None,
        ))
        .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn create_account_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NewAccountForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.account.is_admin() {
        return auth::web_forbidden("只有全局管理员可以创建账户。");
    }
    match state
        .accounts
        .create(session.account.actor(), form.into_command())
        .await
    {
        Ok(account) => Redirect::to(&format!("/accounts/{}", account.id)).into_response(),
        Err(error) => accounts_error(&state, &session, error).await,
    }
}

async fn accounts_error(
    state: &AppState,
    session: &WebAccountSession,
    error: ServiceError,
) -> Response {
    let message = error.to_string();
    match state
        .accounts
        .list(session.account.actor(), AccountFilter::default())
        .await
    {
        Ok(accounts) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Html(foundry_web::accounts_page(
                &session.context,
                &accounts,
                Some(&message),
            )),
        )
            .into_response(),
        Err(_) => auth::web_error(error),
    }
}

pub async fn account_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let can_manage = session.account.is_admin();
    if !can_manage && session.account.id != id {
        return auth::web_forbidden("只能查看自己的账户。");
    }
    match state.accounts.get(session.account.actor(), id).await {
        Ok(account) => Html(foundry_web::account_page(
            &session.context,
            &account,
            can_manage,
            None,
        ))
        .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn update_account_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<AccountUpdateForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.account.is_admin() {
        return auth::web_forbidden("只有全局管理员可以修改账户。");
    }
    match state
        .accounts
        .update(session.account.actor(), form.into_command(id))
        .await
    {
        Ok(account) => Redirect::to(&format!("/accounts/{}", account.id)).into_response(),
        Err(error) => {
            let message = error.to_string();
            match state.accounts.get(session.account.actor(), id).await {
                Ok(account) => (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Html(foundry_web::account_page(
                        &session.context,
                        &account,
                        true,
                        Some(&message),
                    )),
                )
                    .into_response(),
                Err(_) => auth::web_error(error),
            }
        }
    }
}

pub async fn list_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(_query): Query<Value>,
) -> Result<Json<Vec<foundry_domain::Account>>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let accounts = state
        .accounts
        .list(session.actor(), AccountFilter::default())
        .await?;
    Ok(Json(accounts))
}

pub async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateAccountRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let account = state
        .accounts
        .create(session.actor(), body.into_command())
        .await?;
    Ok((StatusCode::CREATED, Json(account)))
}

pub async fn get_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<foundry_domain::Account>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(state.accounts.get(session.actor(), id).await?))
}

pub async fn update_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateAccountRequest>,
) -> Result<Json<foundry_domain::Account>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .accounts
            .update(session.actor(), body.into_command(id))
            .await?,
    ))
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

pub async fn projects_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let root_workflows = match state
        .projects
        .root_workflow_candidates(session.account.actor())
        .await
    {
        Ok(workflows) => workflows,
        Err(error) => return auth::web_error(error),
    };
    Html(foundry_web::projects_page(
        &session.context,
        &session.context.projects,
        &root_workflows,
        None,
    ))
    .into_response()
}

pub async fn create_project_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NewProjectForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state
        .projects
        .create(session.account.actor(), form.into_command())
        .await
    {
        Ok(project) => {
            ensure_default_triggers(&state, project.id).await;
            state.agents.ensure_project_dispatcher(project.id).await;
            Redirect::to(&format!("/projects/{}", project.id)).into_response()
        }
        Err(error) => project_form_error(&state, &session, error).await,
    }
}

async fn project_form_error(
    state: &AppState,
    session: &WebAccountSession,
    error: ServiceError,
) -> Response {
    let message = error.to_string();
    match state.projects.list_visible(session.account.actor()).await {
        Ok(projects) => {
            let root_workflows = state
                .projects
                .root_workflow_candidates(session.account.actor())
                .await
                .unwrap_or_default();
            let context = foundry_web::ConsoleContext::new(&session.account, &projects, None);
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(foundry_web::projects_page(
                    &context,
                    &context.projects,
                    &root_workflows,
                    Some(&message),
                )),
            )
                .into_response()
        }
        Err(_) => auth::web_error(error),
    }
}

pub async fn select_project_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SelectProjectForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session
        .projects
        .iter()
        .any(|project| project.id.to_string() == form.project.trim())
    {
        return Redirect::to("/projects").into_response();
    }
    let mut response = Redirect::to("/").into_response();
    if let Ok(value) = HeaderValue::from_str(&auth::project_cookie(&form.project)) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn project_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state.projects.detail(session.account.actor(), id).await {
        Ok(detail) => {
            let members = member_infos(&detail);
            let triggers = state
                .triggers
                .list(session.account.actor(), id)
                .await
                .unwrap_or_default();
            Html(foundry_web::project_page(
                &session.context,
                &detail.project,
                &members,
                &triggers,
                account_can_manage(&session.account, &detail.project),
                None,
            ))
            .into_response()
        }
        Err(error) => auth::web_error(error),
    }
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct DesignSpaceQuery {
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct DesignSpaceSectionQuery {
    #[serde(default)]
    pub branch: Option<String>,
    pub qname: String,
}

fn requested_design_space_branch(project: &Project, query: &DesignSpaceQuery) -> String {
    query
        .branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
        .unwrap_or(project.design_space_branch.as_str())
        .to_string()
}

pub async fn design_space_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<DesignSpaceQuery>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let access = &state.design_space;
    let branches = access.branches(&project).await.unwrap_or_default();
    let mut branch = requested_design_space_branch(&project, &query);
    if !branches.iter().any(|candidate| candidate == &branch) {
        branch = project.design_space_branch.clone();
    }
    match access.overview(&project, &branch).await {
        Ok(overview) => Html(foundry_web::design_space_page(
            &session.context,
            &project,
            &overview,
            &branches,
            &branch,
        ))
        .into_response(),
        Err(error) => Html(foundry_web::design_space_unavailable_page(
            &session.context,
            &project,
            &error,
        ))
        .into_response(),
    }
}

pub async fn design_space_tree(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<DesignSpaceQuery>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "项目不存在" })),
        )
            .into_response();
    };
    let access = &state.design_space;
    let branch = requested_design_space_branch(&project, &query);
    match access.overview(&project, &branch).await {
        Ok(overview) => Json(overview).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn design_space_element(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, hash)): Path<(Uuid, String)>,
    Query(query): Query<DesignSpaceQuery>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let access = &state.design_space;
    let branch = requested_design_space_branch(&project, &query);
    let source_url = format!("/projects/{id}/design-space/elements/{hash}/source?branch={branch}");
    match access.element(&project, &hash, &branch).await {
        Ok(detail) => Html(foundry_web::design_space_element(&detail, &source_url)).into_response(),
        Err(error) => Html(foundry_web::design_space_element_error(&error)).into_response(),
    }
}

pub async fn design_space_element_section(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, hash)): Path<(Uuid, String)>,
    Query(query): Query<DesignSpaceSectionQuery>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let qname = query.qname.trim();
    if qname.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Html(foundry_web::design_space_section_error("缺少章节 qname")),
        )
            .into_response();
    }
    let branch = requested_design_space_branch(
        &project,
        &DesignSpaceQuery {
            branch: query.branch,
        },
    );
    match state
        .design_space
        .section(&project, &hash, &branch, qname)
        .await
    {
        Ok(markdown) => Html(foundry_web::design_space_section(&markdown)).into_response(),
        Err(error) => Html(foundry_web::design_space_section_error(&error)).into_response(),
    }
}

pub async fn design_space_element_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, hash)): Path<(Uuid, String)>,
    Query(query): Query<DesignSpaceQuery>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let access = &state.design_space;
    let branch = requested_design_space_branch(&project, &query);
    match access.source(&project, &hash, &branch).await {
        Ok(source) => Html(foundry_web::design_space_source(&source)).into_response(),
        Err(error) => Html(foundry_web::design_space_source_error(&error)).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Design-space merge requests
// ---------------------------------------------------------------------------

fn merge_request_viewer(account: &foundry_domain::Account) -> TaskViewer {
    if account.is_admin() {
        TaskViewer::manager(account.id)
    } else {
        TaskViewer::member(account.id)
    }
}

fn can_contribute_to_project(account: &foundry_domain::Account, project: &Project) -> bool {
    account.is_admin()
        || project
            .role_of(account.id)
            .is_some_and(ProjectRole::can_contribute)
}

pub async fn merge_requests_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let viewer = merge_request_viewer(&session.account);
    let requests = match state.merge_requests.list(&project, viewer).await {
        Ok(requests) => requests,
        Err(error) => return auth::web_error(error),
    };
    let branches = state
        .merge_requests
        .branches(&project)
        .await
        .unwrap_or_default();
    Html(foundry_web::merge_requests_page(
        &session.context,
        &project,
        &requests,
        &branches,
        None,
    ))
    .into_response()
}

pub async fn create_merge_request_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<CreateMergeRequestForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    if !can_contribute_to_project(&session.account, &project) {
        return auth::web_forbidden("当前项目角色不能创建合并请求。");
    }
    match state
        .merge_requests
        .create(&project, session.account.id, form.into_command())
        .await
    {
        Ok(view) => Redirect::to(&format!(
            "/projects/{}/merge-requests/{}",
            project.id, view.merge_request.id
        ))
        .into_response(),
        Err(ServiceError::Validation(message) | ServiceError::Forbidden(message)) => {
            let viewer = merge_request_viewer(&session.account);
            let requests = state
                .merge_requests
                .list(&project, viewer)
                .await
                .unwrap_or_default();
            let branches = state
                .merge_requests
                .branches(&project)
                .await
                .unwrap_or_default();
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(foundry_web::merge_requests_page(
                    &session.context,
                    &project,
                    &requests,
                    &branches,
                    Some(&message),
                )),
            )
                .into_response()
        }
        Err(error) => auth::web_error(error),
    }
}

pub async fn merge_request_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    match state
        .merge_requests
        .review(
            &project,
            merge_request_id,
            merge_request_viewer(&session.account),
        )
        .await
    {
        Ok(review) => Html(foundry_web::merge_request_page(
            &session.context,
            &project,
            &review,
        ))
        .into_response(),
        Err(ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. }) => (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn merge_request_changes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    match state
        .merge_requests
        .get(
            &project,
            merge_request_id,
            merge_request_viewer(&session.account),
        )
        .await
    {
        Ok(detail) => Html(foundry_web::merge_request_changes(&detail)).into_response(),
        Err(ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. }) => (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn merge_request_comment_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, merge_request_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<MergeRequestCommentForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    if !can_contribute_to_project(&session.account, &project) {
        return auth::web_forbidden("当前项目角色不能评论合并请求。");
    }
    let request = match state
        .merge_requests
        .load(project.id, merge_request_id)
        .await
    {
        Ok(request) => request,
        Err(error) => return auth::web_error(error),
    };
    match state
        .tasks
        .add_comment(
            project.id,
            request.task_id,
            session.account.id,
            form.body,
            merge_request_viewer(&session.account),
        )
        .await
    {
        Ok(_) => Redirect::to(&format!(
            "/projects/{}/merge-requests/{merge_request_id}",
            project.id
        ))
        .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn approve_merge_request_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    if let Err(error) = state
        .merge_requests
        .approve(&project, merge_request_id, session.account.id)
        .await
    {
        return auth::web_error(error);
    }
    Redirect::to(&format!(
        "/projects/{}/merge-requests/{merge_request_id}",
        project.id
    ))
    .into_response()
}

pub async fn merge_request_operation_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, merge_request_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<MergeRequestOperationForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let operation = form.operation.trim();
    let operation_name = operation
        .rsplit_once(':')
        .map(|(_, name)| name)
        .unwrap_or(operation);
    if operation_name == "approved" || operation_name == "review_approved" {
        return auth::web_forbidden("请使用合并请求页面的批准按钮，以便记录审批的设计内容摘要。");
    }
    if operation_name == "merge" {
        match state
            .merge_requests
            .merge(&project, merge_request_id, session.account.id)
            .await
        {
            Ok(_) => {}
            Err(error) => return auth::web_error(error),
        }
    } else {
        let request = match state
            .merge_requests
            .load(project.id, merge_request_id)
            .await
        {
            Ok(request) => request,
            Err(error) => return auth::web_error(error),
        };
        let idempotency = form
            .idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("design-space-mr:{merge_request_id}:{operation}"));
        if let Err(error) = state
            .tasks
            .send_operation(
                project.id,
                request.task_id,
                session.account.id,
                OperationName::new(operation),
                idempotency,
            )
            .await
        {
            return auth::web_error(error);
        }
    }
    Redirect::to(&format!(
        "/projects/{}/merge-requests/{merge_request_id}",
        project.id
    ))
    .into_response()
}

pub async fn create_merge_request_for_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
    Json(body): Json<CreateMergeRequestRequest>,
) -> Result<(StatusCode, Json<foundry_domain::MergeRequestReview>), ApiError> {
    let session = task_api_session(&state, &headers, task_id).await?;
    let view = state
        .merge_requests
        .create_for_task(
            &session.project,
            task_id,
            session.account.id,
            body.into_command(),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn get_merge_request_for_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
) -> Result<Json<foundry_domain::MergeRequestReview>, ApiError> {
    let session = task_api_session(&state, &headers, task_id).await?;
    let review = state
        .merge_requests
        .for_task_review(
            &session.project,
            task_id,
            merge_request_viewer(&session.account),
        )
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "this task has no merge request",
            )
        })?;
    Ok(Json(review))
}

pub async fn delete_design_space_branch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<DeleteDesignSpaceBranchRequest>,
) -> Result<StatusCode, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    if !can_contribute_to_project(&account, &project) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "current project role cannot delete design-space branches",
        ));
    }
    state
        .merge_requests
        .delete_branch(&project, body.branch.trim())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_merge_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<foundry_domain::MergeRequestReview>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    Ok(Json(
        state
            .merge_requests
            .list(&project, merge_request_viewer(&account))
            .await?,
    ))
}

pub async fn create_merge_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateMergeRequestRequest>,
) -> Result<(StatusCode, Json<foundry_domain::MergeRequestReview>), ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    if !can_contribute_to_project(&account, &project) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "current project role cannot create merge requests",
        ));
    }
    let view = state
        .merge_requests
        .create(&project, account.id, body.into_command())
        .await?;
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn get_merge_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<foundry_domain::MergeRequestDetail>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    Ok(Json(
        state
            .merge_requests
            .get(&project, merge_request_id, merge_request_viewer(&account))
            .await?,
    ))
}

pub async fn approve_merge_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<foundry_domain::MergeRequestReview>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    Ok(Json(
        state
            .merge_requests
            .approve(&project, merge_request_id, account.id)
            .await?,
    ))
}

pub async fn merge_merge_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, merge_request_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<foundry_domain::MergeRequestReview>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let project = state.projects.get(account.actor(), project_id).await?;
    Ok(Json(
        state
            .merge_requests
            .merge(&project, merge_request_id, account.id)
            .await?,
    ))
}

fn session_project(session: &WebAccountSession, id: Uuid) -> Option<Project> {
    session
        .projects
        .iter()
        .find(|project| project.id == id)
        .cloned()
}

/// Resolve a task's project from the task itself. Tasks never move between
/// projects, so task endpoints do not take a project header.
async fn task_api_session(
    state: &AppState,
    headers: &HeaderMap,
    task_id: Uuid,
) -> Result<ApiSession, ApiError> {
    let account = auth::api_account(state, headers).await?;
    let project_id = state.tasks.project_id(task_id).await?;
    let projects = state.projects.list_visible(account.actor()).await?;
    let project = projects
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "not_found", "task not found"))?;
    let role = project.role_of(account.id);
    Ok(ApiSession {
        account,
        project,
        role,
    })
}

pub async fn update_project_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<ProjectUpdateForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match can_manage_project(&state, &session.account, id).await {
        Ok(true) => {}
        Ok(false) => {
            return auth::web_forbidden("只有项目 Owner 或全局管理员可以修改项目。");
        }
        Err(error) => return auth::web_error(error),
    }
    let reevaluate = form.skip_human_review;
    match state
        .projects
        .update(session.account.actor(), form.into_command(id))
        .await
    {
        Ok(_) => {
            if reevaluate && let Err(error) = state.tasks.reevaluate_project(id).await {
                tracing::warn!(project_id = %id, %error, "cannot re-evaluate project tasks");
            }
            Redirect::to(&format!("/projects/{id}")).into_response()
        }
        Err(error) => project_detail_error(&state, &session, id, error).await,
    }
}

async fn project_detail_error(
    state: &AppState,
    session: &WebSession,
    id: Uuid,
    error: ServiceError,
) -> Response {
    let message = error.to_string();
    match state.projects.detail(session.account.actor(), id).await {
        Ok(detail) => {
            let members = member_infos(&detail);
            let triggers = state
                .triggers
                .list(session.account.actor(), id)
                .await
                .unwrap_or_default();
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(foundry_web::project_page(
                    &session.context,
                    &detail.project,
                    &members,
                    &triggers,
                    account_can_manage(&session.account, &detail.project),
                    Some(&message),
                )),
            )
                .into_response()
        }
        Err(_) => auth::web_error(error),
    }
}

pub async fn delete_project_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = require_project_manager(
        &state,
        &session,
        id,
        "只有项目 Owner 或全局管理员可以删除项目。",
    )
    .await
    {
        return response;
    }
    let Some(purger) = state.project_purger.as_ref() else {
        return project_detail_error(
            &state,
            &session,
            id,
            foundry_app::ServiceError::Internal("项目清理服务不可用".to_string()),
        )
        .await;
    };
    match purger.purge(session.account.actor(), id).await {
        Ok(()) => {
            let mut response = Redirect::to("/projects").into_response();
            if session.project.id == id
                && let Ok(value) = HeaderValue::from_str(&auth::clear_project_cookie())
            {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
            response
        }
        Err(error) => project_detail_error(&state, &session, id, error).await,
    }
}

pub async fn add_member_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<AddMemberForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = require_project_manager(
        &state,
        &session,
        id,
        "只有项目 Owner 或全局管理员可以添加成员。",
    )
    .await
    {
        return response;
    }
    match state
        .projects
        .add_member_by_handle(session.account.actor(), form.into_command(id))
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}")).into_response(),
        Err(error) => project_detail_error(&state, &session, id, error).await,
    }
}

pub async fn set_member_role_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, account_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<MemberRoleForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = require_project_manager(
        &state,
        &session,
        id,
        "只有项目 Owner 或全局管理员可以修改成员角色。",
    )
    .await
    {
        return response;
    }
    match state
        .projects
        .set_member_role(session.account.actor(), form.into_command(id, account_id))
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}")).into_response(),
        Err(error) => project_detail_error(&state, &session, id, error).await,
    }
}

pub async fn remove_member_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, account_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if let Err(response) = require_project_manager(
        &state,
        &session,
        id,
        "只有项目 Owner 或全局管理员可以移除成员。",
    )
    .await
    {
        return response;
    }
    match state
        .projects
        .remove_member(
            session.account.actor(),
            RemoveProjectMember {
                project_id: id,
                account_id,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}")).into_response(),
        Err(error) => project_detail_error(&state, &session, id, error).await,
    }
}

pub async fn list_projects(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Project>>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(state.projects.list_visible(session.actor()).await?))
}

pub async fn create_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateProjectRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let project = state
        .projects
        .create(session.actor(), body.into_command())
        .await?;
    ensure_default_triggers(&state, project.id).await;
    state.agents.ensure_project_dispatcher(project.id).await;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn ensure_default_triggers(state: &AppState, project_id: Uuid) {
    if let Err(error) = state.triggers.ensure_daily_check(project_id).await {
        tracing::warn!(project_id = %project_id, %error, "cannot create default daily-check trigger");
    }
}

pub async fn get_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<ProjectDetail>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(state.projects.detail(session.actor(), id).await?))
}

pub async fn update_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<Project>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let reevaluate = body.skip_human_review == Some(true);
    let project = state
        .projects
        .update(session.actor(), body.into_command(id))
        .await?;
    if reevaluate && let Err(error) = state.tasks.reevaluate_project(id).await {
        tracing::warn!(project_id = %id, %error, "cannot re-evaluate project tasks");
    }
    Ok(Json(project))
}

pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let Some(purger) = state.project_purger.as_ref() else {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "project_purge_unavailable",
            "项目清理服务不可用",
        ));
    };
    purger.purge(session.actor(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn add_project_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<AddMemberRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    let project = state
        .projects
        .add_member_by_handle(session.actor(), body.into_command(id))
        .await?;
    Ok((StatusCode::CREATED, Json(project)))
}

pub async fn set_project_member_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, account_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetMemberRoleRequest>,
) -> Result<Json<Project>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .projects
            .set_member_role(session.actor(), body.into_command(id, account_id))
            .await?,
    ))
}

pub async fn remove_project_member(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, account_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    state
        .projects
        .remove_member(
            session.actor(),
            RemoveProjectMember {
                project_id: id,
                account_id,
            },
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_project_member_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, identity)): Path<(Uuid, String)>,
    Json(body): Json<SetMemberIdentityRequest>,
) -> Result<Json<Project>, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .projects
            .set_member_identity(session.actor(), body.into_command(id, identity))
            .await?,
    ))
}

async fn can_manage_project(
    state: &AppState,
    account: &foundry_domain::Account,
    project_id: Uuid,
) -> Result<bool, ServiceError> {
    if account.is_admin() {
        return Ok(true);
    }
    let project = state.projects.get(account.actor(), project_id).await?;
    Ok(project
        .role_of(account.id)
        .is_some_and(foundry_domain::ProjectRole::can_manage_members))
}

async fn require_project_manager(
    state: &AppState,
    session: &WebSession,
    project_id: Uuid,
    message: &str,
) -> Result<(), Response> {
    match can_manage_project(state, &session.account, project_id).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(auth::web_forbidden(message)),
        Err(error) => Err(auth::web_error(error)),
    }
}

fn account_can_manage(account: &foundry_domain::Account, project: &Project) -> bool {
    account.is_admin()
        || project
            .role_of(account.id)
            .is_some_and(foundry_domain::ProjectRole::can_manage_members)
}

fn member_infos(detail: &ProjectDetail) -> Vec<MemberInfo> {
    detail
        .members
        .iter()
        .map(|member| MemberInfo {
            account_id: member.account_id.to_string(),
            handle: member.handle.clone(),
            display_name: member.display_name.clone(),
            kind: member.kind,
            status: member.status,
            role: member.role,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Issues
// ---------------------------------------------------------------------------

pub async fn list_triggers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<foundry_domain::Trigger>>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    Ok(Json(
        state
            .triggers
            .list(session.account.actor(), project_id)
            .await?,
    ))
}

pub async fn create_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateTriggerRequest>,
) -> Result<(StatusCode, Json<foundry_domain::Trigger>), ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let title = body.title_template.unwrap_or_else(|| body.name.clone());
    let trigger = state
        .triggers
        .create(
            session.account.actor(),
            CreateTrigger {
                project_id,
                name: body.name,
                description: body.description,
                workflow_id: body.workflow_id,
                input: body.input,
                schedule: body.schedule,
                title_template: title,
                description_template: body.description_template.unwrap_or_default(),
                created_by_id: session.account.id,
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(trigger)))
}

pub async fn update_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<UpdateTriggerRequest>,
) -> Result<Json<foundry_domain::Trigger>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    Ok(Json(
        state
            .triggers
            .update(
                session.account.actor(),
                UpdateTrigger {
                    project_id,
                    id: trigger_id,
                    name: body.name,
                    description: body.description,
                    input: body.input,
                    schedule: body.schedule,
                    title_template: body.title_template,
                    description_template: body.description_template,
                    enabled: body.enabled,
                },
            )
            .await?,
    ))
}

pub async fn delete_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    state
        .triggers
        .delete(session.account.actor(), project_id, trigger_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn run_trigger(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Result<(StatusCode, Json<foundry_domain::TriggerRun>), ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let run = state
        .triggers
        .run(session.account.actor(), project_id, trigger_id)
        .await?;
    Ok((StatusCode::CREATED, Json(run)))
}

pub async fn list_trigger_runs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<foundry_domain::TriggerRun>>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    Ok(Json(
        state
            .triggers
            .runs(session.account.actor(), project_id, trigger_id)
            .await?,
    ))
}

pub async fn create_trigger_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Form(form): Form<NewTriggerForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let project = match state
        .projects
        .get(session.account.actor(), project_id)
        .await
    {
        Ok(project) => project,
        Err(error) => return auth::web_error(error),
    };
    if !can_contribute_to_project(&session.account, &project) {
        return auth::web_forbidden("当前项目角色不能创建触发器。");
    }
    let input = if form.scope.trim().is_empty() {
        json!({})
    } else {
        json!({ "scope": form.scope.trim() })
    };
    match state
        .triggers
        .create(
            session.account.actor(),
            CreateTrigger {
                project_id,
                name: form.name.clone(),
                description: String::new(),
                workflow_id: form.workflow_id,
                input,
                schedule: form.schedule,
                title_template: form.name,
                description_template: "由触发器 {trigger} 启动。".to_string(),
                created_by_id: session.account.id,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{project_id}")).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn run_trigger_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state
        .triggers
        .run(session.account.actor(), project_id, trigger_id)
        .await
    {
        Ok(run) => Redirect::to(&format!("/tasks/{}", run.task_id)).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn toggle_trigger_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let trigger = match state
        .triggers
        .get(session.account.actor(), project_id, trigger_id)
        .await
    {
        Ok(Some(trigger)) => trigger,
        Ok(None) => return auth::web_error(ServiceError::not_found("trigger", trigger_id)),
        Err(error) => return auth::web_error(error),
    };
    match state
        .triggers
        .update(
            session.account.actor(),
            UpdateTrigger {
                project_id,
                id: trigger_id,
                name: None,
                description: None,
                input: None,
                schedule: None,
                title_template: None,
                description_template: None,
                enabled: Some(!trigger.enabled),
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{project_id}")).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn delete_trigger_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state
        .triggers
        .delete(session.account.actor(), project_id, trigger_id)
        .await
    {
        Ok(()) => Redirect::to(&format!("/projects/{project_id}")).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn create_inspection_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Form(form): Form<NewInspectionForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let project = match state
        .projects
        .get(session.account.actor(), project_id)
        .await
    {
        Ok(project) => project,
        Err(error) => return auth::web_error(error),
    };
    if !can_contribute_to_project(&session.account, &project) {
        return auth::web_forbidden("当前项目角色不能启动检查。");
    }
    let (workflow, title) = match form.kind.trim() {
        "style" => ("style_inspection", "代码风格检查"),
        "logic" => ("logic_inspection", "代码逻辑检查"),
        "cohesion" => ("cohesion_inspection", "逻辑聚合度检查"),
        "consistency" => ("consistency_inspection", "代码-设计一致性检查"),
        "design" => ("design_inspection", "设计检查"),
        _ => return auth::web_forbidden("未知的检查类型。"),
    };
    let scope = form.scope.trim().to_string();
    let description = if scope.is_empty() {
        format!("对项目执行{title}，检查现有代码/设计并把问题上报为 Issue。")
    } else {
        format!("对范围 {scope} 执行{title}，把问题上报为 Issue。")
    };
    match state
        .tasks
        .create_task(
            project_id,
            NewTask {
                parent_id: match project.root_task_id {
                    Some(parent_id) => parent_id,
                    None => return auth::web_forbidden("项目没有根任务。"),
                },
                workflow: workflow.to_string(),
                title: title.to_string(),
                description,
                input: json!({ "scope": scope }),
            },
            session.account.id,
        )
        .await
    {
        Ok(task) => Redirect::to(&format!("/tasks/{}", task.id)).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn create_inspection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<CreateInspectionRequest>,
) -> Result<(StatusCode, Json<Task>), ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let project = state
        .projects
        .get(session.account.actor(), project_id)
        .await?;
    if !can_contribute_to_project(&session.account, &project) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "current project role cannot start inspections",
        ));
    }
    let (workflow, title) = match body.kind.trim() {
        "style" => ("style_inspection", "代码风格检查"),
        "logic" => ("logic_inspection", "代码逻辑检查"),
        "cohesion" => ("cohesion_inspection", "逻辑聚合度检查"),
        "consistency" => ("consistency_inspection", "代码-设计一致性检查"),
        "design" => ("design_inspection", "设计检查"),
        other => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "unknown_inspection",
                format!("unknown inspection kind: {other}"),
            ));
        }
    };
    let scope = body.scope.unwrap_or_default();
    let description = if scope.trim().is_empty() {
        format!("对项目执行{title}，检查现有代码/设计并把问题上报为 Issue。")
    } else {
        format!("对范围 {scope} 执行{title}，把问题上报为 Issue。")
    };
    let task = state
        .tasks
        .create_task(
            project_id,
            NewTask {
                parent_id: project.root_task_id.ok_or_else(|| {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "no_root_task",
                        "project has no root task",
                    )
                })?,
                workflow: workflow.to_string(),
                title: title.to_string(),
                description,
                input: json!({ "scope": scope }),
            },
            session.account.id,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn create_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateIssueRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    require_contribute(&session)?;
    let issue = state
        .issues
        .create(session.actor(), body.into_command(session.project.id))
        .await?;
    Ok((StatusCode::CREATED, Json(issue)))
}

pub async fn issue_triage_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let issues = state
        .issues
        .list(
            session.project.id,
            IssueFilter {
                status: None,
                kind: None,
                source: None,
                assignee: None,
                text: None,
            },
        )
        .await?;
    let pending = issues
        .iter()
        .filter(|issue| issue.status == foundry_domain::IssueStatus::Open && issue.triage.is_none())
        .cloned()
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "pending": pending,
        "open_issues": issues,
    })))
}

pub async fn get_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<foundry_domain::Issue>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    Ok(Json(state.issues.get(session.project.id, id).await?))
}

pub async fn list_issues(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<IssueListQuery>,
) -> Result<Json<Vec<foundry_domain::Issue>>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let issues = state
        .issues
        .list(
            session.project.id,
            IssueFilter {
                status: query.status,
                kind: query.kind,
                source: query.source,
                assignee: query.assignee,
                text: query.text,
            },
        )
        .await?;
    Ok(Json(issues))
}

pub async fn comment_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<CommentIssueRequest>,
) -> Result<Json<foundry_domain::Issue>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    require_contribute(&session)?;
    Ok(Json(
        state
            .issues
            .comment(body.into_command(session.project.id, id, session.actor()))
            .await?,
    ))
}

pub async fn transition_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<TransitionIssueRequest>,
) -> Result<Json<foundry_domain::Issue>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let agent_may_advance = session.account.kind == foundry_domain::AccountKind::Agent
        && session.project.member(session.account.id).is_some();
    if !session.account.is_admin() && !session.can_dispose() && !agent_may_advance {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only a project maintainer, administrator or project agent can transition issues",
        ));
    }
    Ok(Json(
        state
            .issues
            .transition(body.into_command(session.project.id, id, session.actor()))
            .await?,
    ))
}

pub async fn triage_issue(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<TriageIssueRequest>,
) -> Result<Json<foundry_domain::Issue>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    require_contribute(&session)?;
    Ok(Json(
        state
            .issues
            .set_triage(session.actor(), session.project.id, id, body.into_triage())
            .await?,
    ))
}

pub async fn new_issue_form(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_contribute() {
        return auth::web_forbidden("当前项目角色不允许创建议题。");
    }
    Html(foundry_web::new_issue_page(
        &session.context,
        &foundry_web::IssueFormValues::default(),
        None,
    ))
    .into_response()
}

pub async fn create_issue_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NewIssueForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_contribute() {
        return auth::web_forbidden("当前项目角色不允许创建议题。");
    }
    let values = form.values();
    match state
        .issues
        .create(
            session.account.actor(),
            form.into_command(session.project.id),
        )
        .await
    {
        Ok(issue) => Redirect::to(&format!("/issues/{}", issue.id)).into_response(),
        Err(ServiceError::Validation(message)) | Err(ServiceError::Forbidden(message)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Html(foundry_web::new_issue_page(
                &session.context,
                &values,
                Some(&message),
            )),
        )
            .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn issue_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state.issues.get(session.project.id, id).await {
        Ok(issue) => {
            let linked = state
                .task_links
                .linked_issue_tasks(session.account.actor(), session.project.id, id)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|task| foundry_web::IssueLinkedTask {
                    id: task.id.to_string(),
                    title: task.title,
                    workflow: task.workflow,
                    stage: task.stage.unwrap_or_default(),
                    state: task.state,
                    result: task.result.unwrap_or_default(),
                    active: task.active,
                })
                .collect::<Vec<_>>();
            Html(foundry_web::issue_page(&session.context, &issue, &linked)).into_response()
        }
        Err(ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. }) => (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn issue_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state.issues.get(session.project.id, id).await {
        Ok(issue) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            format!("{}:{}", issue.version, issue.comments.len()),
        )
            .into_response(),
        Err(ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. }) => {
            (StatusCode::NOT_FOUND, "not found").into_response()
        }
        Err(error) => auth::web_error(error),
    }
}

pub async fn comment_issue_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<CommentIssueForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_contribute() {
        return auth::web_forbidden("当前项目角色不允许评论议题。");
    }
    match state
        .issues
        .comment(form.into_command(session.project.id, id, session.actor()))
        .await
    {
        Ok(_) => Redirect::to(&format!("/issues/{id}")).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn transition_issue_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<IssueTransitionForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_dispose() {
        return auth::web_forbidden("当前项目角色不允许处置议题。");
    }
    let command = match form.into_command(session.project.id, id, session.actor()) {
        Ok(command) => command,
        Err(message) => return (StatusCode::UNPROCESSABLE_ENTITY, message).into_response(),
    };
    match state.issues.transition(command).await {
        Ok(_) => Redirect::to(&format!("/issues/{id}")).into_response(),
        Err(error) => auth::web_error(error),
    }
}

// ---------------------------------------------------------------------------
// Console pages
// ---------------------------------------------------------------------------

pub async fn index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let issues = match state
        .issues
        .list(session.project.id, IssueFilter::default())
        .await
    {
        Ok(issues) => issues,
        Err(error) => return auth::web_error(error),
    };
    let tasks = match state
        .tasks
        .list(
            session.project.id,
            TaskFilter::default(),
            web_task_viewer(&session),
        )
        .await
    {
        Ok(tasks) => tasks,
        Err(error) => return auth::web_error(error),
    };
    Html(foundry_web::workbench(&session.context, &issues, &tasks)).into_response()
}

pub async fn issue_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state
        .issues
        .list(session.project.id, IssueFilter::default())
        .await
    {
        Ok(issues) => Html(foundry_web::issue_dashboard(&session.context, &issues)).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn tasks_index(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TaskListQuery>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let filter = match query.into_filter() {
        Ok(filter) => filter,
        Err(message) => return auth::web_error(ServiceError::Validation(message)),
    };
    let selected_state = filter.state;
    let tasks = match state
        .tasks
        .list(session.project.id, filter, web_task_viewer(&session))
        .await
    {
        Ok(tasks) => tasks,
        Err(error) => return auth::web_error(error),
    };
    Html(foundry_web::tasks_page(
        &session.context,
        &tasks,
        selected_state,
    ))
    .into_response()
}

pub async fn task_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let task = match state
        .tasks
        .get(session.project.id, id, web_task_viewer(&session))
        .await
    {
        Ok(task) => task,
        Err(ServiceError::NotFound { .. } | ServiceError::NotFoundNamed { .. }) => {
            return (
                StatusCode::NOT_FOUND,
                Html(foundry_web::not_found_page(&session.context)),
            )
                .into_response();
        }
        Err(error) => return auth::web_error(error),
    };
    let links = state
        .task_links
        .list(session.account.actor(), id)
        .await
        .unwrap_or_default();
    Html(foundry_web::task_page(&session.context, &task, &links)).into_response()
}

pub async fn send_task_operation_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<TaskOperationForm>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_contribute() {
        return auth::web_forbidden("当前项目角色不能执行任务操作。");
    }
    let task = match state
        .tasks
        .get(session.project.id, id, web_task_viewer(&session))
        .await
    {
        Ok(task) => task,
        Err(error) => return auth::web_error(error),
    };
    if task.project_id != session.project.id {
        return auth::web_error(ServiceError::not_found("task", id));
    }
    let operation = form.operation.trim();
    let operation_name = operation
        .rsplit_once(':')
        .map(|(_, name)| name)
        .unwrap_or(operation);
    if design_space_approval_operation(&task, operation_name) {
        if !state
            .merge_requests
            .has_merge_request(session.project.id, id)
            .await
            .unwrap_or(false)
        {
            return auth::web_forbidden("设计空间修改必须先创建合并请求，才能审批。");
        }
        return auth::web_forbidden(
            "这个任务有关联的合并请求，请从合并请求页面批准并记录设计内容摘要。",
        );
    }
    if (operation_name == "approved" || operation_name == "review_approved")
        && state
            .merge_requests
            .has_merge_request(session.project.id, id)
            .await
            .unwrap_or(false)
    {
        return auth::web_forbidden(
            "这个任务有关联的合并请求，请从合并请求页面批准并记录设计内容摘要。",
        );
    }
    let idempotency = form
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "task-operation:{id}:{operation}:{}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            )
        });
    if let Err(error) = state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(operation),
            idempotency,
        )
        .await
    {
        return auth::web_error(error);
    }
    Redirect::to(&format!("/tasks/{id}")).into_response()
}

pub async fn retry_task_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    task_recovery_form(state, headers, id, foundry_domain::RETRY_OPERATION).await
}

pub async fn abandon_task_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    task_recovery_form(state, headers, id, foundry_domain::ABANDON_OPERATION).await
}

async fn task_recovery_form(
    state: AppState,
    headers: HeaderMap,
    id: Uuid,
    operation: &str,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if !session.can_dispose() {
        return auth::web_forbidden("当前项目角色不能重试或放弃任务。");
    }
    let task = match state
        .tasks
        .get(session.project.id, id, web_task_viewer(&session))
        .await
    {
        Ok(task) => task,
        Err(error) => return auth::web_error(error),
    };
    if task.project_id != session.project.id {
        return auth::web_error(ServiceError::not_found("task", id));
    }
    if let Err(error) = state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(operation),
            format!(
                "{operation}:{id}:{}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
        )
        .await
    {
        return auth::web_error(error);
    }
    Redirect::to(&format!("/tasks/{id}")).into_response()
}

pub async fn tasks_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TaskListQuery>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let filter = match query.into_filter() {
        Ok(filter) => filter,
        Err(message) => return auth::web_error(ServiceError::Validation(message)),
    };
    let tasks = match state
        .tasks
        .list(session.project.id, filter, web_task_viewer(&session))
        .await
    {
        Ok(tasks) => tasks,
        Err(error) => return auth::web_error(error),
    };
    no_store(foundry_web::tasks_fingerprint(&tasks))
}

pub async fn task_state(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let task = match state
        .tasks
        .get(session.project.id, id, web_task_viewer(&session))
        .await
    {
        Ok(task) => task,
        Err(error) => return auth::web_error(error),
    };
    no_store(foundry_web::task_fingerprint(&task))
}

fn no_store(body: String) -> Response {
    let mut response = Html(body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn no_store_text(body: String) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn web_task_viewer(session: &WebSession) -> TaskViewer {
    if session
        .role
        .is_some_and(|role| role >= ProjectRole::Maintainer)
    {
        TaskViewer::manager(session.account.id)
    } else {
        TaskViewer::member(session.account.id)
    }
}

// ---------------------------------------------------------------------------
// MCP
// ---------------------------------------------------------------------------

pub async fn mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let session = match auth::api_session(&state, &headers).await {
        Ok(session) => session,
        Err(error) => return error.into_response(),
    };
    let actor = session.actor();
    let caller = foundry_api_mcp::Caller {
        actor: &actor,
        project_id: session.project.id,
        can_contribute: session.can_contribute(),
        can_dispose: session.can_dispose(),
    };
    let services = foundry_api_mcp::Services {
        issues: state.issues.as_ref(),
        workflows: state.workflows.as_ref(),
        task_links: state.task_links.as_ref(),
    };
    let response = foundry_api_mcp::handle(&services, &caller, body).await;
    if response.is_null() {
        StatusCode::ACCEPTED.into_response()
    } else {
        Json(response).into_response()
    }
}

fn require_contribute(session: &ApiSession) -> Result<(), ApiError> {
    if session.can_contribute() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "project role does not allow contributions",
        ))
    }
}

fn require_dispose(session: &ApiSession) -> Result<(), ApiError> {
    if session.can_dispose() {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "project role does not allow dispositions",
        ))
    }
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

fn task_viewer(session: &ApiSession) -> TaskViewer {
    if session.can_manage_members() {
        TaskViewer::manager(session.account.id)
    } else {
        TaskViewer::member(session.account.id)
    }
}

pub async fn list_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<TaskListQuery>,
) -> Result<Json<Vec<foundry_domain::Task>>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
    let filter = query
        .into_filter()
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "invalid_filter", message))?;
    Ok(Json(
        state
            .tasks
            .list(session.project.id, filter, task_viewer(&session))
            .await?,
    ))
}

pub async fn get_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<foundry_domain::Task>, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    Ok(Json(
        state
            .tasks
            .get(session.project.id, id, task_viewer(&session))
            .await?,
    ))
}

pub async fn bind_task_actor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<BindTaskActorRequest>,
) -> Result<StatusCode, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let agent_may_bind = session.account.kind == foundry_domain::AccountKind::Agent
        && session.project.member(session.account.id).is_some();
    if !session.account.is_admin() && !session.can_dispose() && !agent_may_bind {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only a project maintainer, administrator or project agent can bind task actors",
        ));
    }
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    let Some(role) = task.role_specs.iter().find(|role| role.name == body.role) else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown_role",
            format!("workflow does not declare role {}", body.role),
        ));
    };
    if role.kind != foundry_domain::RoleKind::Human {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "agent_role",
            "agent roles are bound through agent assignment, not human binding",
        ));
    }
    let detail = state
        .projects
        .detail(session.account.actor(), session.project.id)
        .await?;
    let member = detail
        .members
        .iter()
        .find(|member| member.account_id == body.account_id)
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_a_member",
                "the bound account is not a member of this project",
            )
        })?;
    if member.kind != foundry_domain::AccountKind::Human
        || member.status != foundry_domain::AccountStatus::Active
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_a_human",
            "human task roles require an active human project member",
        ));
    }
    if !detail
        .project
        .role_of(body.account_id)
        .is_some_and(ProjectRole::can_contribute)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "viewer_cannot_approve",
            "human task roles require a project role that can contribute",
        ));
    }
    state
        .tasks
        .bind_actor(
            session.project.id,
            id,
            foundry_domain::RoleKey::new(body.role),
            body.account_id,
            session.account.id,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn spawn_task_children(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<SpawnChildTasksRequest>,
) -> Result<(StatusCode, Json<Vec<foundry_domain::Task>>), ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    let active = session.account.is_admin()
        || task
            .actors
            .iter()
            .any(|actor| actor.account_id == session.account.id && actor.active);
    if !active {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only an active task actor or an administrator can spawn child tasks",
        ));
    }
    let tasks = body
        .tasks
        .into_iter()
        .map(|task| foundry_app::SpawnTask {
            workflow: task.workflow,
            title: task.title,
            description: task.description,
            input: task.input,
            depends_on: task.depends_on,
        })
        .collect();
    let children = state
        .tasks
        .spawn_children(
            session.project.id,
            id,
            tasks,
            body.sequential,
            session.account.id,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(children)))
}

pub async fn create_follow_up_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<FollowUpTaskRequest>,
) -> Result<(StatusCode, Json<foundry_domain::Task>), ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    let active = session.account.is_admin()
        || task
            .actors
            .iter()
            .any(|actor| actor.account_id == session.account.id && actor.active);
    if !active {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only an active task actor or an administrator can create follow-up tasks",
        ));
    }
    if !matches!(
        body.workflow.as_str(),
        "design_baseline" | "design_correction" | "design"
    ) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_workflow",
            "the triage task can only create design follow-up tasks",
        ));
    }
    let root_id = session.project.root_task_id.ok_or_else(|| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "no_root_task",
            "project has no root task",
        )
    })?;
    let issue = state.issues.get(session.project.id, body.issue_id).await?;
    let mut input = if body.input.is_object() {
        body.input
    } else {
        json!({})
    };
    input["issue_id"] = json!(issue.id);
    let created = state
        .tasks
        .create_task_with_dependencies(
            session.project.id,
            NewTask {
                parent_id: root_id,
                workflow: body.workflow,
                title: body.title,
                description: body.description,
                input,
            },
            Vec::new(),
            session.account.id,
        )
        .await?;
    state
        .task_links
        .add(
            session.account.actor(),
            AddTaskLink {
                task_id: created.id,
                kind: TaskLinkKind::Issue,
                target_id: Some(issue.id),
                url: None,
                title: issue.title.clone(),
                relation: TaskLinkRelation::References,
                active: true,
                metadata: json!({ "issue_id": issue.id, "source": "triage" }),
            },
        )
        .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

fn design_space_approval_operation(task: &foundry_domain::Task, operation: &str) -> bool {
    matches!(
        task.workflow.as_str(),
        "goal_clarification"
            | "requirements_analysis"
            | "design"
            | "design_change"
            | "design_correction"
            | "design_baseline"
    ) && task.stage.as_deref() == Some("human_review")
        && matches!(operation, "merge" | "approved" | "review_approved")
}

pub async fn send_task_operation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<TaskOperationRequest>,
) -> Result<StatusCode, ApiError> {
    let operation_name = body
        .operation
        .rsplit_once(':')
        .map(|(_, name)| name)
        .unwrap_or(body.operation.as_str());
    if [
        foundry_domain::START_OPERATION,
        foundry_domain::ACCEPT_OPERATION,
        foundry_domain::REJECT_OPERATION,
        foundry_domain::RETRY_OPERATION,
        foundry_domain::ABANDON_OPERATION,
    ]
    .contains(&operation_name)
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "reserved_operation",
            "use the dedicated task endpoint for this operation",
        ));
    }
    if !body.operation.contains(':') {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_operation",
            "operation must use the stage:operation form",
        ));
    }
    let session = task_api_session(&state, &headers, id).await?;
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    if design_space_approval_operation(&task, operation_name) {
        if !state
            .merge_requests
            .has_merge_request(session.project.id, id)
            .await?
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "merge_request_required",
                "design-space changes require a merge request before approval",
            ));
        }
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "reserved_operation",
            "use the merge request merge endpoint so the branch is moved",
        ));
    }
    if operation_name == foundry_domain::DESIGN_SPACE_MERGE_OPERATION
        && state
            .merge_requests
            .has_merge_request(session.project.id, id)
            .await?
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "reserved_operation",
            "use the merge request merge endpoint so the branch is moved",
        ));
    }
    if (operation_name == "approved" || operation_name == "review_approved")
        && state
            .merge_requests
            .has_merge_request(session.project.id, id)
            .await?
    {
        let task = state
            .tasks
            .get(session.project.id, id, task_viewer(&session))
            .await?;
        if matches!(
            task.stage.as_ref().map(|stage| stage.as_str()),
            Some("review" | "human_review")
        ) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "reserved_operation",
                "use the merge request approve endpoint so the approved content digest is recorded",
            ));
        }
    }
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    if task.workflow.as_str() == "design_change" {
        let stage = task.stage.as_ref().map(|stage| stage.as_str());
        if operation_name == "approved"
            && stage == Some("design_review")
            && !state
                .merge_requests
                .has_merge_request(session.project.id, id)
                .await?
        {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "merge_request_required",
                "create the design-change merge request before approving the review",
            ));
        }
        if operation_name == "merged" && stage == Some("merge") {
            let merged = state
                .merge_requests
                .for_task_review(&session.project, id, task_viewer(&session))
                .await?
                .is_some_and(|review| review.status == MergeRequestStatus::Merged);
            if !merged {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "merge_request_not_merged",
                    "merge the design-change MR through the merge request endpoint first",
                ));
            }
        }
    }
    state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(body.operation),
            body.idempotency_key,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn retry_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    if !session.account.is_admin() && !session.can_dispose() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only project maintainers can retry tasks",
        ));
    }
    state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(foundry_domain::RETRY_OPERATION),
            format!(
                "retry:{id}:{}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn abandon_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    if !session.account.is_admin() && !session.can_dispose() {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only project maintainers can abandon tasks",
        ));
    }
    state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(foundry_domain::ABANDON_OPERATION),
            format!(
                "abandon:{id}:{}",
                OffsetDateTime::now_utc().unix_timestamp_nanos()
            ),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn start_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    if session.account.role != AccountRole::Admin {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "only administrators can start tasks",
        ));
    }
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    match task.dispatch_state {
        foundry_domain::TaskDispatchState::Ready => {}
        foundry_domain::TaskDispatchState::Running => return Ok(StatusCode::NO_CONTENT),
        foundry_domain::TaskDispatchState::Blocked
        | foundry_domain::TaskDispatchState::Failed
        | foundry_domain::TaskDispatchState::Closed => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_ready",
                "task is not Ready",
            ));
        }
    }
    let ready = state
        .agents
        .ready_tasks(session.account.actor(), Some(session.project.id))
        .await?;
    let Some(view) = ready.iter().find(|view| view.task_id == id) else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_ready",
            "task is not a Ready dispatch candidate",
        ));
    };
    if view.roles.iter().any(|role| {
        role.kind == foundry_domain::RoleKind::Agent
            && role.agent.as_ref().is_none_or(|agent| !agent.enabled)
    }) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "roles_unassigned",
            "assign every agent role before starting the task",
        ));
    }
    state
        .tasks
        .send_operation(
            session.project.id,
            id,
            session.account.id,
            OperationName::new(foundry_domain::START_OPERATION),
            foundry_domain::START_OPERATION.to_string(),
        )
        .await?;
    // Starting a task makes it the Issue's current handler; previous handlers
    // stay in the link list as history.
    state.task_links.handoff_task_issues(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn task_allowed_operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AllowedOperation>>, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let task = state
        .tasks
        .get(session.project.id, id, task_viewer(&session))
        .await?;
    Ok(Json(task.allowed_operations))
}

pub async fn list_task_comments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<foundry_domain::TaskComment>>, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let (limit, offset) = query.page(50);
    Ok(Json(
        state
            .tasks
            .comments(session.project.id, id, limit, offset)
            .await?,
    ))
}

pub async fn add_task_comment(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<TaskCommentRequest>,
) -> Result<(StatusCode, Json<foundry_domain::Task>), ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let viewer = task_viewer(&session);
    let task = state
        .tasks
        .add_comment(
            session.project.id,
            id,
            session.account.id,
            body.body,
            viewer,
        )
        .await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn task_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Query(query): Query<PageQuery>,
) -> Result<Json<Vec<foundry_domain::TaskOperation>>, ApiError> {
    let session = task_api_session(&state, &headers, id).await?;
    let (limit, offset) = query.page(50);
    Ok(Json(
        state
            .tasks
            .history(session.project.id, id, task_viewer(&session), limit, offset)
            .await?,
    ))
}

// ---------------------------------------------------------------------------
// Workflows
// ---------------------------------------------------------------------------

/// Parses `If-Match: "<revision>"`. Missing headers are a precondition error;
/// anything that is not a single quoted integer is a format error.
fn if_match(headers: &HeaderMap) -> Result<u64, ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "precondition_required",
            "If-Match is required",
        ));
    };
    let invalid = || {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_if_match",
            "If-Match must be a quoted revision, for example \"7\"",
        )
    };
    let text = value.to_str().map_err(|_| invalid())?.trim();
    let revision = text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .ok_or_else(invalid)?;
    revision.parse().map_err(|_| invalid())
}

pub async fn list_workflows(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<foundry_domain::WorkflowDefinition>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state.workflows.list(account.actor()).await,
    )?))
}

pub async fn create_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateWorkflowRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let (definition, draft) = workflow_error(
        state
            .workflows
            .create(account.actor(), body.into_command())
            .await,
    )?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "definition": definition, "draft": draft })),
    ))
}

pub async fn get_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<foundry_domain::WorkflowDefinition>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .get(account.actor(), WorkflowId::new(id))
            .await,
    )?))
}

pub async fn rename_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<RenameWorkflowRequest>,
) -> Result<Json<foundry_domain::WorkflowDefinition>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = body.into_command(WorkflowId::new(id));
    Ok(Json(workflow_error(
        state.workflows.rename(account.actor(), command).await,
    )?))
}

pub async fn list_workflow_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Vec<foundry_domain::WorkflowVersionInfo>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .versions(account.actor(), WorkflowId::new(id))
            .await,
    )?))
}

pub async fn get_workflow_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, u32)>,
) -> Result<Json<foundry_domain::WorkflowVersion>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .version(account.actor(), WorkflowId::new(id), version)
            .await,
    )?))
}

pub async fn open_workflow_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<foundry_domain::WorkflowDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .open_draft(account.actor(), WorkflowId::new(id))
            .await,
    )?))
}

pub async fn get_workflow_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<foundry_domain::WorkflowDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .get_draft(account.actor(), WorkflowId::new(id))
            .await,
    )?))
}

pub async fn save_workflow_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<SaveDraftRequest>,
) -> Result<Json<foundry_app::DraftSaved>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let revision = if_match(&headers)?;
    let command = body.into_command(WorkflowId::new(id), revision);
    Ok(Json(workflow_error(
        state.workflows.save_draft(account.actor(), command).await,
    )?))
}

pub async fn validate_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<ValidateWorkflowRequest>,
) -> Result<Json<foundry_domain::Validation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .validate(account.actor(), WorkflowId::new(id), body.body)
            .await,
    )?))
}

pub async fn publish_workflow(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<foundry_app::WorkflowPublished>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = PublishWorkflow {
        id: WorkflowId::new(id),
        expected_revision: if_match(&headers)?,
    };
    Ok(Json(workflow_error(
        state.workflows.publish(account.actor(), command).await,
    )?))
}

pub async fn get_workflow_layout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WorkflowLayout>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(workflow_error(
        state
            .workflows
            .layout(account.actor(), WorkflowId::new(id))
            .await,
    )?))
}

pub async fn save_workflow_layout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(layout): Json<WorkflowLayout>,
) -> Result<Json<WorkflowLayout>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = SaveWorkflowLayout {
        id: WorkflowId::new(id),
        layout,
    };
    Ok(Json(workflow_error(
        state.workflows.save_layout(account.actor(), command).await,
    )?))
}

pub async fn list_task_links(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Vec<foundry_domain::TaskLink>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(state.task_links.list(account.actor(), task_id).await?))
}

pub async fn add_task_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(task_id): Path<Uuid>,
    Json(body): Json<AddTaskLinkRequest>,
) -> Result<Json<foundry_domain::TaskLink>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let kind = TaskLinkKind::parse(&body.kind).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_link_kind",
            format!("unknown task link kind {}", body.kind),
        )
    })?;
    let relation = TaskLinkRelation::parse(&body.relation).ok_or_else(|| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_task_link_relation",
            format!("unknown task link relation {}", body.relation),
        )
    })?;
    let command = AddTaskLink {
        task_id,
        kind,
        target_id: body.target_id,
        url: body.url,
        title: body.title,
        relation,
        active: body.active,
        metadata: body.metadata,
    };
    Ok(Json(state.task_links.add(account.actor(), command).await?))
}

pub async fn remove_task_link(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((task_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    state
        .task_links
        .remove(account.actor(), RemoveTaskLink { task_id, link_id })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn workflows_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    render_workflows(&state, &session, None).await
}

pub async fn create_workflow_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<NewWorkflowForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    match state
        .workflows
        .create(session.account.actor(), form.into_command())
        .await
    {
        Ok(_) => Redirect::to("/workflows").into_response(),
        Err(ServiceError::Forbidden(message)) => auth::web_forbidden(&message),
        Err(error) => workflow_form_error(&state, &session, error).await,
    }
}

pub async fn rename_workflow_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<RenameWorkflowForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let command = foundry_app::RenameWorkflow {
        id: WorkflowId::new(id),
        title: form.title,
    };
    match state
        .workflows
        .rename(session.account.actor(), command)
        .await
    {
        Ok(_) => Redirect::to("/workflows").into_response(),
        Err(ServiceError::Forbidden(message)) => auth::web_forbidden(&message),
        Err(error) => workflow_form_error(&state, &session, error).await,
    }
}

pub async fn publish_workflow_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<PublishWorkflowForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Ok(expected_revision) = form.revision.trim().parse::<u64>() else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "草稿版本号无效。").into_response();
    };
    let command = PublishWorkflow {
        id: WorkflowId::new(id),
        expected_revision,
    };
    match state
        .workflows
        .publish(session.account.actor(), command)
        .await
    {
        Ok(_) => Redirect::to("/workflows").into_response(),
        Err(ServiceError::Forbidden(message)) => auth::web_forbidden(&message),
        Err(error) => workflow_form_error(&state, &session, error).await,
    }
}

async fn render_workflows(
    state: &AppState,
    session: &WebAccountSession,
    error: Option<&str>,
) -> Response {
    match state.workflows.list(session.account.actor()).await {
        Ok(workflows) => Html(foundry_web::workflows_page(
            &session.context,
            &workflows,
            error,
        ))
        .into_response(),
        Err(error) => auth::web_error(error),
    }
}

async fn workflow_form_error(
    state: &AppState,
    session: &WebAccountSession,
    error: ServiceError,
) -> Response {
    let message = error.to_string();
    let mut response = render_workflows(state, session, Some(&message)).await;
    *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    response
}
