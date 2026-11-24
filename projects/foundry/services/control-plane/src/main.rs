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
    listen_issue_changes,
};
use foundry_api_http::{AppState, SetupState};
use foundry_app::{
    AccountService, AgentAdminUseCases, AuthService, CreateDesignSpaceMergeRequest,
    DesignSpaceGateway, IssueFilter, IssueService, IssueUseCases, MergeRequestRepository,
    MergeRequestService, NewTask, ProjectFilter, ProjectRepository, ProjectService,
    ProjectUseCases, TaskCommand, TaskDispatcher, TaskFilter, TaskLinkService, TaskLinkUseCases,
    TaskRepository, TaskService, TaskUseCases, TaskViewer, TriggerService, TriggerUseCases,
    WorkflowCatalog, WorkflowService, WorkflowUseCases,
};
use foundry_domain::{OperationName, StageKey, TaskState, WorkflowId};
use foundry_shared::{AgentLogFiles, Config};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

mod agent_admin;
mod agent_credentials;
mod arche_ssh;
mod design_space;
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

    let issues = Arc::new(IssueService::new(
        Arc::clone(&store),
        Arc::clone(&events),
        Arc::clone(&policy),
    ));
    let accounts = Arc::new(AccountService::new(Arc::clone(&store), Arc::clone(&events)));
    let arche_ssh_base = config
        .arche_ssh_url
        .clone()
        .unwrap_or_else(|| format!("ssh://foundry-arche@{}", design_space::host_ip()));
    tracing::info!(
        ssh = %arche_ssh_base,
        root = %config.design_space_root.display(),
        "using Arche design spaces"
    );
    let arche_ssh = Arc::new(
        arche_ssh::ArcheSsh::new(
            &foundry_shared::default_foundry_dir(),
            arche_ssh_base.clone(),
            config.design_space_root.clone(),
            store.pool().clone(),
        )
        .map_err(|error| anyhow::anyhow!(error))?,
    );
    arche_ssh
        .refresh_authorized_keys()
        .await
        .map_err(|error| anyhow::anyhow!(error))?;
    let design_spaces: Arc<dyn foundry_app::ports::DesignSpaceProvisioner> =
        Arc::new(design_space::ArcheDesignSpaceProvisioner::new(
            config.design_space_root.clone(),
            &arche_ssh_base,
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
        config.desktop_image.clone(),
        Arc::clone(&agent_logs),
    );
    supervisor.reap_orphans().await;
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
    let wake_store = Arc::clone(&store);
    let wake_tasks = Arc::clone(&task_use_cases);
    let wake_issues = Arc::clone(&issues);
    let wake_database_url = config.database_url.clone();
    tokio::spawn(async move {
        loop {
            let mut notifications = match listen_issue_changes(&wake_database_url).await {
                Ok(receiver) => receiver,
                Err(error) => {
                    tracing::warn!(%error, "cannot listen for issue changes");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            reconcile_iterations(
                wake_store.as_ref(),
                wake_tasks.as_ref(),
                wake_issues.as_ref(),
            )
            .await;
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    notification = notifications.recv() => {
                        if notification.is_none() {
                            break;
                        }
                    }
                }
                reconcile_iterations(
                    wake_store.as_ref(),
                    wake_tasks.as_ref(),
                    wake_issues.as_ref(),
                )
                .await;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
    let project_purger: Arc<dyn foundry_api_http::ProjectPurger> =
        Arc::new(project_purge::ProjectPurger::new(
            Arc::clone(&projects) as Arc<dyn ProjectUseCases>,
            Arc::clone(&store),
            Arc::clone(&supervisor),
            config.design_space_root.clone(),
            Arc::clone(&agent_logs),
            Arc::clone(&arche_ssh),
        ));
    let state = AppState {
        issues,
        accounts,
        projects,
        tasks,
        triggers,
        agents,
        agent_logs,
        workflows: workflows_admin,
        task_links,
        auth,
        setup,
        editor: Arc::new(foundry_api_http::EditorAssets::new(resolve_editor_dir(
            &config.editor_dir,
        ))),
        design_space: design_space_access,
        merge_requests: Arc::clone(&merge_requests),
        project_purger: Some(project_purger),
    };
    let credentials = agent_credentials::CredentialState {
        auth: credential_auth,
        store: Arc::clone(&store),
        github,
        ssh: arche_ssh,
    };
    let app = foundry_api_http::router(state).merge(agent_credentials::router(credentials));
    tokio::spawn(run_task_dispatcher(
        task_dispatcher,
        Arc::clone(&store),
        Arc::clone(&merge_requests),
    ));

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
    tracing::info!("http server stopped; stopping agentd processes");
    if tokio::time::timeout(Duration::from_secs(10), supervisor.shutdown_all())
        .await
        .is_err()
    {
        tracing::warn!("agent shutdown timed out");
    }
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

#[cfg(unix)]
async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
    tracing::info!("shutdown signal received; waiting for requests and agents to stop");
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received; waiting for requests and agents to stop");
}

/// Ensure exactly one long-lived iteration task exists for each non-empty
/// Issue pool, and wake it when it is waiting for work.
async fn reconcile_iterations(
    store: &PostgresStore,
    tasks: &dyn TaskUseCases,
    issues: &dyn IssueUseCases,
) {
    let iterations = [
        (
            "requirements_iteration",
            "requirements",
            "需求迭代",
            "处理需求池中的 Issue。",
        ),
        (
            "design_iteration",
            "design",
            "设计迭代",
            "处理设计池中的 Issue。",
        ),
        (
            "development_iteration",
            "development",
            "开发迭代",
            "处理开发池中的 Issue。",
        ),
        (
            "test_iteration",
            "tests",
            "测试迭代",
            "处理测试池中的 Issue。",
        ),
    ];
    let projects = match ProjectRepository::list(store, ProjectFilter::default()).await {
        Ok(projects) => projects,
        Err(error) => {
            tracing::warn!(%error, "cannot list projects to reconcile iterations");
            return;
        }
    };
    for project in projects {
        let Some(root_id) = project.root_task_id else {
            continue;
        };
        let Some(owner) = project
            .members
            .iter()
            .find(|member| member.role == foundry_domain::ProjectRole::Owner)
        else {
            continue;
        };
        let pooled = match issues.list(project.id, IssueFilter::default()).await {
            Ok(issues) => issues
                .into_iter()
                .filter_map(|issue| {
                    (issue.status.as_str() == "pooled")
                        .then(|| issue.pool.map(|pool| pool.as_str()))
                        .flatten()
                })
                .collect::<std::collections::BTreeSet<_>>(),
            Err(error) => {
                tracing::warn!(project_id = %project.id, %error, "cannot list pooled Issues");
                continue;
            }
        };
        for (workflow, pool, title, description) in iterations {
            if !pooled.contains(pool) {
                continue;
            }
            let filter = TaskFilter {
                workflow: Some(WorkflowId::new(workflow)),
                ..TaskFilter::default()
            };
            let active = match tasks.list(project.id, filter, TaskViewer::system()).await {
                Ok(tasks) => tasks
                    .into_iter()
                    .find(|task| task.state != TaskState::Completed),
                Err(error) => {
                    tracing::warn!(project_id = %project.id, %error, "cannot list iteration tasks");
                    continue;
                }
            };
            let Some(task) = active else {
                if let Err(error) = tasks
                    .create_task(
                        project.id,
                        NewTask {
                            parent_id: root_id,
                            workflow: workflow.to_string(),
                            title: title.to_string(),
                            description: description.to_string(),
                            input: serde_json::json!({}),
                        },
                        owner.account_id,
                    )
                    .await
                {
                    tracing::warn!(project_id = %project.id, workflow, %error, "cannot create iteration task");
                }
                continue;
            };
            if task.stage.as_ref().map(StageKey::as_str) != Some("waiting") {
                continue;
            }
            let Some(actor) = task
                .actors
                .iter()
                .find(|actor| actor.role_key.as_str() == "dispatcher" && actor.active)
            else {
                tracing::warn!(task_id = %task.id, "waiting iteration has no active dispatcher");
                continue;
            };
            if let Err(error) = tasks
                .send_operation(
                    project.id,
                    task.id,
                    actor.account_id,
                    OperationName::new("waiting:work_arrived"),
                    format!("wake:{}", uuid::Uuid::now_v7()),
                )
                .await
            {
                tracing::warn!(task_id = %task.id, %error, "cannot wake waiting iteration");
            }
        }
    }
}

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

fn design_space_branch(workflow: &str, task_id: uuid::Uuid) -> Option<String> {
    match workflow {
        "goal_clarification" => Some(format!("project-document-{task_id}")),
        "requirements_analysis" => Some(format!("requirements/{task_id}")),
        "design" => Some(format!("design/{task_id}")),
        "design_change" => Some(format!("design-change/{task_id}")),
        "design_correction" => Some(format!("design-correction/{task_id}")),
        "design_baseline" => Some(format!("design-baseline/{task_id}")),
        _ => None,
    }
}

/// Every task that changes the design space must have a merge request before
/// a human can approve it. Reviewers normally create it; this reconciliation
/// closes the gap when an LLM reviewer forgets.
async fn ensure_design_space_merge_requests(
    store: &PostgresStore,
    merge_requests: &MergeRequestService,
) {
    let rows = match store.design_space_human_review_tasks().await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "cannot list design-space human review tasks");
            return;
        }
    };
    for (task_id, project_id, workflow, acceptor_id) in rows {
        let Ok(Some(project)) = ProjectRepository::get(store, project_id).await else {
            continue;
        };
        match merge_requests
            .for_task_review(&project, task_id, TaskViewer::system())
            .await
        {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(%error, %task_id, "cannot read task merge request");
                continue;
            }
        }
        let Some(source_branch) = design_space_branch(&workflow, task_id) else {
            continue;
        };
        let title = format!("Design space change: {}", project.name);
        let description = format!("{workflow} 任务的设计空间修改，等待人类审批合入。");
        if let Err(error) = merge_requests
            .create_for_task(
                &project,
                task_id,
                acceptor_id,
                CreateDesignSpaceMergeRequest {
                    title,
                    description,
                    source_branch,
                    target_branch: project.design_space_branch.clone(),
                },
            )
            .await
        {
            tracing::warn!(
                %error,
                %task_id,
                workflow,
                "cannot create missing design-space merge request"
            );
        }
    }
}

/// Advances task workflows and reconciles durable state into agent
/// activations. Notifications are not persisted; this loop rebuilds missed
/// ones from task state, assignments and Issue comments.
async fn run_task_dispatcher(
    dispatcher: Arc<dyn TaskDispatcher>,
    store: Arc<PostgresStore>,
    merge_requests: Arc<MergeRequestService>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        ensure_design_space_merge_requests(store.as_ref(), merge_requests.as_ref()).await;
        match store.pending_auto_design_merges().await {
            Ok(rows) => {
                for (project_id, merge_request_id, owner_id) in rows {
                    let Ok(Some(project)) =
                        ProjectRepository::get(store.as_ref(), project_id).await
                    else {
                        continue;
                    };
                    if let Err(error) = merge_requests
                        .approve(&project, merge_request_id, owner_id)
                        .await
                    {
                        tracing::debug!(
                            %error,
                            merge_request_id = %merge_request_id,
                            "auto merge approval deferred"
                        );
                    }
                    if let Err(error) = merge_requests
                        .merge(&project, merge_request_id, owner_id)
                        .await
                    {
                        tracing::debug!(
                            %error,
                            merge_request_id = %merge_request_id,
                            "auto merge deferred"
                        );
                    }
                }
            }
            Err(error) => tracing::warn!(%error, "auto design merge scan failed"),
        }
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
