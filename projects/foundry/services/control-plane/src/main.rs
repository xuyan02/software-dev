//! Foundry control plane.
//!
//! Composition root: the only place that knows which adapters are used.
//! Operational state lives in Postgres; migrations run at startup so a fresh
//! database converges before the first request.
//!
//! Bootstrap is interactive: while the account table is empty the service
//! serves only the first-run setup flow. The operator first signs in with the
//! initial account and password from the config file (an authorization
//! identity, not an account), then creates the real first administrator. There
//! is no default account, no auto-created administrator and no default
//! project. Once accounts exist setup is unreachable, and login sessions are
//! the only source of actor identity. Projects are created explicitly by an
//! account; startup never seeds or repairs domain data.

use foundry_adapter_policy::DemoPolicy;
use foundry_adapter_postgres::{
    PostgresEventSink, PostgresStore, PostgresTaskDispatcher, PostgresWorkflowCatalog,
};
use foundry_api_http::{AppState, SetupState};
use foundry_app::{
    AccountService, AuthService, IssueService, ProjectService, RequirementService, TaskDispatcher,
    TaskRepository, TaskService, WorkflowCatalog, WorkflowService, WorkflowUseCases,
};
use foundry_shared::Config;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_tracing("foundry=info,tower_http=info");

    let config = Config::from_env()?;
    match &config.config_file {
        Some(path) => tracing::info!(path = %path.display(), "loaded config file"),
        None => match foundry_shared::default_config_path() {
            Some(path) => tracing::info!(
                path = %path.display(),
                "no config file found; using environment and defaults"
            ),
            None => tracing::info!("no config file found; using environment and defaults"),
        },
    }
    let store = Arc::new(PostgresStore::connect(&config.database_url).await?);
    store.migrate().await?;
    store.seed_builtin_workflows().await?;
    let events = Arc::new(PostgresEventSink::new(store.pool().clone()));
    let policy = Arc::new(DemoPolicy);
    let workflows: Arc<dyn WorkflowCatalog> =
        Arc::new(PostgresWorkflowCatalog::new(store.pool().clone()));
    let task_dispatcher: Arc<dyn TaskDispatcher> = Arc::new(PostgresTaskDispatcher::new(
        store.pool().clone(),
        Arc::clone(&workflows),
    ));

    let requirements =
        RequirementService::new(Arc::clone(&store), Arc::clone(&events), Arc::clone(&policy));
    let issues = IssueService::new(Arc::clone(&store), Arc::clone(&events), Arc::clone(&policy));
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));
    let projects = Arc::new(ProjectService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::clone(&workflows),
        Arc::clone(&task_dispatcher),
    ));
    let task_repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = Arc::new(TaskService::new(
        task_repository,
        Arc::clone(&task_dispatcher),
        workflows,
    ));
    let workflows_admin: Arc<dyn WorkflowUseCases> = Arc::new(WorkflowService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let auth = Arc::new(AuthService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        config.session_ttl_hours,
    ));

    let setup_required = accounts.setup_required().await?;
    if setup_required {
        // Without the gate, anyone who can reach the port could claim the
        // first administrator. Refuse to start rather than expose an
        // unauthorized setup window.
        if config
            .bootstrap_password
            .as_deref()
            .filter(|value| !value.is_empty())
            .is_none()
        {
            anyhow::bail!(
                "the account table is empty but no initialization password is configured; \
                 set bootstrap_password in the Foundry config file or \
                 FOUNDRY_BOOTSTRAP_PASSWORD to authorize first-run setup"
            );
        }
        tracing::warn!(
            "no accounts exist; open http://{}/setup, sign in with the configured initial \
             account and password, then create the first administrator",
            config.addr
        );
    } else {
        // Refuse to start an unreachable system: accounts exist but nobody can
        // administer it. Recover by restoring an administrator in the database.
        accounts.require_active_admin().await?;
    }

    let setup = Arc::new(SetupState::new(
        setup_required,
        config.bootstrap_handle.clone(),
        config.bootstrap_name.clone(),
        if setup_required {
            config.bootstrap_password.as_deref()
        } else {
            None
        },
    ));
    let state = AppState {
        requirements: Arc::new(requirements),
        issues: Arc::new(issues),
        accounts,
        projects,
        tasks,
        workflows: workflows_admin,
        auth,
        setup,
        editor: Arc::new(foundry_api_http::EditorAssets::new(resolve_editor_dir(
            &config.editor_dir,
        ))),
    };
    let app = foundry_api_http::router(state);
    tokio::spawn(run_task_dispatcher(task_dispatcher));

    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    tracing::info!(
        addr = %config.addr,
        "foundry control plane listening"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Locate the editor build.
///
/// A relative `editor_dir` is relative to the process working directory, but
/// the documented launch command (`cargo run -p foundry-control-plane` from the
/// workspace root) does not share it. Fall back to the same path under the
/// project root this binary was built from, which covers every launch
/// directory inside the repository.
fn resolve_editor_dir(configured: &Path) -> PathBuf {
    if configured.is_absolute() || configured.is_dir() {
        return configured.to_path_buf();
    }
    let candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(configured);
    if candidate.is_dir() {
        candidate
    } else {
        configured.to_path_buf()
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}

/// Runs the task scheduler. Ready tasks are woken first so their
/// `DispatchRequested` events are consumed by the same tick.
async fn run_task_dispatcher(dispatcher: Arc<dyn TaskDispatcher>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        match dispatcher.dispatch_ready().await {
            Ok(0) => {}
            Ok(count) => tracing::debug!(count, "woke ready tasks"),
            Err(error) => tracing::warn!(%error, "task scheduler failed"),
        }
        match dispatcher.process_pending(100).await {
            Ok(0) => {}
            Ok(count) => tracing::debug!(count, "processed task trigger partitions"),
            Err(error) => tracing::warn!(%error, "task dispatcher failed"),
        }
    }
}
