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
    AccountService, AgentAdminUseCases, AnchorDocumentService, AnchorDocumentUseCases, AuthService,
    DesignSpaceGateway, IssueService, MergeRequestRepository, MergeRequestService, NewTask,
    ProjectFilter, ProjectRepository, ProjectService, ProjectUseCases, RequirementsDocumentService,
    RequirementsDocumentUseCases, TaskCommand, TaskDispatcher, TaskFilter, TaskLinkService,
    TaskLinkUseCases, TaskRepository, TaskService, TaskUseCases, TaskViewer, TriggerService,
    TriggerUseCases, WorkflowCatalog, WorkflowService, WorkflowUseCases,
};
use foundry_domain::{OperationName, TaskState};
use foundry_shared::{AgentLogFiles, Config};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod agent_admin;
mod agent_credentials;
mod design_space;
mod ds_ssh;
mod github_app;
mod platform_agents;
mod project_purge;
mod supervisor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_tracing("foundry=info,tower_http=info");

    let config = Config::from_file()?;
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
    reevaluate_transitions(store.as_ref(), task_dispatcher.as_ref()).await;

    let issues = IssueService::new(Arc::clone(&store), Arc::clone(&events), Arc::clone(&policy));
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));
    let (local_design_space, design_space_base, design_space_port, design_space_token) =
        design_space::start_local(
            config.design_space_root.clone(),
            config.design_space_listen.clone(),
            config.design_space_token_file.clone(),
        )
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    let design_space_ssh_base = config
        .design_space_ssh_url
        .clone()
        .unwrap_or_else(|| format!("ssh://foundry-ds@{}", design_space::host_ip()));
    tracing::info!(
        url = %design_space_base,
        ssh = %design_space_ssh_base,
        port = design_space_port,
        "started local ds-server"
    );
    // Keep the child alive until the control plane exits. Dropping it kills
    // ds-server (`kill_on_drop`).
    let _local_design_space = local_design_space;
    let ds_ssh = Arc::new(
        ds_ssh::DsSsh::new(
            &foundry_shared::default_foundry_dir(),
            design_space_ssh_base.clone(),
            config.design_space_root.clone(),
            ds_server_binary()?,
            store.pool().clone(),
        )
        .map_err(|error| anyhow::anyhow!(error))?,
    );
    ds_ssh
        .refresh_authorized_keys()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    migrate_design_space_urls(store.as_ref(), &design_space_ssh_base).await?;
    let design_spaces: Arc<dyn foundry_app::ports::DesignSpaceProvisioner> =
        Arc::new(design_space::HttpDesignSpaceProvisioner::new(
            &design_space_base,
            &design_space_ssh_base,
            Some(design_space_token.clone()),
        ));
    let projects = Arc::new(ProjectService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::clone(&workflows),
        Arc::clone(&task_dispatcher),
        design_spaces,
    ));
    let task_repository: Arc<dyn TaskRepository> = store.clone();
    let tasks = Arc::new(TaskService::new(
        task_repository,
        Arc::clone(&task_dispatcher),
        Arc::clone(&workflows),
    ));
    let workflows_admin: Arc<dyn WorkflowUseCases> = Arc::new(WorkflowService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&events),
    ));
    let anchor_docs: Arc<dyn AnchorDocumentUseCases> = Arc::new(AnchorDocumentService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
    ));
    let requirements_doc: Arc<dyn RequirementsDocumentUseCases> =
        Arc::new(RequirementsDocumentService::new(
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
    let auth = Arc::new(AuthService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        config.session_ttl_hours,
    ));
    let credential_auth = Arc::clone(&auth);
    let github =
        github_app::GitHubApp::from_config(&config).map_err(|error| anyhow::anyhow!(error))?;
    let github = github.map(Arc::new);
    if github.is_some() {
        tracing::info!("GitHub App credential broker enabled");
    }

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
    let host = if config.addr.ip().is_unspecified() {
        "127.0.0.1".to_string()
    } else {
        config.addr.ip().to_string()
    };
    let agent_logs = Arc::new(AgentLogFiles::new(AgentLogFiles::default_dir()));
    let supervisor = supervisor::AgentSupervisor::new(
        Arc::clone(&store),
        config.database_url.clone(),
        format!("http://{host}:{}", config.addr.port()),
        config.session_ttl_hours,
        config.openai_api_key.clone().unwrap_or_default(),
        config.shell_image.clone(),
        Arc::clone(&agent_logs),
    );
    let agents: Arc<dyn AgentAdminUseCases> = Arc::new(agent_admin::PostgresAgentAdmin::new(
        Arc::clone(&store),
        Arc::clone(&supervisor),
        config
            .openai_base_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434/v1".to_string()),
        config
            .openai_model
            .clone()
            .unwrap_or_else(|| "qwen2.5-coder".to_string()),
    ));
    let design_space_access = Arc::new(foundry_api_http::DesignSpaceAccess::new(
        config.design_space_root.clone(),
    ));
    let design_space_gateway: Arc<dyn DesignSpaceGateway> = design_space_access.clone();
    let task_use_cases: Arc<dyn TaskUseCases> = tasks.clone();
    let merge_requests = Arc::new(MergeRequestService::new(
        Arc::clone(&store) as Arc<dyn MergeRequestRepository>,
        Arc::clone(&task_use_cases),
        Arc::clone(&task_dispatcher),
        Arc::clone(&workflows),
        Arc::clone(&store) as Arc<dyn foundry_app::AccountRepository>,
        design_space_gateway,
    ));
    let trigger_service = Arc::new(TriggerService::new(
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&store),
        Arc::clone(&tasks),
    ));
    let triggers: Arc<dyn TriggerUseCases> = trigger_service.clone();
    match ProjectRepository::list(store.as_ref(), ProjectFilter::default()).await {
        Ok(projects) => {
            for project in projects {
                if let Err(error) = triggers.ensure_daily_check(project.id).await {
                    tracing::warn!(
                        project_id = %project.id,
                        %error,
                        "cannot ensure default daily-check trigger"
                    );
                }
                if let Err(error) = triggers.ensure_design_iteration(project.id).await {
                    tracing::warn!(
                        project_id = %project.id,
                        %error,
                        "cannot ensure default design-iteration trigger"
                    );
                }
                if let Err(error) = triggers.ensure_development_iteration(project.id).await {
                    tracing::warn!(
                        project_id = %project.id,
                        %error,
                        "cannot ensure default development-iteration trigger"
                    );
                }
                if let Some(root_id) = project.root_task_id {
                    match task_use_cases
                        .list(project.id, TaskFilter::default(), TaskViewer::system())
                        .await
                    {
                        Ok(tasks) => {
                            let has_triage = tasks.iter().any(|task| {
                                task.workflow.as_str() == "triage"
                                    && !matches!(
                                        task.state,
                                        TaskState::Completed | TaskState::Failed
                                    )
                            });
                            if !has_triage
                                && let Some(owner) = project.members.iter().find(|member| {
                                    member.role == foundry_domain::ProjectRole::Owner
                                })
                                && let Err(error) = task_use_cases
                                    .create_task(
                                        project.id,
                                        NewTask {
                                            parent_id: root_id,
                                            workflow: "triage".to_string(),
                                            title: "分诊".to_string(),
                                            description: "常驻分诊任务。".to_string(),
                                            input: serde_json::json!({}),
                                        },
                                        owner.account_id,
                                    )
                                    .await
                            {
                                tracing::warn!(
                                    project_id = %project.id,
                                    %error,
                                    "cannot ensure the persistent triage task"
                                );
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                project_id = %project.id,
                                %error,
                                "cannot list tasks to ensure the triage task"
                            );
                        }
                    }
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "cannot list projects to ensure default triggers");
        }
    }
    let scheduler = Arc::clone(&trigger_service);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(error) = scheduler.run_due(time::OffsetDateTime::now_utc()).await {
                tracing::warn!(%error, "trigger scheduler sweep failed");
            }
        }
    });
    let project_purger: Arc<dyn foundry_api_http::ProjectPurger> =
        Arc::new(project_purge::ProjectPurger::new(
            Arc::clone(&projects) as Arc<dyn ProjectUseCases>,
            Arc::clone(&store),
            Arc::clone(&supervisor),
            config.design_space_root.clone(),
            Arc::clone(&agent_logs),
            Arc::clone(&ds_ssh),
        ));
    let state = AppState {
        issues: Arc::new(issues),
        accounts,
        projects,
        tasks,
        triggers,
        agents,
        agent_logs,
        workflows: workflows_admin,
        anchor_docs,
        requirements_doc,
        task_links,
        auth,
        setup,
        editor: Arc::new(foundry_api_http::EditorAssets::new(resolve_editor_dir(
            &config.editor_dir,
        ))),
        design_space: design_space_access,
        merge_requests,
        project_purger: Some(project_purger),
    };
    let credentials = agent_credentials::CredentialState {
        auth: credential_auth,
        store: Arc::clone(&store),
        github,
        ssh: ds_ssh,
    };
    let app = foundry_api_http::router(state).merge(agent_credentials::router(credentials));
    tokio::spawn(run_task_dispatcher(task_dispatcher, Arc::clone(&store)));

    let listener = tokio::net::TcpListener::bind(config.addr).await?;
    tracing::info!(
        addr = %config.addr,
        "foundry control plane listening"
    );
    let platform_store = Arc::clone(&store);
    let platform_supervisor = Arc::clone(&supervisor);
    let platform_base = config.openai_base_url.clone();
    let platform_model = config.openai_model.clone();
    let platform_has_key = config.openai_api_key.is_some();
    tokio::spawn(async move {
        platform_agents::ensure_project_agents(
            platform_store,
            platform_supervisor,
            platform_base,
            platform_model,
            platform_has_key,
        )
        .await;
    });
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
    tracing::info!("shutdown signal received; waiting up to 5s for requests to finish");
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        tracing::warn!("graceful shutdown timed out; forcing process exit");
        std::process::exit(0);
    });
}

/// Re-evaluate transitions for tasks that were already in progress when the
/// process stopped. Transitions normally run only on a new task event; this
/// startup pass recovers tasks whose persisted stage variables already satisfy
/// a transition.
async fn reevaluate_transitions(store: &PostgresStore, dispatcher: &dyn TaskDispatcher) {
    let ids = match store.in_progress_task_ids().await {
        Ok(ids) => ids,
        Err(error) => {
            tracing::warn!(error = %error, "cannot list in-progress tasks for transition recheck");
            return;
        }
    };
    let mut checked = 0;
    for id in ids {
        match dispatcher.process(id, Some(TaskCommand::Reevaluate)).await {
            Ok(()) => checked += 1,
            Err(error) => {
                tracing::warn!(task_id = %id, error = %error, "transition recheck failed");
            }
        }
    }
    tracing::info!(tasks = checked, "startup transition recheck finished");
}

/// Advances task workflows and reconciles durable state into agent
/// activations. Notifications are not persisted; this loop rebuilds missed
/// ones from task state, assignments and Issue comments.
async fn run_task_dispatcher(dispatcher: Arc<dyn TaskDispatcher>, store: Arc<PostgresStore>) {
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
        match store.approved_goal_clarifications().await {
            Ok(tasks) => {
                for (task_id, _project_id, approved_by, revision) in tasks {
                    let command = TaskCommand::SendOperation {
                        actor_id: approved_by,
                        operation: OperationName::new(format!(
                            "human_review:{}",
                            foundry_domain::ACCEPT_OPERATION
                        )),
                        idempotency_key: format!("anchor-document-approved:{task_id}:{revision}"),
                    };
                    if let Err(error) = dispatcher.process(task_id, Some(command)).await {
                        tracing::warn!(
                            %error,
                            task_id = %task_id,
                            "approved project layer task completion failed"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "approved project layer scan failed"),
        }
        match store.approved_requirement_document_tasks().await {
            Ok(tasks) => {
                for (task_id, _project_id, published_by, revision) in tasks {
                    let command = TaskCommand::SendOperation {
                        actor_id: published_by,
                        operation: OperationName::new(format!(
                            "human_review:{}",
                            foundry_domain::ACCEPT_OPERATION
                        )),
                        idempotency_key: format!("anchor-document-approved:{task_id}:{revision}"),
                    };
                    if let Err(error) = dispatcher.process(task_id, Some(command)).await {
                        tracing::warn!(
                            %error,
                            task_id = %task_id,
                            "published requirements task completion failed"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "published requirements scan failed"),
        }
        match store.approved_architecture_design_tasks().await {
            Ok(tasks) => {
                for (task_id, _project_id, published_by, revision) in tasks {
                    let command = TaskCommand::SendOperation {
                        actor_id: published_by,
                        operation: OperationName::new(format!(
                            "human_review:{}",
                            foundry_domain::ACCEPT_OPERATION
                        )),
                        idempotency_key: format!("anchor-document-approved:{task_id}:{revision}"),
                    };
                    if let Err(error) = dispatcher.process(task_id, Some(command)).await {
                        tracing::warn!(
                            %error,
                            task_id = %task_id,
                            "published architecture task completion failed"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "published architecture scan failed"),
        }
        for (kind, workflows, stage) in [
            ("design", &["detailed_design"][..], "human_review"),
            ("development", &["iteration"][..], "acceptance"),
            ("test_plan", &["test_planning"][..], "human_review"),
            ("test_report", &["test_execution"][..], "human_review"),
        ] {
            match store.approved_document_tasks(kind, workflows, stage).await {
                Ok(tasks) => {
                    for (task_id, _project_id, approved_by, revision) in tasks {
                        let command = TaskCommand::SendOperation {
                            actor_id: approved_by,
                            operation: OperationName::new(format!(
                                "{stage}:{}",
                                foundry_domain::ACCEPT_OPERATION
                            )),
                            idempotency_key: format!(
                                "anchor-document-approved:{task_id}:{revision}"
                            ),
                        };
                        if let Err(error) = dispatcher.process(task_id, Some(command)).await {
                            tracing::warn!(
                                %error,
                                task_id = %task_id,
                                kind,
                                "approved document task completion failed"
                            );
                        }
                    }
                }
                Err(error) => tracing::warn!(%error, kind, "approved document scan failed"),
            }
        }
        match store.rejected_document_tasks().await {
            Ok(tasks) => {
                for (task_id, _project_id, rejected_by, revision, stage) in tasks {
                    let command = TaskCommand::SendOperation {
                        actor_id: rejected_by,
                        operation: OperationName::new(format!(
                            "{stage}:{}",
                            foundry_domain::REJECT_OPERATION
                        )),
                        idempotency_key: format!("anchor-document-rejected:{task_id}:{revision}"),
                    };
                    if let Err(error) = dispatcher.process(task_id, Some(command)).await {
                        tracing::warn!(
                            %error,
                            task_id = %task_id,
                            "document rejection task rollback failed"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "document rejection scan failed"),
        }
        match store.bind_human_review_document_links().await {
            Ok(0) => {}
            Ok(count) => tracing::debug!(count, "bound document review links"),
            Err(error) => tracing::warn!(%error, "document review link bind scan failed"),
        }
        match store.release_stale_document_review_links().await {
            Ok(0) => {}
            Ok(count) => tracing::debug!(count, "released stale document review links"),
            Err(error) => tracing::warn!(%error, "document review link release scan failed"),
        }
    }
}

/// The SSH forced command runs `ds-server` directly instead of going through
/// the control plane. Prefer the sibling built by `cargo build --bins`, and
/// fall back to a system-wide install.
fn ds_server_binary() -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("FOUNDRY_DS_SERVER") {
        return Ok(PathBuf::from(path));
    }
    let exe = std::env::current_exe()?;
    let sibling = exe
        .parent()
        .map(|directory| directory.join("ds-server"))
        .filter(|path| path.is_file());
    if let Some(path) = sibling {
        return Ok(path);
    }
    let installed = PathBuf::from("/usr/local/bin/ds-server");
    if installed.is_file() {
        return Ok(installed);
    }
    anyhow::bail!(
        "ds-server binary not found next to {}; build it with `cargo build --bins` \
         or set FOUNDRY_DS_SERVER",
        exe.display()
    )
}

/// Point every project at the local SSH transport. The repo name is the
/// project id, so the stored URL only records the transport address.
async fn migrate_design_space_urls(store: &PostgresStore, ssh_base: &str) -> anyhow::Result<()> {
    let projects = ProjectRepository::list(store, ProjectFilter::default()).await?;
    for mut project in projects {
        let expected = format!("{ssh_base}/{}", project.id);
        if project.design_space_url == expected {
            continue;
        }
        let version = project.version;
        project.design_space_url = expected;
        ProjectRepository::save(store, &project, version).await?;
    }
    Ok(())
}
