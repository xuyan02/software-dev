//! One managed agent process.
//!
//! The control-plane supervisor starts this binary with `FOUNDRY_AGENT_ID` and
//! a freshly minted `FOUNDRY_TOKEN`. The process loads its definition (model,
//! prompt, tool whitelist) from Postgres, claims only runs addressed to that
//! definition, and records process heartbeat / current run in
//! `agent_processes`.

use async_trait::async_trait;
use clap::Parser;
use foundry_adapter_postgres::{AssignedTask, PostgresStore, listen_task_changes};
use foundry_agent_runtime::{
    AgentCreateForRoleTool, AgentRunRequest, AgentRuntime, AgentTemplatesListTool, AgentsListTool,
    FoundryClient, MergeRequestTool, ProjectTool, Shell, ShellOutput, ShellTool,
    TaskAllowedOperationsTool, TaskAssignAgentTool, TaskBindActorTool, TaskCommentTool, TaskGetTool, TaskHistoryTool,
    TaskOperationTool, TaskSpawnChildrenTool, TaskStartTool, TasksReadyListTool, ToolError,
    ToolRegistry, compose_system_prompt,
};
use foundry_domain::AgentDefinition;
use graph_agent::{OpenAiCompatibleConfig, OpenAiCompatibleModel};
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

    run_agent_watcher(Arc::clone(&store), &config, &definition, model, shell).await?;
    Ok(())
}

async fn create_shell(config: &Config) -> anyhow::Result<Arc<dyn Shell>> {
    let capsule = LinuxCapsule::headless_with_image(config.shell_image.clone()).await?;
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

/// Dispatcher mode: sweep the task pool on startup and after every task change.
///
/// The PM does not claim queue Runs. It listens on the Postgres notification
/// channel and re-queries the task pool; the model decides which agents to
/// create and which task roles to assign.
async fn run_dispatcher_watcher(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
    shell: Option<Arc<dyn Shell>>,
) -> anyhow::Result<()> {
    let client = Arc::new(FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        definition.project_id.to_string(),
    ));
    let tools = build_tools(definition, &client, shell.clone())?;
    let runtime = build_runtime(definition, model, tools, shell)?;

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
            let system_prompt = match system_prompt_for(definition) {
                Ok(prompt) => prompt,
                Err(error) => {
                    tracing::warn!(agent = %definition.name, %error, "cannot compose skills");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };
            let request = AgentRunRequest::new(
                definition.name.clone(),
                String::new(),
                system_prompt,
                "Inspect the task pool now: call tasks_ready_list, assign every agent role \
                 that has no enabled agent, then call task_start for every task.",
            )
            .with_project_id(definition.project_id.to_string());
            tracing::info!("dispatcher sweep started");
            match runtime.run(request).await {
                Ok(result) => {
                    let final_message = result.final_message.as_deref().unwrap_or("");
                    tracing::info!(
                        final = %truncate(final_message, 500),
                        "dispatcher sweep finished"
                    )
                }
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
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
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
    .with_project_id(task.project_id.to_string());
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
    if definition.role.ends_with(":summarizer") {
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
