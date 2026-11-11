use crate::ApiError;
use crate::auth::{self, ApiSession, WebAccountSession, WebSession};
use crate::dto::{
    AccountUpdateForm, AddAnchorAnnotationRequest, AddMemberForm, AddMemberRequest,
    AddTaskLinkRequest, AgentTemplateQuery, AnchorAnnotationForm, AnchorAnnotationQuery,
    AnchorQuery, AnnotateRequirementsForm, AnnotateRequirementsRequest,
    ApplyAnchorOperationsRequest, ApplyRequirementsOperationsRequest, ApproveDocumentForm,
    ApproveRequirementsForm, AssignAgentRequest, CommentIssueForm, CommentIssueRequest,
    CreateAccountRequest, CreateAgentFromTemplateRequest, CreateIssueRequest, CreateProjectRequest,
    CreateWorkflowRequest, DecideAnchorDocumentRequest, DecideRequirementsRequest, IssueListQuery,
    IssueTransitionForm, LoginForm, LoginQuery, LoginRequest, LoginResponse, MemberRoleForm,
    NewAccountForm, NewIssueForm, NewProjectForm, NewWorkflowForm, PageQuery, ProjectUpdateForm,
    PublishWorkflowForm, RejectDocumentForm, RejectRequirementsForm, RenameWorkflowForm,
    RenameWorkflowRequest, RequirementsAnnotationQuery, ResolveAnchorAnnotationRequest,
    ResolveRequirementsAnnotationRequest, SaveDraftRequest, SelectProjectForm,
    SetMemberRoleRequest, SetupAdminForm, SetupLoginForm, TaskCommentRequest, TaskListQuery,
    TaskOperationRequest, TransitionIssueRequest, UpdateAccountRequest, UpdateProjectRequest,
    ValidateWorkflowRequest,
};
use crate::error::{anchor_document_error, requirements_error, workflow_error};
use axum::Form;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use foundry_app::{
    AccountFilter, AccountUseCases, AddAnchorAnnotation, AddTaskLink, AgentAdminUseCases,
    AnchorDocumentUseCases, AnchorRef, AnnotateRequirements, ApplyAnchorOperations,
    ApplyRequirementsOperations, AuthUseCases, DecideAnchorDocument, DecideRequirementsDraft,
    IssueFilter, IssueUseCases, ProjectDetail, ProjectUseCases, PublishWorkflow,
    RemoveProjectMember, RemoveTaskLink, RequirementsApprovalResult, RequirementsApprovalView,
    RequirementsDocumentUseCases, RequirementsDocumentView, RequirementsDraftView,
    ResolveAnchorAnnotation, ResolveRequirementsAnnotation, SaveWorkflowLayout, ServiceError,
    TaskFilter, TaskLinkUseCases, TaskUseCases, TaskViewer, WorkflowUseCases, hash_token,
    new_session_token,
};
use foundry_domain::{
    AccountRole, AllowedOperation, ApprovalDecision, OperationName, Project, ProjectRole,
    RequirementsAnnotation, Task, TaskLinkKind, TaskLinkRelation, TaskState, WorkflowId,
    WorkflowLayout,
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
    pub agents: Arc<dyn AgentAdminUseCases>,
    pub agent_logs: Arc<foundry_shared::AgentLogFiles>,
    pub workflows: Arc<dyn WorkflowUseCases>,
    pub anchor_docs: Arc<dyn AnchorDocumentUseCases>,
    pub requirements_doc: Arc<dyn RequirementsDocumentUseCases>,
    pub task_links: Arc<dyn TaskLinkUseCases>,
    pub auth: Arc<dyn AuthUseCases>,
    pub setup: Arc<SetupState>,
    pub editor: Arc<EditorAssets>,
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
) -> Result<Json<Vec<foundry_domain::AgentDefinition>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(state.agents.agents(account.actor()).await?))
}

pub async fn dispatch_ready_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<foundry_domain::ReadyTaskView>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(state.agents.ready_tasks(account.actor()).await?))
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

pub async fn agent_logs_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    if session.account.role != AccountRole::Admin {
        return auth::web_forbidden("仅管理员可以查看智能体日志。");
    }
    let definition = match state.agents.agents(session.account.actor()).await {
        Ok(definitions) => definitions
            .into_iter()
            .find(|definition| definition.id == id),
        Err(error) => return auth::web_error(error),
    };
    let Some(definition) = definition else {
        return auth::web_error(ServiceError::not_found("agent", id));
    };
    match state.agent_logs.tail(&id.to_string(), 256 * 1024) {
        Ok(log) => Html(foundry_web::agent_logs_page(
            &session.context,
            &definition,
            &log,
        ))
        .into_response(),
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
    match state.agents.overview(session.account.actor()).await {
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
        Ok(project) => Redirect::to(&format!("/projects/{}", project.id)).into_response(),
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
            Html(foundry_web::project_page(
                &session.context,
                &detail.project,
                &members,
                account_can_manage(&session.account, &detail.project),
                None,
            ))
            .into_response()
        }
        Err(error) => auth::web_error(error),
    }
}

pub async fn anchor_document_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(Uuid, String)>,
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
    match render_anchor_document(&state, &session, &project, &kind, None).await {
        Ok(response) => response,
        Err(error) => auth::web_error(error),
    }
}

pub async fn anchor_document_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(Uuid, String)>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(_project) = session_project(&session, id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let view = match state
        .anchor_docs
        .get(
            session.account.actor(),
            id,
            &kind,
            &AnchorRef::Key("document".to_string()),
        )
        .await
    {
        Ok(view) => view,
        Err(error) => return auth::web_error(error),
    };
    let annotations = match state
        .anchor_docs
        .annotations(session.account.actor(), id, &kind, true)
        .await
    {
        Ok(annotations) => annotations,
        Err(error) => return auth::web_error(error),
    };
    let signature = foundry_web::anchor_document_signature(
        view.current_version,
        view.latest_version,
        view.draft_revision,
        view.draft_version,
        &annotations,
    );
    Json(json!({ "signature": signature })).into_response()
}

async fn render_anchor_document(
    state: &AppState,
    session: &WebAccountSession,
    project: &Project,
    kind: &str,
    error: Option<&str>,
) -> Result<Response, ServiceError> {
    let view = state
        .anchor_docs
        .get(
            session.account.actor(),
            project.id,
            kind,
            &AnchorRef::Key("document".to_string()),
        )
        .await?;
    let content = state
        .anchor_docs
        .content_with_anchors(
            session.account.actor(),
            project.id,
            kind,
            &AnchorRef::Key("document".to_string()),
        )
        .await?;
    let annotations = state
        .anchor_docs
        .annotations(session.account.actor(), project.id, kind, true)
        .await?;
    let review_link = state
        .anchor_docs
        .review_link(session.account.actor(), project.id, kind)
        .await?;
    let viewer = if session.account.is_admin()
        || project
            .role_of(session.account.id)
            .is_some_and(|role| role >= ProjectRole::Maintainer)
    {
        TaskViewer::manager(session.account.id)
    } else {
        TaskViewer::member(session.account.id)
    };
    let mut review_task = match &review_link {
        Some(link) => state.tasks.get(project.id, link.task_id, viewer).await.ok(),
        None => None,
    };
    if review_task.is_none() {
        review_task = active_human_review_task(state, project.id, kind)
            .await
            .ok()
            .flatten();
    }
    let review_task_id = review_link
        .as_ref()
        .map(|link| link.task_id.to_string())
        .or_else(|| review_task.as_ref().map(|task| task.id.to_string()));
    let review_task_stage = review_task.as_ref().and_then(|task| task.stage.clone());
    let approvals = state
        .anchor_docs
        .approvals(session.account.actor(), project.id, kind)
        .await?;
    let approvals = approvals
        .into_iter()
        .map(|approval| foundry_web::RequirementsApprovalData {
            revision: approval.revision,
            decision: approval.decision,
            actor_id: approval.actor_id,
            comment: approval.comment,
            created_at: approval.created_at,
        })
        .collect::<Vec<_>>();
    Ok(Html(foundry_web::anchor_document_page(
        &session.context,
        project,
        foundry_web::AnchorDocumentPageData {
            kind,
            current_version: view.current_version,
            latest_version: view.latest_version,
            draft_revision: view.draft_revision,
            draft_version: view.draft_version,
            root_id: &content.anchor_id,
            root_key: &content.key,
            review_task_id: review_task_id.as_deref(),
            review_task_stage: review_task_stage.as_deref(),
            markdown: &content.markdown,
            annotations: &annotations,
            approvals: &approvals,
            can_approve: can_approve_requirements(session, project),
            error,
        },
    ))
    .into_response())
}

pub async fn approve_anchor_document_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(Uuid, String)>,
    Form(form): Form<ApproveDocumentForm>,
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
    if !can_approve_requirements(&session, &project) {
        return auth::web_forbidden("只有项目 Maintainer/Owner 可以批准文档。");
    }
    let review_task = match form.task_id {
        Some(task_id) => match human_review_task_by_id(&state, id, &kind, task_id).await {
            Ok(Some(task)) => Some(task),
            Ok(None) => {
                return (StatusCode::CONFLICT, "关联任务不在人类审批阶段").into_response();
            }
            Err(error) => return auth::web_error(error),
        },
        None => None,
    };
    match state
        .anchor_docs
        .decide(
            session.account.actor(),
            DecideAnchorDocument {
                project_id: id,
                kind: kind.clone(),
                expected_revision: form.revision,
                decision: ApprovalDecision::Approved,
                comment: form.comment,
            },
        )
        .await
    {
        Ok(_) => {
            if let Some(task) = review_task {
                match state
                    .tasks
                    .send_operation(
                        id,
                        task.id,
                        session.account.id,
                        OperationName::new(foundry_domain::ACCEPT_OPERATION),
                        format!("anchor-document-approved:{}", task.id),
                    )
                    .await
                {
                    Ok(()) => {
                        release_review_link(&state, session.account.actor(), id, &kind, task.id)
                            .await;
                    }
                    Err(error) => tracing::warn!(
                        %error,
                        task_id = %task.id,
                        "could not accept the linked review task"
                    ),
                }
            }
            Redirect::to(&format!("/projects/{id}/documents/{kind}")).into_response()
        }
        Err(error) => match render_anchor_document(
            &state,
            &session,
            &project,
            &kind,
            Some(&error.to_string()),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => auth::web_error(error),
        },
    }
}

pub async fn reject_anchor_document_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(Uuid, String)>,
    Form(form): Form<RejectDocumentForm>,
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
    if !can_approve_requirements(&session, &project) {
        return auth::web_forbidden("只有项目 Maintainer/Owner 可以打回文档。");
    }
    let reason = form.reason.trim();
    let comment = if reason.is_empty() {
        None
    } else {
        Some(reason.to_string())
    };
    let review_task = match form.task_id {
        Some(task_id) => match human_review_task_by_id(&state, id, &kind, task_id).await {
            Ok(Some(task)) => Some(task),
            Ok(None) => {
                return (StatusCode::CONFLICT, "关联任务不在人类审批阶段").into_response();
            }
            Err(error) => return auth::web_error(error),
        },
        None => match active_anchor_document_task(&state, id, &kind).await {
            Ok(task) => task,
            Err(error) => {
                tracing::warn!(
                    %error,
                    project_id = %id,
                    kind = %kind,
                    "anchor document rejection task lookup failed"
                );
                None
            }
        },
    };
    match state
        .anchor_docs
        .decide(
            session.account.actor(),
            DecideAnchorDocument {
                project_id: id,
                kind: kind.clone(),
                expected_revision: form.revision,
                decision: ApprovalDecision::Rejected,
                comment,
            },
        )
        .await
    {
        Ok(_) => {
            if let Some(task) = review_task {
                match state
                    .tasks
                    .send_operation(
                        id,
                        task.id,
                        session.account.id,
                        OperationName::new(foundry_domain::REJECT_OPERATION),
                        format!("anchor-document-rejected:{}", task.id),
                    )
                    .await
                {
                    Ok(()) => {
                        release_review_link(&state, session.account.actor(), id, &kind, task.id)
                            .await;
                    }
                    Err(error) => tracing::warn!(
                        %error,
                        task_id = %task.id,
                        "anchor document rejection could not move the task"
                    ),
                }
            } else {
                tracing::warn!(
                    project_id = %id,
                    kind = %kind,
                    "anchor document rejected without an active task"
                );
            }
            Redirect::to(&format!("/projects/{id}/documents/{kind}")).into_response()
        }
        Err(error) => match render_anchor_document(
            &state,
            &session,
            &project,
            &kind,
            Some(&error.to_string()),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => auth::web_error(error),
        },
    }
}

pub async fn annotate_anchor_document_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind)): Path<(Uuid, String)>,
    Form(form): Form<AnchorAnnotationForm>,
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
        .anchor_docs
        .annotate(
            session.account.actor(),
            AddAnchorAnnotation {
                project_id: id,
                kind: kind.clone(),
                anchor: form.anchor,
                quote: form.quote,
                body: form.body,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}/documents/{kind}")).into_response(),
        Err(error) => match render_anchor_document(
            &state,
            &session,
            &project,
            &kind,
            Some(&error.to_string()),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => auth::web_error(error),
        },
    }
}

pub async fn resolve_anchor_annotation_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, kind, annotation_id)): Path<(Uuid, String, Uuid)>,
    Form(form): Form<ResolveAnchorAnnotationRequest>,
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
        .anchor_docs
        .resolve_annotation(
            session.account.actor(),
            ResolveAnchorAnnotation {
                project_id: id,
                kind: kind.clone(),
                annotation_id,
                resolution: form.resolution,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}/documents/{kind}")).into_response(),
        Err(error) => match render_anchor_document(
            &state,
            &session,
            &project,
            &kind,
            Some(&error.to_string()),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => auth::web_error(error),
        },
    }
}

pub async fn project_document_redirect(Path(id): Path<Uuid>) -> Redirect {
    Redirect::to(&format!("/projects/{id}/documents/project"))
}

pub async fn requirements_document_page(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    Redirect::to(&format!("/projects/{id}/documents/requirements")).into_response()
}

async fn render_requirements_document(
    state: &AppState,
    session: &WebAccountSession,
    project: &Project,
    view: &RequirementsDocumentView,
    error: Option<&str>,
) -> Response {
    let annotations = match state
        .requirements_doc
        .annotations(session.account.actor(), project.id, false)
        .await
    {
        Ok(annotations) => annotations,
        Err(error) => return auth::web_error(error),
    };
    let approvals: Vec<foundry_web::RequirementsApprovalData> = match state
        .requirements_doc
        .approvals(session.account.actor(), project.id)
        .await
    {
        Ok(approvals) => approvals
            .into_iter()
            .map(|approval| foundry_web::RequirementsApprovalData {
                revision: approval.revision,
                decision: approval.decision,
                actor_id: approval.actor_id,
                comment: approval.comment,
                created_at: approval.created_at,
            })
            .collect(),
        Err(error) => return auth::web_error(error),
    };
    Html(foundry_web::requirements_document_page(
        &session.context,
        project,
        &view.body,
        view.current_version,
        view.latest_version,
        view.draft_revision,
        view.draft_version,
        view.editable,
        can_approve_requirements(session, project),
        &annotations,
        &approvals,
        error,
    ))
    .into_response()
}

pub async fn approve_requirements_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<ApproveRequirementsForm>,
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
    if !can_approve_requirements(&session, &project) {
        return auth::web_forbidden("只有项目 Maintainer/Owner 可以批准需求文档。");
    }
    match state
        .requirements_doc
        .decide(
            session.account.actor(),
            DecideRequirementsDraft {
                project_id: id,
                expected_revision: form.revision,
                decision: ApprovalDecision::Approved,
                comment: form.comment,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}/requirements")).into_response(),
        Err(error) => match state
            .requirements_doc
            .get(session.account.actor(), id)
            .await
        {
            Ok(view) => {
                render_requirements_document(
                    &state,
                    &session,
                    &project,
                    &view,
                    Some(&error.to_string()),
                )
                .await
            }
            Err(error) => auth::web_error(error),
        },
    }
}

pub async fn list_requirements_annotations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Query(query): Query<RequirementsAnnotationQuery>,
) -> Result<Json<Vec<RequirementsAnnotation>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .requirements_doc
            .annotations(account.actor(), project_id, query.include_resolved)
            .await?,
    ))
}

pub async fn annotate_requirements(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<AnnotateRequirementsRequest>,
) -> Result<Json<RequirementsAnnotation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .requirements_doc
            .annotate(
                account.actor(),
                AnnotateRequirements {
                    project_id,
                    target_type: body.target_type,
                    target_id: body.target_id,
                    field: body.field,
                    body: body.body,
                },
            )
            .await?,
    ))
}

pub async fn resolve_requirements_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, annotation_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<ResolveRequirementsAnnotationRequest>,
) -> Result<Json<RequirementsAnnotation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(
        state
            .requirements_doc
            .resolve_annotation(
                account.actor(),
                ResolveRequirementsAnnotation {
                    project_id,
                    annotation_id,
                    resolution: body.resolution,
                },
            )
            .await?,
    ))
}

pub async fn annotate_requirements_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Form(form): Form<AnnotateRequirementsForm>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, project_id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    let Some(target_type) = foundry_domain::AnnotationTarget::parse(&form.target_type) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unknown annotation target",
        )
            .into_response();
    };
    match state
        .requirements_doc
        .annotate(
            session.account.actor(),
            AnnotateRequirements {
                project_id,
                target_type,
                target_id: form.target_id,
                field: form.field,
                body: form.body,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{}/requirements", project.id)).into_response(),
        Err(error) => auth::web_error(error),
    }
}

pub async fn resolve_requirements_annotation_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, annotation_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<ResolveRequirementsAnnotationRequest>,
) -> Response {
    let session = match auth::web_account_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
    };
    let Some(project) = session_project(&session, project_id) else {
        return (
            StatusCode::NOT_FOUND,
            Html(foundry_web::not_found_page(&session.context)),
        )
            .into_response();
    };
    match state
        .requirements_doc
        .resolve_annotation(
            session.account.actor(),
            ResolveRequirementsAnnotation {
                project_id,
                annotation_id,
                resolution: form.resolution,
            },
        )
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{}/requirements", project.id)).into_response(),
        Err(error) => auth::web_error(error),
    }
}

fn session_project(session: &WebAccountSession, id: Uuid) -> Option<Project> {
    session
        .projects
        .iter()
        .find(|project| project.id == id)
        .cloned()
}

fn can_approve_requirements(session: &WebAccountSession, project: &Project) -> bool {
    session.account.is_admin()
        || project
            .role_of(session.account.id)
            .is_some_and(|role| role >= ProjectRole::Maintainer)
}

async fn human_review_requirements_task(
    state: &AppState,
    project_id: Uuid,
) -> Result<Option<Task>, ServiceError> {
    Ok(state
        .tasks
        .list(project_id, TaskFilter::default(), TaskViewer::system())
        .await?
        .into_iter()
        .find(|task| {
            task.workflow.as_str() == "requirements_analysis"
                && task.state == TaskState::InProgress
                && task.stage.as_deref() == Some("human_review")
        }))
}

fn document_review_workflows(kind: &str) -> Option<&'static [&'static str]> {
    match kind {
        "project" => Some(&["goal_clarification"]),
        "requirements" => Some(&["requirements_analysis", "interaction_design", "ui_design"]),
        _ => None,
    }
}

async fn active_anchor_document_task(
    state: &AppState,
    project_id: Uuid,
    kind: &str,
) -> Result<Option<Task>, ServiceError> {
    let Some(workflows) = document_review_workflows(kind) else {
        return Ok(None);
    };
    Ok(state
        .tasks
        .list(project_id, TaskFilter::default(), TaskViewer::system())
        .await?
        .into_iter()
        .find(|task| {
            workflows.contains(&task.workflow.as_str()) && task.state == TaskState::InProgress
        }))
}

async fn human_review_task_by_id(
    state: &AppState,
    project_id: Uuid,
    kind: &str,
    task_id: Uuid,
) -> Result<Option<Task>, ServiceError> {
    let Some(workflows) = document_review_workflows(kind) else {
        return Ok(None);
    };
    let task = state
        .tasks
        .get(project_id, task_id, TaskViewer::system())
        .await?;
    if workflows.contains(&task.workflow.as_str())
        && task.state == TaskState::InProgress
        && task.stage.as_deref() == Some("human_review")
    {
        Ok(Some(task))
    } else {
        Ok(None)
    }
}

async fn active_human_review_task(
    state: &AppState,
    project_id: Uuid,
    kind: &str,
) -> Result<Option<Task>, ServiceError> {
    let Some(workflows) = document_review_workflows(kind) else {
        return Ok(None);
    };
    Ok(state
        .tasks
        .list(project_id, TaskFilter::default(), TaskViewer::system())
        .await?
        .into_iter()
        .find(|task| {
            workflows.contains(&task.workflow.as_str())
                && task.state == TaskState::InProgress
                && task.stage.as_deref() == Some("human_review")
        }))
}

async fn release_review_link(
    state: &AppState,
    actor: foundry_domain::Actor,
    project_id: Uuid,
    kind: &str,
    task_id: Uuid,
) {
    if let Err(error) = state
        .anchor_docs
        .release_review_link(actor, project_id, kind, task_id)
        .await
    {
        tracing::warn!(
            %error,
            project_id = %project_id,
            kind = %kind,
            task_id = %task_id,
            "could not release document review link"
        );
    }
}

pub async fn reject_requirements_form(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Form(form): Form<RejectRequirementsForm>,
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
    if !can_approve_requirements(&session, &project) {
        return auth::web_forbidden("只有项目 Maintainer/Owner 可以打回需求文档。");
    }
    let reason = form.reason.trim();
    if reason.is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "打回原因不能为空").into_response();
    }
    match state
        .requirements_doc
        .decide(
            session.account.actor(),
            DecideRequirementsDraft {
                project_id: id,
                expected_revision: form.revision,
                decision: ApprovalDecision::Rejected,
                comment: Some(reason.to_string()),
            },
        )
        .await
    {
        Ok(_) => {
            if let Ok(Some(task)) = human_review_requirements_task(&state, id).await {
                let _ = state
                    .tasks
                    .send_operation(
                        id,
                        task.id,
                        session.account.id,
                        OperationName::new(foundry_domain::REJECT_OPERATION),
                        format!("requirements-rejected:{}", task.id),
                    )
                    .await;
            }
            Redirect::to(&format!("/projects/{id}/requirements")).into_response()
        }
        Err(error) => auth::web_error(error),
    }
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
    match state
        .projects
        .update(session.account.actor(), form.into_command(id))
        .await
    {
        Ok(_) => Redirect::to(&format!("/projects/{id}")).into_response(),
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
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Html(foundry_web::project_page(
                    &session.context,
                    &detail.project,
                    &members,
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
    match state.projects.delete(session.account.actor(), id).await {
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
    Ok((StatusCode::CREATED, Json(project)))
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
    Ok(Json(
        state
            .projects
            .update(session.actor(), body.into_command(id))
            .await?,
    ))
}

pub async fn delete_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = auth::api_account(&state, &headers).await?;
    state.projects.delete(session.actor(), id).await?;
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
    require_dispose(&session)?;
    Ok(Json(
        state
            .issues
            .transition(body.into_command(session.project.id, id, session.actor()))
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
        Ok(issue) => Html(foundry_web::issue_page(&session.context, &issue)).into_response(),
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

pub async fn tasks_index(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
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
    Html(foundry_web::tasks_page(&session.context, &tasks)).into_response()
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

pub async fn tasks_state(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = match auth::web_session(&state, &headers).await {
        Ok(session) => session,
        Err(response) => return response,
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
    let mut response = body.into_response();
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
        anchor_docs: state.anchor_docs.as_ref(),
        requirements_doc: state.requirements_doc.as_ref(),
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
    let session = auth::api_session(&state, &headers).await?;
    Ok(Json(
        state
            .tasks
            .get(session.project.id, id, task_viewer(&session))
            .await?,
    ))
}

fn review_document_kind(workflow: &str) -> Option<&'static str> {
    match workflow {
        "goal_clarification" => Some("project"),
        "requirements_analysis" | "interaction_design" | "ui_design" => Some("requirements"),
        _ => None,
    }
}

pub async fn send_task_operation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<TaskOperationRequest>,
) -> Result<StatusCode, ApiError> {
    if [
        foundry_domain::START_OPERATION,
        foundry_domain::ACCEPT_OPERATION,
        foundry_domain::REJECT_OPERATION,
    ]
    .contains(&body.operation.as_str())
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "reserved_operation",
            "use the dedicated task endpoint for this operation",
        ));
    }
    let session = auth::api_session(&state, &headers).await?;
    if body.operation == "approved" {
        let task = state
            .tasks
            .get(session.project.id, id, task_viewer(&session))
            .await?;
        if let Some(kind) = review_document_kind(task.workflow.as_str()) {
            let open_annotations = state
                .anchor_docs
                .annotations(session.account.actor(), session.project.id, kind, false)
                .await?
                .into_iter()
                .filter(|annotation| {
                    matches!(
                        annotation.status,
                        foundry_domain::AnchorAnnotationStatus::Open
                    )
                })
                .count();
            if open_annotations > 0 {
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "open_annotations",
                    "cannot approve while open annotations remain; reject the task and wait for the author to resolve them",
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

pub async fn start_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
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
        foundry_domain::TaskDispatchState::Blocked | foundry_domain::TaskDispatchState::Closed => {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "not_ready",
                "task is not Ready",
            ));
        }
    }
    let ready = state.agents.ready_tasks(session.account.actor()).await?;
    let Some(view) = ready.iter().find(|view| view.task_id == id) else {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "not_ready",
            "task is not a Ready dispatch candidate",
        ));
    };
    if view
        .roles
        .iter()
        .any(|role| role.agent.as_ref().is_none_or(|agent| !agent.enabled))
    {
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
    Ok(StatusCode::NO_CONTENT)
}

pub async fn task_allowed_operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AllowedOperation>>, ApiError> {
    let session = auth::api_session(&state, &headers).await?;
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
    let session = auth::api_session(&state, &headers).await?;
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
    let session = auth::api_session(&state, &headers).await?;
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
    let session = auth::api_session(&state, &headers).await?;
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

pub async fn get_requirements_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<RequirementsDocumentView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .get(account.actor(), project_id)
            .await,
    )?))
}

pub async fn get_requirements_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<RequirementsDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .draft(account.actor(), project_id)
            .await,
    )?))
}

pub async fn apply_requirements_operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<ApplyRequirementsOperationsRequest>,
) -> Result<Json<RequirementsDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let expected_revision = if_match(&headers)?;
    let command = ApplyRequirementsOperations {
        project_id,
        expected_revision,
        operations: body.operations,
    };
    Ok(Json(requirements_error(
        state.requirements_doc.apply(account.actor(), command).await,
    )?))
}

pub async fn validate_requirements_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<foundry_domain::Validation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .validate(account.actor(), project_id)
            .await,
    )?))
}

pub async fn decide_requirements_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
    Json(body): Json<DecideRequirementsRequest>,
) -> Result<Json<RequirementsApprovalResult>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = DecideRequirementsDraft {
        project_id,
        expected_revision: if_match(&headers)?,
        decision: body.decision,
        comment: body.comment,
    };
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .decide(account.actor(), command)
            .await,
    )?))
}

pub async fn list_requirements_approvals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<RequirementsApprovalView>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .approvals(account.actor(), project_id)
            .await,
    )?))
}

pub async fn list_requirements_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<foundry_domain::RequirementsVersionRow>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .versions(account.actor(), project_id)
            .await,
    )?))
}

pub async fn get_requirements_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, version)): Path<(Uuid, u32)>,
) -> Result<Json<foundry_app::RequirementsVersionView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(requirements_error(
        state
            .requirements_doc
            .version(account.actor(), project_id, version)
            .await,
    )?))
}

pub async fn get_anchor_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Query(query): Query<AnchorQuery>,
) -> Result<Response, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let anchor = AnchorRef::parse(query.anchor.as_deref().unwrap_or("document"));
    let view = anchor_document_error(
        state
            .anchor_docs
            .get(account.actor(), project_id, &kind, &anchor)
            .await,
    )?;
    Ok((
        [(header::CONTENT_TYPE, "text/markdown; charset=utf-8")],
        foundry_app::render_toc_markdown(&view),
    )
        .into_response())
}

pub async fn open_anchor_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
) -> Result<Json<foundry_app::AnchorDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .open_draft(account.actor(), project_id, &kind)
            .await,
    )?))
}

pub async fn create_anchor_draft(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
) -> Result<Json<foundry_app::AnchorDraftView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .create_draft(account.actor(), project_id, &kind)
            .await,
    )?))
}

pub async fn get_anchor_content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Query(query): Query<AnchorQuery>,
) -> Result<Json<foundry_app::AnchorContentView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let anchor = AnchorRef::parse(query.anchor.as_deref().unwrap_or("document"));
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .content(account.actor(), project_id, &kind, &anchor)
            .await,
    )?))
}

pub async fn apply_anchor_operations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Json(body): Json<ApplyAnchorOperationsRequest>,
) -> Result<Json<foundry_app::AnchorApplyView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = ApplyAnchorOperations {
        project_id,
        kind,
        expected_revision: if_match(&headers)?,
        operations: body.operations,
    };
    Ok(Json(anchor_document_error(
        state.anchor_docs.apply(account.actor(), command).await,
    )?))
}

pub async fn validate_anchor_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
) -> Result<Json<foundry_domain::Validation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .validate(account.actor(), project_id, &kind)
            .await,
    )?))
}

pub async fn decide_anchor_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Json(body): Json<DecideAnchorDocumentRequest>,
) -> Result<Json<foundry_app::AnchorApprovalResult>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = DecideAnchorDocument {
        project_id,
        kind,
        expected_revision: if_match(&headers)?,
        decision: body.decision,
        comment: body.comment,
    };
    Ok(Json(anchor_document_error(
        state.anchor_docs.decide(account.actor(), command).await,
    )?))
}

pub async fn list_anchor_approvals(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
) -> Result<Json<Vec<foundry_app::AnchorDocumentApprovalDAO>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .approvals(account.actor(), project_id, &kind)
            .await,
    )?))
}

pub async fn list_anchor_versions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
) -> Result<Json<Vec<foundry_app::AnchorDocumentVersionRow>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .versions(account.actor(), project_id, &kind)
            .await,
    )?))
}

pub async fn get_anchor_version(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind, version)): Path<(Uuid, String, u32)>,
) -> Result<Json<foundry_app::AnchorVersionView>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .version(account.actor(), project_id, &kind, version)
            .await,
    )?))
}

pub async fn list_anchor_annotations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Query(query): Query<AnchorAnnotationQuery>,
) -> Result<Json<Vec<foundry_domain::AnchorAnnotation>>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .annotations(account.actor(), project_id, &kind, query.include_resolved)
            .await,
    )?))
}

pub async fn annotate_anchor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind)): Path<(Uuid, String)>,
    Json(body): Json<AddAnchorAnnotationRequest>,
) -> Result<Json<foundry_domain::AnchorAnnotation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = AddAnchorAnnotation {
        project_id,
        kind,
        anchor: body.anchor,
        quote: body.quote,
        body: body.body,
    };
    Ok(Json(anchor_document_error(
        state.anchor_docs.annotate(account.actor(), command).await,
    )?))
}

pub async fn resolve_anchor_annotation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((project_id, kind, annotation_id)): Path<(Uuid, String, Uuid)>,
    Json(body): Json<ResolveAnchorAnnotationRequest>,
) -> Result<Json<foundry_domain::AnchorAnnotation>, ApiError> {
    let account = auth::api_account(&state, &headers).await?;
    let command = ResolveAnchorAnnotation {
        project_id,
        kind,
        annotation_id,
        resolution: body.resolution,
    };
    Ok(Json(anchor_document_error(
        state
            .anchor_docs
            .resolve_annotation(account.actor(), command)
            .await,
    )?))
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
