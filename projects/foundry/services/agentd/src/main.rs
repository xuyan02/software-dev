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
    FoundryClient, IssueCommentTool, IssueCreateTool, IssueGetTool, IssueListTool,
    IssueTransitionTool, IssueTriageBatchTool, IssueTriageResultTool, MergeRequestTool,
    ProjectTool, Shell, ShellOutput, ShellTool, TaskAddLinkTool, TaskAllowedOperationsTool,
    TaskAssignAgentTool, TaskBindActorTool, TaskCommentTool, TaskCreateFollowUpTool, TaskGetTool,
    TaskHistoryTool, TaskLinksListTool, TaskOperationTool, TaskRemoveLinkTool,
    TaskSpawnChildrenTool, TaskStartTool, TasksReadyListTool, ToolError, ToolRegistry,
    TriggerCreateTool, TriggerListTool, compose_system_prompt,
};
use foundry_domain::AgentDefinition;
use graph_agent::{
    LlmMessage, LlmParams, LlmRequest, ModelGateway, ModelProfileRef, OpenAiCompatibleConfig,
    OpenAiCompatibleModel,
};
use linux_capsule::LinuxCapsule;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
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
    api_key: String,
}

struct Config {
    database_url: String,
    foundry_url: String,
    foundry_token: String,
    agent_id: Uuid,
    shell_image: String,
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
            api_key: cli.api_key,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_agent_tracing(
        "foundry_agentd=info,foundry_agent_runtime=info,graph_agent=info",
    );
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

    let shell = if definition.shell {
        Some(create_shell(&config).await?)
    } else {
        None
    };

    if definition.role == "dispatcher" {
        run_dispatcher_watcher(Arc::clone(&store), &config, &definition, model, shell).await?;
        return Ok(());
    }
    if definition.role == "triage_check:checker" {
        run_triage_check_watcher(Arc::clone(&store), &config, &definition).await?;
        return Ok(());
    }

    run_agent_watcher(Arc::clone(&store), &config, &definition, model, shell).await?;
    Ok(())
}

async fn create_shell(config: &Config) -> anyhow::Result<Arc<dyn Shell>> {
    let workspace = foundry_shared::default_agent_workspace_dir(&config.agent_id.to_string());
    foundry_shared::ensure_private_dir(&workspace).map_err(|error| {
        anyhow::anyhow!(
            "cannot create agent workspace {}: {error}",
            workspace.display()
        )
    })?;
    let capsule =
        LinuxCapsule::headless_with_image_and_workspace(config.shell_image.clone(), workspace)
            .await?;
    Ok(Arc::new(CapsuleShell { capsule }))
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

    async fn exec(&self, command: &str) -> Result<ShellOutput, ToolError> {
        let command = format!("export DS_TOKEN_FILE=/run/foundry/ds-token; {command}");
        tracing::info!(
            target: "foundry_agentd::shell",
            command = %truncate_log_line(&redact_credentials(&command)),
            "shell command started"
        );
        let output = self
            .capsule
            .exec_streaming(&command, |_stream, line| {
                if !line.is_empty() {
                    tracing::info!(target: "foundry_agentd::shell", "{}", truncate_log_line(line));
                }
            })
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
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
        loop {
            scan_assigned_tasks(
                Arc::clone(&store),
                config,
                definition,
                Arc::clone(&model),
                shell.clone(),
                &mut handled,
            )
            .await;
            tokio::select! {
                received = notifications.recv() => {
                    if received.is_none() {
                        tracing::warn!("task change listener closed; reconnecting");
                        break;
                    }
                }
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
            // One scan sees every current assignment, so merge a burst of
            // notifications instead of scanning once per event.
            while notifications.try_recv().is_ok() {}
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
        if handled.get(&task.task_id) == Some(&fingerprint) {
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

/// Triage check mode: a code-driven coordinator that watches the project's
/// `triage_check` task and starts a `triage` child when Issues need attention.
async fn run_triage_check_watcher(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
) -> anyhow::Result<()> {
    let client = Arc::new(FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        definition.project_id.to_string(),
    ));
    let mut task_changes = listen_task_changes(&config.database_url).await?;
    let mut issue_changes = listen_issue_changes(&config.database_url).await?;
    loop {
        let _ = store.agent_process_busy(config.agent_id).await;
        match triage_check_once(&client).await {
            Ok(()) => {}
            Err(error) => tracing::warn!(error = %error, "triage check sweep failed"),
        }
        let _ = store.agent_process_idle(config.agent_id).await;
        tokio::select! {
            _ = task_changes.recv() => {}
            _ = issue_changes.recv() => {}
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
        while task_changes.try_recv().is_ok() {}
        while issue_changes.try_recv().is_ok() {}
    }
}

async fn triage_check_once(client: &FoundryClient) -> anyhow::Result<()> {
    let Some(task) = find_check_task(client).await? else {
        return Ok(());
    };
    if task.get("state").and_then(Value::as_str) != Some("in_progress") {
        return Ok(());
    }
    let task_id = task
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<Uuid>().ok())
        .ok_or_else(|| anyhow::anyhow!("triage_check task has no id"))?;
    let stage = task.get("stage").and_then(Value::as_str).unwrap_or("");
    let pending = pending_issue_count(client).await?;
    match stage {
        "check" => {
            if pending == 0 {
                client
                    .send_task_operation(task_id, "check:no_issues", "triage-check-no-issues")
                    .await?;
            } else {
                if let Some(failed) = find_failed_triage_child(client, task_id).await? {
                    client.retry_task(failed).await?;
                } else if !has_active_triage_child(client, task_id).await? {
                    client
                        .spawn_task_children(
                            task_id,
                            json!([{
                                "workflow": "triage",
                                "title": "分诊",
                                "description": "处理当前待分诊 Issue：分类、合并重复并创建后续任务。",
                                "input": {}
                            }]),
                            false,
                        )
                        .await?;
                }
                client
                    .send_task_operation(task_id, "check:has_issues", "triage-check-has-issues")
                    .await?;
            }
        }
        "waiting_issue" => {
            if pending > 0 {
                client
                    .send_task_operation(
                        task_id,
                        "waiting_issue:issue_arrived",
                        "triage-check-issue",
                    )
                    .await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn find_check_task(client: &FoundryClient) -> anyhow::Result<Option<Value>> {
    let tasks = client.list_tasks(None, "").await?;
    Ok(tasks
        .as_array()
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("workflow").and_then(Value::as_str) == Some("triage_check")
                    && !matches!(
                        task.get("state").and_then(Value::as_str),
                        Some("completed" | "failed")
                    )
            })
        })
        .cloned())
}

async fn has_active_triage_child(
    client: &FoundryClient,
    check_task_id: Uuid,
) -> anyhow::Result<bool> {
    let tasks = client.list_tasks(None, "").await?;
    let expected = check_task_id.to_string();
    Ok(tasks.as_array().is_some_and(|tasks| {
        tasks.iter().any(|task| {
            task.get("workflow").and_then(Value::as_str) == Some("triage")
                && task.get("parent_id").and_then(Value::as_str) == Some(expected.as_str())
                && matches!(
                    task.get("state").and_then(Value::as_str),
                    Some("pending" | "in_progress")
                )
        })
    }))
}

async fn find_failed_triage_child(
    client: &FoundryClient,
    check_task_id: Uuid,
) -> anyhow::Result<Option<Uuid>> {
    let tasks = client.list_tasks(None, "").await?;
    let expected = check_task_id.to_string();
    Ok(tasks.as_array().and_then(|tasks| {
        tasks.iter().find_map(|task| {
            let is_triage = task.get("workflow").and_then(Value::as_str) == Some("triage");
            let is_child = task.get("parent_id").and_then(Value::as_str) == Some(expected.as_str());
            let failed = task.get("state").and_then(Value::as_str) == Some("failed");
            if is_triage && is_child && failed {
                task.get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| id.parse::<Uuid>().ok())
            } else {
                None
            }
        })
    }))
}

async fn pending_issue_count(client: &FoundryClient) -> anyhow::Result<usize> {
    let batch = client.issue_triage_batch().await?;
    Ok(batch
        .get("pending")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0))
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
                _ = tokio::signal::ctrl_c() => return Ok(()),
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
                            "cannot assign the agent required by a role"
                        );
                        complete = false;
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
    let tools = build_tools(definition, &client, shell.clone())?;
    let runtime = build_runtime(definition, model, tools, shell.clone())?;

    if definition.shell {
        let Some(shell) = shell.as_ref() else {
            if definition.role.ends_with(":summarizer") {
                finish_codesummary(&client, task, false).await;
            }
            anyhow::bail!("shell agent has no shell");
        };
        match client.agent_credentials(task.task_id).await {
            Ok(credentials) => {
                if let Err(error) =
                    inject_task_credentials(shell, task.project_id, &credentials).await
                {
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

    if definition.shell {
        clear_task_credentials(shell.as_ref(), task.project_id).await;
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
    project_id: Uuid,
    credentials: &Value,
) -> anyhow::Result<()> {
    let root = format!("/work/{project_id}/.credentials");
    let git_dir = format!("{root}/git");
    shell_ok(shell, format!("mkdir -p {}", shell_quote(&git_dir))).await?;

    if let Some(git) = credentials.get("git").filter(|value| !value.is_null()) {
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
            &format!("{git_dir}/.git-credentials"),
            &format!("https://{username}:{token}@{host}\n"),
        )
        .await?;
        upload_text(
            shell,
            &format!("{git_dir}/gitconfig"),
            "[credential]\n\thelper = store\n",
        )
        .await?;
        upload_text(shell, &format!("{git_dir}/url"), url).await?;
    }

    if let Some(ds) = credentials
        .get("design_space")
        .filter(|value| !value.is_null())
    {
        if let Some(url) = ds.get("url").and_then(Value::as_str)
            && !url.is_empty()
        {
            upload_text(shell, &format!("{root}/ds-url"), url).await?;
        }
        if let Some(token) = ds.get("token").and_then(Value::as_str)
            && !token.is_empty()
        {
            shell_ok(
                shell,
                "mkdir -p /run/foundry && chmod 700 /run/foundry".to_string(),
            )
            .await?;
            upload_text(shell, "/run/foundry/ds-token", &format!("{token}\n")).await?;
            shell_ok(shell, "chmod 600 /run/foundry/ds-token".to_owned()).await?;
            upload_text(shell, &format!("{root}/ds-token"), &format!("{token}\n")).await?;
        }
    }

    shell_ok(
        shell,
        format!(
            "chmod 700 {} && chmod 600 {}/* 2>/dev/null || true",
            shell_quote(&root),
            shell_quote(&git_dir)
        ),
    )
    .await?;
    Ok(())
}

async fn clear_task_credentials(shell: Option<&Arc<dyn Shell>>, project_id: Uuid) {
    if let Some(shell) = shell {
        let _ = shell
            .exec(&format!(
                "rm -rf {} /run/foundry/ds-token",
                shell_quote(&format!("/work/{project_id}/.credentials"))
            ))
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
    } else {
        Ok(AgentRuntime::new(model, Arc::new(tools)))
    }
}

fn build_tools(
    definition: &AgentDefinition,
    client: &Arc<FoundryClient>,
    shell: Option<Arc<dyn Shell>>,
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
        tools.register(Arc::new(ShellTool::new(shell)));
    }
    Ok(tools)
}
