//! One managed agent process.
//!
//! The control-plane supervisor starts this binary with `FOUNDRY_AGENT_ID` and
//! a freshly minted `FOUNDRY_TOKEN`. The process loads its definition (model,
//! prompt, tool whitelist) from Postgres, claims only runs addressed to that
//! definition, and records process heartbeat / current run in
//! `agent_processes`.

use async_trait::async_trait;
use clap::Parser;
use foundry_adapter_postgres::{
    AssignedTask, PostgresStore, listen_issue_changes, listen_task_changes,
};
use foundry_agent_runtime::{
    AgentCreateForRoleTool, AgentRunRequest, AgentRuntime, AgentTemplatesListTool, AgentsListTool,
    ArtifactLogsTool, ArtifactStartTool, ArtifactStatusTool, ArtifactStopTool, BrowserTool,
    DesignPoolBatchTool, DesktopInputTool, DesktopScreenshotTool, DeterministicMode,
    DevelopmentPoolBatchTool, EditFileTool, FoundryClient, IssueCommentTool, IssueCreateTool,
    IssueGetTool, IssueListTool, IssueTransitionTool, IssueTriageBatchTool, IssueTriageResultTool,
    MergeRequestTool, ProjectTool, ReadFileTool, Shell, ShellOutput, ShellTool, TaskAddLinkTool,
    TaskAllowedOperationsTool, TaskAssignAgentTool, TaskBindActorTool, TaskChildrenTool,
    TaskCommentTool, TaskCreateFollowUpTool, TaskGetTool, TaskHistoryTool, TaskLinksListTool,
    TaskOperationTool, TaskRemoveLinkTool, TaskSpawnChildrenTool, TaskStartTool,
    TasksReadyListTool, ToolError, ToolRegistry, TriggerCreateTool, TriggerListTool, WriteFileTool,
    compose_system_prompt,
};
use foundry_domain::{AgentDefinition, CapsuleProfile};
use graph_agent::{
    LlmMessage, LlmParams, LlmRequest, ModelGateway, ModelProfileRef, OpenAiCompatibleConfig,
    OpenAiCompatibleModel,
};
use linux_capsule::{Input as CapsuleInput, LinuxCapsule};

mod services;
use serde_json::{Value, json};
use services::{
    ServiceCapsules, ServiceLogsTool, ServiceStartTool, ServiceStatusTool, ServiceStopTool,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long)]
    database_url: String,
    #[arg(long)]
    foundry_url: String,
    #[arg(long)]
    foundry_token: String,
    #[arg(long)]
    agent_id: Uuid,
    #[arg(long)]
    shell_image: String,
    #[arg(long)]
    desktop_image: String,
    #[arg(long)]
    api_key: String,
}

struct Config {
    database_url: String,
    foundry_url: String,
    foundry_token: String,
    agent_id: Uuid,
    shell_image: String,
    desktop_image: String,
    api_key: String,
}

impl Config {
    fn from_args() -> Self {
        let cli = Cli::parse();
        Self {
            database_url: cli.database_url,
            foundry_url: cli.foundry_url,
            foundry_token: cli.foundry_token,
            agent_id: cli.agent_id,
            shell_image: cli.shell_image,
            desktop_image: cli.desktop_image,
            api_key: cli.api_key,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_agent_tracing(
        "foundry_agentd=info,foundry_agent_runtime=info,graph_agent=info",
    );
    #[cfg(target_os = "linux")]
    {
        // If the control plane exits without running Drop (Ctrl-C, SIGTERM,
        // crash), the kernel terminates this agentd as well. Check the parent
        // after installing the hook to close the race where it already died.
        unsafe {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0);
        }
        if unsafe { libc::getppid() } == 1 {
            return Ok(());
        }
    }
    let config = Config::from_args();
    let store = Arc::new(PostgresStore::connect(&config.database_url).await?);

    let definition = store
        .agent_definition(config.agent_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("agent definition {} not found", config.agent_id))?;
    if !definition.enabled {
        anyhow::bail!("agent definition {} is disabled", definition.name);
    }
    let model = Arc::new(OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
        definition.model_base_url.clone(),
        config.api_key.clone(),
        definition.model_name.clone(),
    ))?);
    let _ = store.agent_process_idle(config.agent_id).await;

    let heartbeat_store = Arc::clone(&store);
    let heartbeat_id = config.agent_id;
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        loop {
            interval.tick().await;
            let _ = heartbeat_store.agent_process_heartbeat(heartbeat_id).await;
        }
    });

    tracing::info!(
        agent = %definition.name,
        role = %definition.role,
        shell = definition.shell,
        "agentd started"
    );

    let capsule_profile = if definition.role == "test_execution:executor" {
        store
            .project_test_capsule(definition.project_id)
            .await
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or_default()
    } else {
        CapsuleProfile::Cli
    };
    let shell = if definition.shell {
        let shell = create_shell(&config, capsule_profile).await?;
        install_arche_credential(&config, &definition, &shell).await?;
        Some(shell)
    } else {
        None
    };

    if definition.role == "dispatcher" {
        run_dispatcher_watcher(Arc::clone(&store), &config, &definition, model, shell).await?;
        return Ok(());
    }

    run_agent_watcher(Arc::clone(&store), &config, &definition, model, shell).await?;
    Ok(())
}

async fn create_shell(
    config: &Config,
    capsule_profile: CapsuleProfile,
) -> anyhow::Result<Arc<dyn Shell>> {
    let workspace = foundry_shared::default_agent_workspace_dir(&config.agent_id.to_string());
    foundry_shared::ensure_private_dir(&workspace).map_err(|error| {
        anyhow::anyhow!(
            "cannot create agent workspace {}: {error}",
            workspace.display()
        )
    })?;
    let labels = vec![("foundry.agent_id".to_string(), config.agent_id.to_string())];
    let capsule = if capsule_profile == CapsuleProfile::Desktop {
        LinuxCapsule::desktop_with_image_workspace_and_labels(
            config.desktop_image.clone(),
            workspace,
            labels,
        )
        .await?
    } else {
        LinuxCapsule::headless_with_image_workspace_and_labels(
            config.shell_image.clone(),
            workspace,
            labels,
        )
        .await?
    };
    tracing::info!(
        agent_id = %config.agent_id,
        capsule = capsule_profile.as_str(),
        "capsule created"
    );
    Ok(Arc::new(CapsuleShell { capsule }))
}

const RUNTIME_DIR: &str = "/run/foundry";
const ARCHE_KEY_PATH: &str = "/run/foundry/arche_key";
const ARCHE_KNOWN_HOSTS_PATH: &str = "/run/foundry/arche_known_hosts";
const GIT_CREDENTIALS_PATH: &str = "/run/foundry/git-credentials";

/// Install the account's Arche SSH key once, for the shell's lifetime.
/// The key is the agent's, not the task's, so task cleanup leaves it alone.
async fn install_arche_credential(
    config: &Config,
    definition: &AgentDefinition,
    shell: &Arc<dyn Shell>,
) -> anyhow::Result<()> {
    let client = FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        definition.project_id.to_string(),
    );
    let credential = client
        .agent_arche_credential(definition.id)
        .await
        .map_err(|error| anyhow::anyhow!("cannot fetch Arche credential: {error}"))?;
    let private_key = credential
        .get("private_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Arche credential has no private key"))?;
    let known_hosts = credential
        .get("known_hosts")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Arche credential has no known_hosts"))?;
    let url = credential
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    shell_ok(
        shell,
        format!("mkdir -p {RUNTIME_DIR} && chmod 700 {RUNTIME_DIR}"),
    )
    .await?;
    upload_text(
        shell,
        ARCHE_KEY_PATH,
        &format!("{}\n", private_key.trim_end()),
    )
    .await?;
    upload_text(shell, ARCHE_KNOWN_HOSTS_PATH, known_hosts).await?;
    shell_ok(
        shell,
        format!(
            "chmod 600 {} {}",
            shell_quote(ARCHE_KEY_PATH),
            shell_quote(ARCHE_KNOWN_HOSTS_PATH)
        ),
    )
    .await?;

    if let Some((host, user, port)) = url.and_then(ssh_target) {
        let mut block = format!(
            "# >>> foundry-arche\nMatch host {host} user {user}\n  IdentityFile {key}\n  UserKnownHostsFile {known}\n  IdentitiesOnly yes\n  StrictHostKeyChecking yes\n",
            key = ARCHE_KEY_PATH,
            known = ARCHE_KNOWN_HOSTS_PATH,
        );
        if port != 22 {
            block.push_str(&format!("  Port {port}\n"));
        }
        block.push_str("# <<< foundry-arche\n");
        shell_ok(
            shell,
            format!(
                "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/config && chmod 600 ~/.ssh/config && \
                 sed -i '/^# >>> foundry-arche$/,/^# <<< foundry-arche$/d' ~/.ssh/config && \
                 printf '%s' {} >> ~/.ssh/config",
                shell_quote(&block)
            ),
        )
        .await?;
    }

    // A project's URL can change when the deployment moves its SSH endpoint,
    // so refresh the workspace remote from the credential.
    if let Some(url) = url {
        let workspace = format!("/work/{}/arche", definition.project_id);
        let config = format!("{workspace}/.arche/config");
        shell_ok(
            shell,
            format!(
                "if [ -f {} ] && command -v arche >/dev/null 2>&1; then \
                 (cd {} && arche remote add {}); fi",
                shell_quote(&config),
                shell_quote(&workspace),
                shell_quote(url)
            ),
        )
        .await?;
    }
    Ok(())
}

fn ssh_target(url: &str) -> Option<(String, String, u16)> {
    let rest = url.strip_prefix("ssh://")?;
    let (authority, _) = rest.split_once('/')?;
    let (user, host_port) = authority
        .rsplit_once('@')
        .unwrap_or(("foundry-arche", authority));
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, port.parse().ok()?),
        None => (host_port, 22),
    };
    Some((host.to_owned(), user.to_owned(), port))
}

const SHELL_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

struct CapsuleShell {
    capsule: LinuxCapsule,
}

#[async_trait]
impl Shell for CapsuleShell {
    async fn upload(&self, local: &Path, container_path: &str) -> Result<(), ToolError> {
        self.capsule
            .upload(local, container_path)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))
    }

    async fn download(&self, container_path: &str, local: &Path) -> Result<(), ToolError> {
        self.capsule
            .download(container_path, local)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))
    }

    async fn screenshot(&self) -> Result<Vec<u8>, ToolError> {
        self.capsule
            .screenshot()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))
    }

    async fn input(&self, input: foundry_agent_runtime::ShellInput) -> Result<(), ToolError> {
        let input = match input {
            foundry_agent_runtime::ShellInput::Move { x, y } => CapsuleInput::Move { x, y },
            foundry_agent_runtime::ShellInput::Click { x, y, button } => {
                CapsuleInput::Click { x, y, button }
            }
            foundry_agent_runtime::ShellInput::Scroll { x, y, delta } => {
                CapsuleInput::Scroll { x, y, delta }
            }
            foundry_agent_runtime::ShellInput::Key { key } => CapsuleInput::Key { key },
            foundry_agent_runtime::ShellInput::Text { text } => CapsuleInput::Text { text },
        };
        self.capsule
            .input(input)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))
    }

    async fn exec(&self, command: &str) -> Result<ShellOutput, ToolError> {
        tracing::info!(
            target: "foundry_agentd::shell",
            command = %truncate_log_line(&redact_credentials(command)),
            "shell command started"
        );
        let last_output = Arc::new(AtomicU64::new(now_millis()));
        let activity = Arc::clone(&last_output);
        let future = self.capsule.exec_streaming(command, move |_stream, line| {
            activity.store(now_millis(), Ordering::Relaxed);
            if !line.is_empty() {
                tracing::info!(target: "foundry_agentd::shell", "{}", truncate_log_line(line));
            }
        });
        tokio::pin!(future);
        let output = loop {
            let idle = now_millis().saturating_sub(last_output.load(Ordering::Relaxed));
            let remaining = if idle >= SHELL_IDLE_TIMEOUT.as_millis() as u64 {
                Duration::ZERO
            } else {
                SHELL_IDLE_TIMEOUT - Duration::from_millis(idle)
            };
            if remaining.is_zero() {
                tracing::warn!(
                    target: "foundry_agentd::shell",
                    "shell command idle for 5 minutes; terminating"
                );
                return Err(ToolError::Failed(
                    "shell command timed out after 5 minutes without stdout/stderr".to_owned(),
                ));
            }
            match tokio::time::timeout(remaining, &mut future).await {
                Ok(result) => {
                    break result.map_err(|error| ToolError::Failed(error.to_string()))?;
                }
                Err(_) => {
                    let idle = now_millis().saturating_sub(last_output.load(Ordering::Relaxed));
                    if idle >= SHELL_IDLE_TIMEOUT.as_millis() as u64 {
                        tracing::warn!(
                            target: "foundry_agentd::shell",
                            "shell command idle for 5 minutes; terminating"
                        );
                        return Err(ToolError::Failed(
                            "shell command timed out after 5 minutes without stdout/stderr"
                                .to_owned(),
                        ));
                    }
                }
            }
        };
        tracing::info!(
            target: "foundry_agentd::shell",
            exit_code = ?output.status,
            stdout_bytes = output.stdout.len(),
            stderr_bytes = output.stderr.len(),
            "shell command finished"
        );
        Ok(ShellOutput {
            exit_code: output.status,
            stdout: output.stdout_string(),
            stderr: output.stderr_string(),
        })
    }
}

/// Agent mode: no durable queue. On startup, reconnect and every transient
/// task/Issue notification, inspect the tasks where this definition is an
/// active actor and decide what to do.
async fn run_agent_watcher(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    shell: Option<Arc<dyn Shell>>,
) -> anyhow::Result<()> {
    let mut handled: HashMap<Uuid, TaskFingerprint> = HashMap::new();
    loop {
        let mut notifications = match listen_task_changes(&config.database_url).await {
            Ok(receiver) => receiver,
            Err(error) => {
                tracing::warn!(error = %error, "cannot listen for task changes; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let mut issue_notifications = match listen_issue_changes(&config.database_url).await {
            Ok(receiver) => receiver,
            Err(error) => {
                tracing::warn!(error = %error, "cannot listen for issue changes; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        loop {
            scan_assigned_tasks(
                Arc::clone(&store),
                config,
                definition,
                Arc::clone(&model),
                shell.clone(),
                &mut handled,
                false,
            )
            .await;
            tokio::select! {
                received = notifications.recv() => {
                    if received.is_none() {
                        tracing::warn!("task change listener closed; reconnecting");
                        break;
                    }
                }
                received = issue_notifications.recv() => {
                    if received.is_none() {
                        tracing::warn!("issue change listener closed; reconnecting");
                        break;
                    }
                    scan_assigned_tasks(
                        Arc::clone(&store),
                        config,
                        definition,
                        Arc::clone(&model),
                        shell.clone(),
                        &mut handled,
                        true,
                    )
                    .await;
                }
                _ = shutdown_requested() => return Ok(()),
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
            // One scan sees every current assignment, so merge a burst of
            // notifications instead of scanning once per event.
            while notifications.try_recv().is_ok() {}
            while issue_notifications.try_recv().is_ok() {}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TaskFingerprint {
    stage: Option<String>,
    state: String,
    updated_at: i128,
    last_human_comment_id: Option<Uuid>,
}

async fn scan_assigned_tasks(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    shell: Option<Arc<dyn Shell>>,
    handled: &mut HashMap<Uuid, TaskFingerprint>,
    wake_triage: bool,
) {
    let tasks = match store.assigned_active_tasks(definition.id).await {
        Ok(tasks) => tasks,
        Err(error) => {
            tracing::warn!(error = %error, "cannot list assigned tasks");
            return;
        }
    };
    for task in tasks {
        let fingerprint = TaskFingerprint {
            stage: task.stage.clone(),
            state: task.state.clone(),
            updated_at: task.updated_at.unix_timestamp_nanos(),
            last_human_comment_id: task.last_human_comment_id,
        };
        if !(wake_triage && task.workflow == "triage")
            && handled.get(&task.task_id) == Some(&fingerprint)
        {
            continue;
        }
        handled.insert(task.task_id, fingerprint);
        let _ = store.agent_process_busy(config.agent_id).await;
        if let Err(error) =
            execute_task(config, definition, Arc::clone(&model), shell.clone(), &task).await
        {
            tracing::warn!(task_id = %task.task_id, error = %error, "agent task execution failed");
        }
        let _ = store.agent_process_idle(config.agent_id).await;
    }
}

/// Dispatcher mode: sweep the task pool on startup and after every task change.
///
/// Assignment is deterministic: agent roles are matched by their
/// `workflow:role` key, human roles use the single eligible candidate. The
/// dispatcher does not call a model.
async fn run_dispatcher_watcher(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    _shell: Option<Arc<dyn Shell>>,
) -> anyhow::Result<()> {
    let client = Arc::new(FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        definition.project_id.to_string(),
    ));

    loop {
        let mut notifications = match listen_task_changes(&config.database_url).await {
            Ok(receiver) => receiver,
            Err(error) => {
                tracing::warn!(error = %error, "cannot listen for task changes; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        loop {
            let _ = store.agent_process_busy(config.agent_id).await;
            tracing::info!("dispatcher sweep started");
            match dispatch_once(&client, model.as_ref(), definition.project_id).await {
                Ok(()) => tracing::info!("dispatcher sweep finished"),
                Err(error) => tracing::warn!(error = %error, "dispatcher sweep failed"),
            }
            let _ = store.agent_process_idle(config.agent_id).await;

            tokio::select! {
                received = notifications.recv() => {
                    if received.is_none() {
                        tracing::warn!("task change listener closed; reconnecting");
                        break;
                    }
                }
                _ = shutdown_requested() => return Ok(()),
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
            // A sweep reads the whole ready pool, so intermediate signals add
            // nothing. Collapse every notification that arrived during the
            // debounce window into one sweep instead of one per event.
            while notifications.try_recv().is_ok() {}
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Assign every unbound role of every ready task, then start the tasks whose
/// roles are all bound. Missing agents/templates or human candidates leave the
/// task pending.
async fn dispatch_once(
    client: &FoundryClient,
    model: &dyn ModelGateway,
    project_id: Uuid,
) -> anyhow::Result<()> {
    let tasks = client.ready_tasks(Some(project_id)).await?;
    let Some(tasks) = tasks.as_array() else {
        return Ok(());
    };
    if tasks.is_empty() {
        return Ok(());
    }
    let agents = client.dispatch_agents(Some(project_id)).await?;
    let members = match client.get_project().await {
        Ok(project) => project
            .get("members")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        Err(error) => {
            tracing::warn!(error = %error, "cannot load project members for dispatch");
            Vec::new()
        }
    };
    let mut assigned_agents: HashMap<String, Uuid> = HashMap::new();
    if let Some(agents) = agents.as_array() {
        for agent in agents {
            let enabled = agent.get("enabled").and_then(Value::as_bool) == Some(true);
            let role = agent.get("role").and_then(Value::as_str);
            let id = agent
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Uuid>().ok());
            if let (true, Some(role), Some(id)) = (enabled, role, id) {
                assigned_agents.entry(role.to_string()).or_insert(id);
            }
        }
    }

    for task in tasks {
        let Some(task_id) = task
            .get("task_id")
            .and_then(Value::as_str)
            .and_then(|id| id.parse::<Uuid>().ok())
        else {
            continue;
        };
        let title = task.get("title").and_then(Value::as_str).unwrap_or("");
        let mut complete = true;
        let roles = task
            .get("roles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for role in &roles {
            let name = role.get("name").and_then(Value::as_str).unwrap_or("");
            let kind = role.get("kind").and_then(Value::as_str).unwrap_or("");
            let assignment = role.get("assignment").and_then(Value::as_str).unwrap_or("");
            if name.is_empty() {
                complete = false;
                continue;
            }
            match kind {
                "agent" => {
                    let bound = role
                        .get("agent")
                        .and_then(|agent| agent.get("enabled"))
                        .and_then(Value::as_bool)
                        == Some(true);
                    if bound {
                        continue;
                    }
                    let role_key = format!("{assignment}:{name}");
                    let agent_id = if let Some(agent_id) = assigned_agents.get(&role_key) {
                        *agent_id
                    } else {
                        let created = match create_agent(client, task_id, &role_key).await {
                            Ok(agent_id) => agent_id,
                            Err(error) => {
                                tracing::warn!(
                                    task_id = %task_id,
                                    title = %title,
                                    role = %role_key,
                                    error = %error,
                                    "cannot create the agent required by a role"
                                );
                                complete = false;
                                continue;
                            }
                        };
                        assigned_agents.insert(role_key.clone(), created);
                        created
                    };
                    if let Err(error) = client.assign_agent(task_id, &role_key, agent_id).await {
                        tracing::warn!(
                            task_id = %task_id,
                            title = %title,
                            role = %role_key,
                            error = %error,
                            "cached agent could not be assigned; creating a fresh one"
                        );
                        // The cached agent may already be busy on another task.
                        // Create a fresh agent for this task and retry once.
                        match create_agent(client, task_id, &role_key).await {
                            Ok(created) => {
                                assigned_agents.insert(role_key.clone(), created);
                                if let Err(error) =
                                    client.assign_agent(task_id, &role_key, created).await
                                {
                                    tracing::warn!(
                                        task_id = %task_id,
                                        title = %title,
                                        role = %role_key,
                                        error = %error,
                                        "cannot assign a fresh agent for a role"
                                    );
                                    complete = false;
                                }
                            }
                            Err(error) => {
                                tracing::warn!(
                                    task_id = %task_id,
                                    title = %title,
                                    role = %role_key,
                                    error = %error,
                                    "cannot create a fresh agent for a busy role"
                                );
                                complete = false;
                            }
                        }
                    }
                }
                "human" => {
                    if role.get("actor").is_some_and(|actor| !actor.is_null()) {
                        continue;
                    }
                    let candidates = role
                        .get("candidates")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    match candidates.as_slice() {
                        [candidate] => {
                            let Some(account_id) =
                                candidate.as_str().and_then(|id| id.parse::<Uuid>().ok())
                            else {
                                complete = false;
                                continue;
                            };
                            if let Err(error) =
                                client.bind_task_actor(task_id, name, account_id).await
                            {
                                tracing::warn!(
                                    task_id = %task_id,
                                    title = %title,
                                    role = %name,
                                    error = %error,
                                    "cannot bind the human required by a role"
                                );
                                complete = false;
                            }
                        }
                        [] => {
                            tracing::warn!(
                                task_id = %task_id,
                                title = %title,
                                role = %name,
                                identity = %assignment,
                                "no human holds the identity required by a role"
                            );
                            complete = false;
                        }
                        _ => {
                            let candidates = candidates
                                .iter()
                                .filter_map(Value::as_str)
                                .filter_map(|id| id.parse::<Uuid>().ok())
                                .collect::<Vec<_>>();
                            match choose_human(model, task, role, &candidates, &members).await {
                                Some(account_id) => {
                                    if let Err(error) =
                                        client.bind_task_actor(task_id, name, account_id).await
                                    {
                                        tracing::warn!(
                                            task_id = %task_id,
                                            title = %title,
                                            role = %name,
                                            error = %error,
                                            "cannot bind the human chosen by the model"
                                        );
                                        complete = false;
                                    }
                                }
                                None => {
                                    tracing::warn!(
                                        task_id = %task_id,
                                        title = %title,
                                        role = %name,
                                        identity = %assignment,
                                        "the model could not choose among human candidates"
                                    );
                                    complete = false;
                                }
                            }
                        }
                    }
                }
                _ => complete = false,
            }
        }
        if complete && let Err(error) = client.start_task(task_id).await {
            tracing::warn!(
                task_id = %task_id,
                title = %title,
                error = %error,
                "cannot start a dispatched task"
            );
        }
    }
    Ok(())
}

async fn create_agent(client: &FoundryClient, task_id: Uuid, role: &str) -> anyhow::Result<Uuid> {
    let templates = client.dispatch_templates(role).await?;
    let template = templates
        .as_array()
        .and_then(|templates| templates.first())
        .and_then(|template| template.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("no agent template for role {role}"))?;
    let created = client
        .create_agent_from_template(task_id, role, template)
        .await?;
    created
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Uuid>().ok())
        .ok_or_else(|| anyhow::anyhow!("created agent has no id"))
}

/// Ask the model to choose one human when a role has several candidates.
/// The model only sees the task and the candidate list; it cannot invent a
/// different account.
async fn choose_human(
    model: &dyn ModelGateway,
    task: &Value,
    role: &Value,
    candidates: &[Uuid],
    members: &[Value],
) -> Option<Uuid> {
    let mut labels = Vec::new();
    for candidate in candidates {
        let id = candidate.to_string();
        let member = members
            .iter()
            .find(|member| member.get("account_id").and_then(Value::as_str) == Some(id.as_str()));
        let label = member
            .map(|member| {
                format!(
                    "{} ({}, 项目角色 {})",
                    member
                        .get("display_name")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    member.get("handle").and_then(Value::as_str).unwrap_or(""),
                    member.get("role").and_then(Value::as_str).unwrap_or("")
                )
            })
            .unwrap_or_else(|| id.clone());
        labels.push(format!("- {id}: {label}"));
    }
    let title = task.get("title").and_then(Value::as_str).unwrap_or("");
    let description = task
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let input = task.get("input").cloned().unwrap_or(Value::Null);
    let role_name = role.get("name").and_then(Value::as_str).unwrap_or("");
    let identity = role.get("assignment").and_then(Value::as_str).unwrap_or("");
    let system = "你是任务调度器。根据任务要求、任务描述和用户输入，在候选者中选择最合适的人。\
                  只能返回候选者列表中的一个 UUID，不要解释，不要返回其他内容。";
    let user = format!(
        "任务：{title}\n描述：{description}\n输入：{input}\n角色：{role_name}\n身份要求：{identity}\n候选者：\n{}\n\
         只返回一个候选者 UUID。",
        labels.join("\n")
    );
    let request = LlmRequest {
        model: ModelProfileRef::new(""),
        params: LlmParams::default(),
        messages: vec![LlmMessage::system(system), LlmMessage::user(user)],
        tools: Vec::new(),
    };
    let response = model.complete(request).await.ok()?;
    let chosen = response.message.content.trim().parse::<Uuid>().ok()?;
    candidates.contains(&chosen).then_some(chosen)
}

/// Fetch the task and its allowed operations when this agent is one of its
/// active actors. The agent should not have to discover its own context with
/// an extra tool call before deciding what to do.
async fn task_context(client: &FoundryClient, task_id: Uuid, account_id: Uuid) -> Option<String> {
    let task = match client.get_task(task_id).await {
        Ok(task) => task,
        Err(error) => {
            tracing::warn!(task_id = %task_id, error = %error, "cannot load task context");
            return None;
        }
    };
    let account = account_id.to_string();
    let active = task
        .get("actors")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|actors| {
            actors.iter().any(|actor| {
                actor.get("account_id").and_then(serde_json::Value::as_str)
                    == Some(account.as_str())
                    && actor.get("active").and_then(serde_json::Value::as_bool) == Some(true)
            })
        });
    if !active {
        return None;
    }
    let details = json!({
        "id": task.get("id"),
        "title": task.get("title"),
        "workflow": task.get("workflow"),
        "workflow_version": task.get("workflow_version"),
        "state": task.get("state"),
        "stage": task.get("stage"),
        "result": task.get("result"),
        "input": task.get("input"),
        "role_specs": task.get("role_specs"),
        "actors": task.get("actors"),
        "allowed_operations": task.get("allowed_operations"),
    });
    serde_json::to_string_pretty(&details).ok()
}

async fn execute_task(
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    shell: Option<Arc<dyn Shell>>,
    task: &AssignedTask,
) -> anyhow::Result<()> {
    tracing::info!(
        project_id = %task.project_id,
        task_id = %task.task_id,
        stage = ?task.stage,
        state = %task.state,
        "agent task started"
    );
    let client = Arc::new(FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        task.project_id.to_string(),
    ));
    let workspace = foundry_shared::default_agent_workspace_dir(&config.agent_id.to_string());
    let services = ServiceCapsules::new(
        config.agent_id.to_string(),
        config.shell_image.clone(),
        workspace,
    );
    let tools = build_tools(definition, &client, shell.clone(), services)?;
    let runtime = build_runtime(definition, model, tools, shell.clone())?;

    let mut credential_refresh: Option<GitCredentialRefresher> = None;
    if definition.shell {
        let Some(shell) = shell.as_ref() else {
            if definition.role.ends_with(":summarizer") {
                finish_codesummary(&client, task, false).await;
            }
            anyhow::bail!("shell agent has no shell");
        };
        match client.agent_credentials(task.task_id).await {
            Ok(credentials) => {
                let has_git = credentials.get("git").is_some_and(|value| !value.is_null());
                if let Err(error) = inject_task_credentials(shell, &credentials).await {
                    tracing::warn!(
                        task_id = %task.task_id,
                        error = %error,
                        "cannot inject task credentials"
                    );
                    if definition.role.ends_with(":summarizer") {
                        finish_codesummary(&client, task, false).await;
                    }
                    return Err(error);
                }
                credential_refresh = start_git_credential_refresh(
                    Arc::clone(&client),
                    Arc::clone(shell),
                    task.task_id,
                    has_git,
                );
            }
            Err(error) => {
                tracing::warn!(
                    task_id = %task.task_id,
                    error = %error,
                    "cannot fetch task credentials"
                );
                if definition.role.ends_with(":summarizer") {
                    finish_codesummary(&client, task, false).await;
                }
                if definition.role == "design_baseline:architect" {
                    finish_design_baseline(&client, task, false).await;
                } else if definition.role.starts_with("design_baseline:") {
                    finish_design_baseline_review(&client, task, definition, false).await;
                }
                return Err(anyhow::anyhow!("cannot fetch task credentials: {error}"));
            }
        }
    }

    let task_id = task.task_id.to_string();
    let system_prompt = system_prompt_for(definition).map_err(|error| anyhow::anyhow!(error))?;
    let mut request = AgentRunRequest::new(
        definition.name.clone(),
        task_id.clone(),
        system_prompt,
        "Inspect the task and decide what to do.",
    )
    .with_project_id(task.project_id.to_string())
    .with_stage(task.stage.clone());
    request.task_input = task.input.clone();
    request.context.push(
        "current_task",
        task_id.clone(),
        format!("task_id = {task_id}"),
    );
    if let Some(details) = task_context(&client, task.task_id, definition.account_id).await {
        request.context.push("task", task_id.clone(), details);
    }

    let succeeded = match runtime.run(request).await {
        Ok(result) => {
            tracing::info!(
                task_id = %task.task_id,
                final = %truncate(result.final_message.as_deref().unwrap_or(""), 1000),
                "agent task completed"
            );
            true
        }
        Err(error) => {
            tracing::warn!(task_id = %task.task_id, error = %error, "agent task failed");
            false
        }
    };

    drop(credential_refresh);
    if definition.shell {
        clear_task_credentials(shell.as_ref()).await;
    }
    if definition.role.ends_with(":summarizer")
        && task.workflow.as_str() == "code_summary_group"
        && task.stage.as_deref() == Some("summary")
    {
        finish_codesummary(&client, task, succeeded).await;
    }
    if definition.role == "design_baseline:architect" {
        finish_design_baseline(&client, task, succeeded).await;
    } else if definition.role.starts_with("design_baseline:") {
        finish_design_baseline_review(&client, task, definition, succeeded).await;
    }
    Ok(())
}

async fn finish_codesummary(client: &FoundryClient, task: &AssignedTask, succeeded: bool) {
    let operation = match task.workflow.as_str() {
        "code_summary_group" => {
            if succeeded {
                "submitted"
            } else {
                "failed"
            }
        }
        _ => return,
    };
    let stage = task.stage.as_deref().unwrap_or("summary");
    let key = Uuid::now_v7().to_string();
    match client
        .send_task_operation(task.task_id, &format!("{stage}:{operation}"), &key)
        .await
    {
        Ok(_) => tracing::info!(
            task_id = %task.task_id,
            operation = %format!("{stage}:{operation}"),
            "code summary task finished"
        ),
        Err(error) => tracing::warn!(
            task_id = %task.task_id,
            error = %error,
            "cannot finish code summary task"
        ),
    }
}

async fn finish_design_baseline(client: &FoundryClient, task: &AssignedTask, succeeded: bool) {
    if task.workflow.as_str() != "design_baseline" {
        return;
    }
    let Some(stage) = task.stage.as_deref() else {
        return;
    };
    let operation = match (stage, succeeded) {
        ("baseline", true) => "baselined",
        ("fix", true) => "fixed",
        ("baseline" | "fix", false) => "failed",
        _ => return,
    };
    let key = format!(
        "design-baseline-finish:{}:{}:{}",
        task.task_id,
        stage,
        task.updated_at.unix_timestamp_nanos()
    );
    match client
        .send_task_operation(task.task_id, &format!("{stage}:{operation}"), &key)
        .await
    {
        Ok(_) => tracing::info!(
            task_id = %task.task_id,
            operation = %format!("{stage}:{operation}"),
            "design baseline task finished"
        ),
        Err(error) => tracing::warn!(
            task_id = %task.task_id,
            error = %error,
            "cannot finish design baseline task"
        ),
    }
}

async fn finish_design_baseline_review(
    client: &FoundryClient,
    task: &AssignedTask,
    definition: &AgentDefinition,
    succeeded: bool,
) {
    if succeeded
        || task.workflow.as_str() != "design_baseline"
        || !definition.role.starts_with("design_baseline:")
    {
        return;
    }
    let Some(stage) = task.stage.as_deref() else {
        return;
    };
    let key = format!(
        "design-baseline-review-failed:{}:{}:{}",
        task.task_id,
        stage,
        task.updated_at.unix_timestamp_nanos()
    );
    match client
        .send_task_operation(task.task_id, &format!("{stage}:failed"), &key)
        .await
    {
        Ok(_) => tracing::info!(
            task_id = %task.task_id,
            operation = %format!("{stage}:failed"),
            "design baseline review reported failure"
        ),
        Err(error) => tracing::warn!(
            task_id = %task.task_id,
            error = %error,
            "cannot report design baseline review failure"
        ),
    }
}

fn redact_credentials(command: &str) -> String {
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    while let Some(index) = rest.find("://") {
        let (before, after) = rest.split_at(index + 3);
        out.push_str(before);
        let stop = after.find(['@', '/', '"', ' ', '\n']);
        match stop {
            Some(at) if after.as_bytes()[at] == b'@' => {
                out.push_str("***@");
                rest = &after[at + 1..];
            }
            _ => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn truncate_log_line(line: &str) -> String {
    const MAX_CHARS: usize = 4096;
    if line.chars().count() <= MAX_CHARS {
        return line.to_owned();
    }
    let mut truncated: String = line.chars().take(MAX_CHARS).collect();
    truncated.push('…');
    truncated
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

async fn run_shell(shell: &Arc<dyn Shell>, command: String) -> Result<ShellOutput, ToolError> {
    shell.exec(&command).await
}

async fn shell_ok(shell: &Arc<dyn Shell>, command: String) -> Result<(), ToolError> {
    let output = run_shell(shell, command).await?;
    if output.exit_code != Some(0) {
        return Err(ToolError::Failed(output.stderr));
    }
    Ok(())
}

async fn upload_text(
    shell: &Arc<dyn Shell>,
    container_path: &str,
    text: &str,
) -> Result<(), ToolError> {
    let local = std::env::temp_dir().join(format!("foundry-cred-{}", Uuid::now_v7()));
    tokio::fs::write(&local, text.as_bytes())
        .await
        .map_err(|error| ToolError::Failed(error.to_string()))?;
    let result = shell.upload(&local, container_path).await;
    let _ = tokio::fs::remove_file(&local).await;
    result
}

async fn inject_task_credentials(
    shell: &Arc<dyn Shell>,
    credentials: &Value,
) -> anyhow::Result<()> {
    // A crashed task may have left the previous task's credential behind.
    shell_ok(
        shell,
        format!(
            "mkdir -p {RUNTIME_DIR} && chmod 700 {RUNTIME_DIR} && \
             rm -f {}",
            shell_quote(GIT_CREDENTIALS_PATH)
        ),
    )
    .await?;

    if let Some(git) = credentials.get("git").filter(|value| !value.is_null()) {
        write_git_credential(shell, git).await?;
    }
    Ok(())
}

/// Git is preconfigured with `credential.helper = store --file=...` in the
/// shell image; this is the only file the agent needs, and it is rewritten by
/// the refresh loop before the installation token expires.
async fn write_git_credential(shell: &Arc<dyn Shell>, git: &Value) -> anyhow::Result<()> {
    let url = git.get("url").and_then(Value::as_str).unwrap_or_default();
    let username = git
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("x-access-token");
    let token = git.get("token").and_then(Value::as_str).unwrap_or_default();
    if url.is_empty() || token.is_empty() {
        anyhow::bail!("git credential is missing url or token");
    }
    let host = url
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty())
        .ok_or_else(|| anyhow::anyhow!("unsupported git URL: {url}"))?;
    upload_text(
        shell,
        GIT_CREDENTIALS_PATH,
        &format!("https://{username}:{token}@{host}\n"),
    )
    .await?;
    shell_ok(
        shell,
        format!(
            "chmod 700 {RUNTIME_DIR} && chmod 600 {}",
            shell_quote(GIT_CREDENTIALS_PATH)
        ),
    )
    .await?;
    Ok(())
}

struct GitCredentialRefresher {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for GitCredentialRefresher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Refresh the GitHub App installation token before it expires. The agentd
/// session is slid forward by the control plane, so the task can outlive the
/// initial session TTL.
fn start_git_credential_refresh(
    client: Arc<FoundryClient>,
    shell: Arc<dyn Shell>,
    task_id: Uuid,
    has_git: bool,
) -> Option<GitCredentialRefresher> {
    if !has_git {
        return None;
    }
    let handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30 * 60)).await;
            match client.agent_credentials(task_id).await {
                Ok(credentials) => {
                    if let Some(git) = credentials.get("git").filter(|value| !value.is_null()) {
                        if let Err(error) = write_git_credential(&shell, git).await {
                            tracing::warn!(
                                task_id = %task_id,
                                error = %error,
                                "cannot refresh git credential"
                            );
                        }
                    } else {
                        let _ = shell
                            .exec(&format!("rm -f {}", shell_quote(GIT_CREDENTIALS_PATH)))
                            .await;
                    }
                }
                Err(error) => tracing::warn!(
                    task_id = %task_id,
                    error = %error,
                    "cannot refresh git credential"
                ),
            }
        }
    });
    Some(GitCredentialRefresher { handle })
}

/// Remove task-scoped credentials. The Arche key and known-hosts files
/// belong to the agent account and stay for the shell's lifetime.
async fn clear_task_credentials(shell: Option<&Arc<dyn Shell>>) {
    if let Some(shell) = shell {
        let _ = shell
            .exec(&format!("rm -f {}", shell_quote(GIT_CREDENTIALS_PATH)))
            .await;
    }
}

fn system_prompt_for(definition: &AgentDefinition) -> Result<String, String> {
    let identity = format!(
        "你的身份：\n- agent_id: {}\n- account_id: {}\n- handle: {}\n- name: {}\n- role: {}\n\n",
        definition.id,
        definition.account_id,
        definition.account_handle,
        definition.name,
        definition.role
    );
    let base = format!("{identity}{}", definition.system_prompt);
    compose_system_prompt(&base, &definition.skills).map_err(|error| error.to_string())
}

#[cfg(unix)]
async fn shutdown_requested() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_requested() {
    let _ = tokio::signal::ctrl_c().await;
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

fn build_runtime(
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    tools: ToolRegistry,
    shell: Option<Arc<dyn Shell>>,
) -> anyhow::Result<AgentRuntime> {
    if definition.role == "design_baseline:architect" {
        let shell = shell.ok_or_else(|| {
            anyhow::anyhow!(
                "agent {} is a design baseline agent but has no shell",
                definition.name
            )
        })?;
        let state_dir = foundry_shared::default_foundry_dir()
            .join("state")
            .join("design-baseline");
        foundry_shared::ensure_private_dir(&state_dir).map_err(|error| {
            anyhow::anyhow!(
                "cannot create design baseline state directory {}: {error}",
                state_dir.display()
            )
        })?;
        Ok(AgentRuntime::new_design_baseline(
            model,
            Arc::new(tools),
            shell,
            state_dir,
        ))
    } else if definition.role.ends_with(":summarizer") {
        let shell = shell.ok_or_else(|| {
            anyhow::anyhow!(
                "agent {} is a CodeSummary agent but has no shell",
                definition.name
            )
        })?;
        Ok(AgentRuntime::new_codesummary(model, Arc::new(tools), shell))
    } else if let Some(mode) = deterministic_mode(&definition.role) {
        Ok(AgentRuntime::new_deterministic(
            model,
            mode,
            Arc::new(tools),
            shell,
        ))
    } else if let Some(mode) = hybrid_mode(&definition.role) {
        Ok(AgentRuntime::new_hybrid(
            model,
            mode,
            Arc::new(tools),
            shell,
        ))
    } else {
        Ok(AgentRuntime::new(model, Arc::new(tools)))
    }
}

fn deterministic_mode(role: &str) -> Option<DeterministicMode> {
    match role {
        "software_development:assessor" => Some(DeterministicMode::Assessor),
        "code_change:tester" => Some(DeterministicMode::CodeTester),
        "code_change:merger" => Some(DeterministicMode::CodeMerger),
        "code_summary_group:merger" => Some(DeterministicMode::CodeSummaryMerger),
        "design_consistency_maintenance:scanner" => {
            Some(DeterministicMode::DesignConsistencyScanner)
        }
        "design_iteration:dispatcher" => Some(DeterministicMode::DesignDispatcher),
        "design_iteration:verifier" => Some(DeterministicMode::DesignVerifier),
        "development_iteration:dispatcher" => Some(DeterministicMode::DevelopmentDispatcher),
        "development_iteration:verifier" => Some(DeterministicMode::DevelopmentVerifier),
        "release:release_manager" => Some(DeterministicMode::ReleaseManager),
        "release:verifier" => Some(DeterministicMode::ReleaseVerifier),
        "release:deployer" => Some(DeterministicMode::ReleaseDeployer),
        "release:observer" => Some(DeterministicMode::ReleaseObserver),
        _ => None,
    }
}

fn hybrid_mode(role: &str) -> Option<DeterministicMode> {
    match role {
        "design_consistency_maintenance:integrator" => {
            Some(DeterministicMode::DesignConsistencyIntegrator)
        }
        "design:design_reviewer" => Some(DeterministicMode::DesignCleanupDesign),
        "design_change:design_reviewer" => Some(DeterministicMode::DesignCleanupDesignChange),
        "design_correction:design_reviewer" => {
            Some(DeterministicMode::DesignCleanupDesignCorrection)
        }
        "design_baseline:integrator" => Some(DeterministicMode::DesignCleanupDesignBaseline),
        _ => None,
    }
}

fn build_tools(
    definition: &AgentDefinition,
    client: &Arc<FoundryClient>,
    shell: Option<Arc<dyn Shell>>,
    services: Arc<ServiceCapsules>,
) -> anyhow::Result<ToolRegistry> {
    let mut tools = ToolRegistry::new();
    tools.register(Arc::new(ProjectTool::new(Arc::clone(client))));
    if definition.role == "dispatcher" {
        tools.register(Arc::new(TasksReadyListTool::new(Arc::clone(client))));
        tools.register(Arc::new(AgentsListTool::new(Arc::clone(client))));
        tools.register(Arc::new(AgentTemplatesListTool::new(Arc::clone(client))));
        tools.register(Arc::new(AgentCreateForRoleTool::new(Arc::clone(client))));
        tools.register(Arc::new(TaskAssignAgentTool::new(Arc::clone(client))));
        tools.register(Arc::new(TaskStartTool::new(Arc::clone(client))));
        tools.register(Arc::new(TaskGetTool::new(Arc::clone(client))));
        tools.register(Arc::new(TaskBindActorTool::new(Arc::clone(client))));
    }
    for tool in &definition.tools {
        match tool.as_str() {
            "task_get" => {
                tools.register(Arc::new(TaskGetTool::new(Arc::clone(client))));
            }
            "task_allowed_operations" => {
                tools.register(Arc::new(TaskAllowedOperationsTool::new(Arc::clone(client))));
            }
            "task_operation" => {
                tools.register(Arc::new(TaskOperationTool::new(Arc::clone(client))));
            }
            "task_comment" => {
                tools.register(Arc::new(TaskCommentTool::new(Arc::clone(client))));
            }
            "task_history" => {
                tools.register(Arc::new(TaskHistoryTool::new(Arc::clone(client))));
            }
            "task_children" => {
                tools.register(Arc::new(TaskChildrenTool::new(Arc::clone(client))));
            }
            "task_spawn_children" => {
                tools.register(Arc::new(TaskSpawnChildrenTool::new(Arc::clone(client))));
            }
            "task_create_follow_up" => {
                tools.register(Arc::new(TaskCreateFollowUpTool::new(Arc::clone(client))));
            }
            "task_add_link" => {
                tools.register(Arc::new(TaskAddLinkTool::new(Arc::clone(client))));
            }
            "task_remove_link" => {
                tools.register(Arc::new(TaskRemoveLinkTool::new(Arc::clone(client))));
            }
            "task_links_list" => {
                tools.register(Arc::new(TaskLinksListTool::new(Arc::clone(client))));
            }
            "issue_get" => {
                tools.register(Arc::new(IssueGetTool::new(Arc::clone(client))));
            }
            "issue_comment" => {
                tools.register(Arc::new(IssueCommentTool::new(Arc::clone(client))));
            }
            "issue_create" => {
                tools.register(Arc::new(IssueCreateTool::new(Arc::clone(client))));
            }
            "issue_list" => {
                tools.register(Arc::new(IssueListTool::new(Arc::clone(client))));
            }
            "issue_triage_batch" => {
                tools.register(Arc::new(IssueTriageBatchTool::new(Arc::clone(client))));
            }
            "design_pool_batch" => {
                tools.register(Arc::new(DesignPoolBatchTool::new(Arc::clone(client))));
            }
            "development_pool_batch" => {
                tools.register(Arc::new(DevelopmentPoolBatchTool::new(Arc::clone(client))));
            }
            "issue_transition" => {
                tools.register(Arc::new(IssueTransitionTool::new(Arc::clone(client))));
            }
            "issue_triage_result" => {
                tools.register(Arc::new(IssueTriageResultTool::new(Arc::clone(client))));
            }
            "trigger_create" => {
                tools.register(Arc::new(TriggerCreateTool::new(Arc::clone(client))));
            }
            "trigger_list" => {
                tools.register(Arc::new(TriggerListTool::new(Arc::clone(client))));
            }
            "merge_request" => {
                tools.register(Arc::new(MergeRequestTool::new(Arc::clone(client))));
            }
            "artifact_start" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("artifact_start requires a shell"))?;
                tools.register(Arc::new(ArtifactStartTool::new(shell)));
            }
            "artifact_status" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("artifact_status requires a shell"))?;
                tools.register(Arc::new(ArtifactStatusTool::new(shell)));
            }
            "artifact_logs" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("artifact_logs requires a shell"))?;
                tools.register(Arc::new(ArtifactLogsTool::new(shell)));
            }
            "artifact_stop" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("artifact_stop requires a shell"))?;
                tools.register(Arc::new(ArtifactStopTool::new(shell)));
            }
            "browser" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("browser requires a shell"))?;
                tools.register(Arc::new(BrowserTool::new(shell)));
            }
            "desktop_screenshot" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("desktop_screenshot requires a shell"))?;
                tools.register(Arc::new(DesktopScreenshotTool::new(shell)));
            }
            "desktop_input" => {
                let shell = shell
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("desktop_input requires a shell"))?;
                tools.register(Arc::new(DesktopInputTool::new(shell)));
            }
            "service_start" => {
                tools.register(Arc::new(ServiceStartTool::new(Arc::clone(&services))));
            }
            "service_status" => {
                tools.register(Arc::new(ServiceStatusTool::new(Arc::clone(&services))));
            }
            "service_logs" => {
                tools.register(Arc::new(ServiceLogsTool::new(Arc::clone(&services))));
            }
            "service_stop" => {
                tools.register(Arc::new(ServiceStopTool::new(Arc::clone(&services))));
            }
            _ => {}
        }
    }
    if definition.shell {
        let shell = shell.ok_or_else(|| {
            anyhow::anyhow!(
                "agent {} requires a shell but agentd did not create one",
                definition.name
            )
        })?;
        tools.register(Arc::new(ShellTool::new(Arc::clone(&shell))));
        tools.register(Arc::new(ReadFileTool::new(Arc::clone(&shell))));
        tools.register(Arc::new(WriteFileTool::new(Arc::clone(&shell))));
        tools.register(Arc::new(EditFileTool::new(shell)));
    }
    Ok(tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanical_roles_do_not_use_the_llm_runtime() {
        for role in [
            "software_development:assessor",
            "code_change:tester",
            "code_change:merger",
            "code_summary_group:merger",
            "design_consistency_maintenance:scanner",
            "design_iteration:dispatcher",
            "design_iteration:verifier",
            "development_iteration:dispatcher",
            "development_iteration:verifier",
            "release:release_manager",
            "release:verifier",
            "release:deployer",
            "release:observer",
        ] {
            assert!(
                deterministic_mode(role).is_some(),
                "{role} should use a deterministic runtime"
            );
        }
        for role in [
            "design_consistency_maintenance:integrator",
            "design:design_reviewer",
            "design_change:design_reviewer",
            "design_correction:design_reviewer",
            "design_baseline:integrator",
        ] {
            assert!(
                hybrid_mode(role).is_some(),
                "{role} should use a hybrid runtime"
            );
        }
        for role in [
            "code_change:developer",
            "code_change:code_reviewer",
            "release:approver",
            "triage:triage",
        ] {
            assert!(
                deterministic_mode(role).is_none() && hybrid_mode(role).is_none(),
                "{role} needs judgment and should stay on the LLM runtime"
            );
        }
    }
}
