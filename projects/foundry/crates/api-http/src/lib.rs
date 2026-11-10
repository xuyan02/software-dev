pub mod auth;
pub mod dto;
pub mod error;
mod routes;

pub use error::ApiError;
pub use routes::{AppState, EditorAssets, SetupState};

use axum::Router;
use axum::routing::{delete, get, patch, post};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> Router {
    let setup_state = state.clone();
    let editor_index = state.editor.dir.join("index.html");
    let editor = Router::new()
        .nest_service(
            "/workflows/editor",
            ServeDir::new(&state.editor.dir).fallback(ServeFile::new(editor_index)),
        )
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state.editor),
            routes::editor_guard,
        ));
    Router::new()
        .route("/health", get(routes::health))
        .route(
            "/setup",
            get(routes::setup_page).post(routes::setup_login_submit),
        )
        .route("/setup/admin", post(routes::setup_admin_submit))
        .route("/login", get(routes::login_page).post(routes::login_submit))
        .route("/logout", post(routes::logout_submit))
        .route("/", get(routes::index))
        .route(
            "/issues",
            get(routes::issue_index).post(routes::create_issue_form),
        )
        .route("/issues/new", get(routes::new_issue_form))
        .route("/issues/{id}", get(routes::issue_page))
        .route("/issues/{id}/state", get(routes::issue_state))
        .route("/issues/{id}/comments", post(routes::comment_issue_form))
        .route(
            "/issues/{id}/transitions",
            post(routes::transition_issue_form),
        )
        .route("/tasks", get(routes::tasks_index))
        .route("/tasks/state", get(routes::tasks_state))
        .route("/tasks/{id}", get(routes::task_page))
        .route("/tasks/{id}/state", get(routes::task_state))
        .route(
            "/requirements",
            get(routes::requirements_index).post(routes::create_requirement_form),
        )
        .route("/requirements/new", get(routes::new_requirement_form))
        .route("/requirements/{id}", get(routes::requirement_page))
        .route(
            "/projects",
            get(routes::projects_index).post(routes::create_project_form),
        )
        .route("/projects/select", post(routes::select_project_form))
        .route("/projects/{id}", get(routes::project_page))
        .route(
            "/projects/{id}/brief",
            get(routes::project_brief_page).post(routes::save_project_brief_form),
        )
        .route(
            "/projects/{id}/brief/drafts",
            post(routes::create_project_brief_draft_form),
        )
        .route(
            "/projects/{id}/brief/approve",
            post(routes::approve_project_brief_form),
        )
        .route(
            "/projects/{id}/brief/reject",
            post(routes::reject_project_brief_form),
        )
        .route(
            "/projects/{id}/brief/annotations",
            post(routes::annotate_project_brief_form),
        )
        .route(
            "/projects/{id}/brief/annotations/{annotation_id}/resolve",
            post(routes::resolve_project_brief_annotation_form),
        )
        .route(
            "/projects/{id}/requirements",
            get(routes::requirements_document_page),
        )
        .route(
            "/projects/{id}/requirements/publish",
            post(routes::publish_requirements_form),
        )
        .route(
            "/projects/{id}/requirements/reject",
            post(routes::reject_requirements_form),
        )
        .route(
            "/projects/{id}/requirements/annotations",
            post(routes::annotate_requirements_form),
        )
        .route(
            "/projects/{id}/requirements/annotations/{annotation_id}/resolve",
            post(routes::resolve_requirements_annotation_form),
        )
        .route("/projects/{id}/edit", post(routes::update_project_form))
        .route("/projects/{id}/delete", post(routes::delete_project_form))
        .route("/projects/{id}/members", post(routes::add_member_form))
        .route(
            "/projects/{id}/members/{account_id}/role",
            post(routes::set_member_role_form),
        )
        .route(
            "/projects/{id}/members/{account_id}/remove",
            post(routes::remove_member_form),
        )
        .route(
            "/accounts",
            get(routes::accounts_index).post(routes::create_account_form),
        )
        .route(
            "/accounts/{id}",
            get(routes::account_page).post(routes::update_account_form),
        )
        .route("/agents", get(routes::agents_index))
        .route("/agents/{id}/logs", get(routes::agent_logs_page))
        .route("/agents/{id}/start", post(routes::start_agent_form))
        .route("/agents/{id}/stop", post(routes::stop_agent_form))
        .route("/agents/{id}/restart", post(routes::restart_agent_form))
        .route(
            "/workflows",
            get(routes::workflows_index).post(routes::create_workflow_form),
        )
        .route("/workflows/{id}/rename", post(routes::rename_workflow_form))
        .route(
            "/workflows/{id}/publish",
            post(routes::publish_workflow_form),
        )
        .route("/api/v1/auth/login", post(routes::api_login))
        .route("/api/v1/auth/logout", post(routes::api_logout))
        .route("/api/v1/auth/me", get(routes::api_me))
        .route(
            "/api/v1/accounts",
            get(routes::list_accounts).post(routes::create_account),
        )
        .route(
            "/api/v1/accounts/{id}",
            get(routes::get_account).patch(routes::update_account),
        )
        .route(
            "/api/v1/projects",
            get(routes::list_projects).post(routes::create_project),
        )
        .route(
            "/api/v1/projects/{id}",
            get(routes::get_project)
                .patch(routes::update_project)
                .delete(routes::delete_project),
        )
        .route(
            "/api/v1/projects/{id}/members",
            post(routes::add_project_member),
        )
        .route(
            "/api/v1/projects/{id}/members/{account_id}",
            patch(routes::set_project_member_role).delete(routes::remove_project_member),
        )
        .route(
            "/api/v1/workflows",
            get(routes::list_workflows).post(routes::create_workflow),
        )
        .route(
            "/api/v1/workflows/{id}",
            get(routes::get_workflow).patch(routes::rename_workflow),
        )
        .route(
            "/api/v1/workflows/{id}/versions",
            get(routes::list_workflow_versions),
        )
        .route(
            "/api/v1/workflows/{id}/versions/{version}",
            get(routes::get_workflow_version),
        )
        .route(
            "/api/v1/workflows/{id}/draft",
            get(routes::get_workflow_draft)
                .post(routes::open_workflow_draft)
                .put(routes::save_workflow_draft),
        )
        .route(
            "/api/v1/workflows/{id}/validate",
            post(routes::validate_workflow),
        )
        .route(
            "/api/v1/workflows/{id}/publish",
            post(routes::publish_workflow),
        )
        .route(
            "/api/v1/workflows/{id}/layout",
            get(routes::get_workflow_layout).put(routes::save_workflow_layout),
        )
        .route(
            "/api/v1/projects/{project}/brief",
            get(routes::get_project_brief),
        )
        .route(
            "/api/v1/projects/{project}/brief/versions",
            get(routes::list_project_brief_versions),
        )
        .route(
            "/api/v1/projects/{project}/brief/versions/{version}",
            get(routes::get_project_brief_version),
        )
        .route(
            "/api/v1/projects/{project}/brief/drafts",
            get(routes::list_project_brief_drafts).post(routes::create_project_brief_draft),
        )
        .route(
            "/api/v1/projects/{project}/brief/drafts/{draft_id}",
            get(routes::get_project_brief_draft),
        )
        .route(
            "/api/v1/projects/{project}/brief/drafts/{draft_id}/operations",
            post(routes::apply_project_brief_draft_operation),
        )
        .route(
            "/api/v1/projects/{project}/brief/drafts/{draft_id}/submit",
            post(routes::submit_project_brief_draft),
        )
        .route(
            "/api/v1/projects/{project}/brief/approve",
            post(routes::approve_project_brief),
        )
        .route(
            "/api/v1/projects/{project}/brief/annotations",
            get(routes::list_project_brief_annotations).post(routes::annotate_project_brief),
        )
        .route(
            "/api/v1/projects/{project}/brief/annotations/{id}/resolve",
            post(routes::resolve_project_brief_annotation),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document",
            get(routes::get_requirements_document),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/draft",
            get(routes::get_requirements_draft),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/draft/operations",
            post(routes::apply_requirements_operations),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/draft/validate",
            post(routes::validate_requirements_document),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/draft/publish",
            post(routes::publish_requirements_document),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/annotations",
            get(routes::list_requirements_annotations)
                .post(routes::annotate_requirements),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/annotations/{id}/resolve",
            post(routes::resolve_requirements_annotation),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/versions",
            get(routes::list_requirements_versions),
        )
        .route(
            "/api/v1/projects/{project}/requirements-document/versions/{version}",
            get(routes::get_requirements_version),
        )
        .route(
            "/api/v1/requirements",
            get(routes::list_requirements).post(routes::create_requirement),
        )
        .route(
            "/api/v1/requirements/{id}",
            get(routes::get_requirement).patch(routes::update_requirement),
        )
        .route(
            "/api/v1/requirements/{id}/transitions",
            post(routes::transition_requirement),
        )
        .route(
            "/api/v1/issues",
            get(routes::list_issues).post(routes::create_issue),
        )
        .route("/api/v1/issues/{id}", get(routes::get_issue))
        .route("/api/v1/issues/{id}/comments", post(routes::comment_issue))
        .route(
            "/api/v1/issues/{id}/transitions",
            post(routes::transition_issue),
        )
        .route(
            "/api/v1/agent-dispatch/agents",
            get(routes::dispatch_agents),
        )
        .route(
            "/api/v1/agent-dispatch/tasks",
            get(routes::dispatch_ready_tasks),
        )
        .route(
            "/api/v1/agent-dispatch/templates",
            get(routes::dispatch_templates),
        )
        .route(
            "/api/v1/agent-dispatch/create",
            post(routes::dispatch_create),
        )
        .route(
            "/api/v1/agent-dispatch/assign",
            post(routes::dispatch_assign),
        )
        .route("/api/v1/tasks", get(routes::list_tasks))
        .route("/api/v1/tasks/{id}", get(routes::get_task))
        .route(
            "/api/v1/tasks/{id}/operations",
            post(routes::send_task_operation),
        )
        .route("/api/v1/tasks/{id}/start", post(routes::start_task))
        .route(
            "/api/v1/tasks/{id}/allowed-operations",
            get(routes::task_allowed_operations),
        )
        .route(
            "/api/v1/tasks/{id}/comments",
            get(routes::list_task_comments).post(routes::add_task_comment),
        )
        .route("/api/v1/tasks/{id}/history", get(routes::task_history))
        .route(
            "/api/v1/tasks/{id}/links",
            get(routes::list_task_links).post(routes::add_task_link),
        )
        .route(
            "/api/v1/tasks/{id}/links/{link_id}",
            delete(routes::remove_task_link),
        )
        .route("/mcp", post(routes::mcp))
        .merge(editor)
        .layer(axum::middleware::from_fn_with_state(
            setup_state,
            routes::setup_guard,
        ))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
