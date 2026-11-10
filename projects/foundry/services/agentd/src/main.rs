//! One managed agent process.
//!
//! The control-plane supervisor starts this binary with `FOUNDRY_AGENT_ID` and
//! a freshly minted `FOUNDRY_TOKEN`. The process loads its definition (model,
//! prompt, tool whitelist) from Postgres, claims only runs addressed to that
//! definition, and records process heartbeat / current run in
//! `agent_processes`.

use foundry_adapter_postgres::{AssignedTask, PostgresStore, listen_task_changes};
use foundry_agent_runtime::{
    AgentAssignTool, AgentCreateForRoleTool, AgentRunRequest, AgentRuntime,
    AgentTemplatesListTool, AgentsListTool, Budget, EchoTool, FoundryClient,
    FoundryRequirementsApplyTool, FoundryRequirementsDraftTool, FoundryRequirementsGetTool,
    FoundryRequirementsValidateTool, InMemoryTrajectory, IssueCommentTool, IssueGetTool,
    LocalSandbox, OpenAiCompatibleConfig, OpenAiCompatibleModel,
    ProjectBriefAnnotateTool, ProjectBriefAnnotationsListTool, ProjectBriefDraftApplyTool,
    ProjectBriefDraftCreateTool, ProjectBriefDraftGetTool, ProjectBriefDraftsListTool,
    ProjectBriefDraftSubmitTool, ProjectBriefGetTool, ProjectBriefResolveAnnotationTool,
    ProjectBriefVersionGetTool, ProjectBriefVersionsListTool, RequirementsAnnotateTool,
    RequirementsAnnotationsListTool, RequirementsResolveAnnotationTool, RunOutcome,
    TaskAddLinkTool,
    TaskAllowedOperationsTool, TaskAssignAgentTool, TaskCommentTool, TaskGetTool, TaskHistoryTool,
    TaskLinksListTool, TaskOperationTool, TaskRemoveLinkTool, TaskStartTool, TasksListTool,
    TasksReadyListTool, ToolRegistry,
};
use foundry_domain::AgentDefinition;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

struct Config {
    database_url: String,
    foundry_url: String,
    foundry_token: String,
    agent_id: Uuid,
    sandbox_root: Option<String>,
}

impl Config {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            foundry_url: required("FOUNDRY_URL")?,
            foundry_token: required("FOUNDRY_TOKEN")?,
            agent_id: required("FOUNDRY_AGENT_ID")?.parse()?,
            sandbox_root: std::env::var("FOUNDRY_SANDBOX_ROOT").ok(),
        })
    }
}

fn required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("{key} is required"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    foundry_shared::init_tracing("foundry_agentd=info,foundry_agent_runtime=info");
    let config = Config::from_env()?;
    let store = Arc::new(PostgresStore::connect(&config.database_url).await?);

    let definition = store
        .agent_definition(config.agent_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("agent definition {} not found", config.agent_id))?;
    if !definition.enabled {
        anyhow::bail!("agent definition {} is disabled", definition.name);
    }
    let api_key = std::env::var(&definition.api_key_env)
        .map_err(|_| anyhow::anyhow!("{} is not set", definition.api_key_env))?;
    let model = Arc::new(OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
        definition.model_base_url.clone(),
        api_key,
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
        "agentd started"
    );

    if definition.role == "dispatcher" {
        run_dispatcher_watcher(Arc::clone(&store), &config, &definition, model).await?;
        return Ok(());
    }

    run_agent_watcher(Arc::clone(&store), &config, &definition, model).await?;
    Ok(())
}

/// Agent mode: no durable queue. On startup, reconnect and every transient
/// task/Issue notification, inspect the tasks where this definition is an
/// active actor and decide what to do.
async fn run_agent_watcher(
    store: Arc<PostgresStore>,
    config: &Config,
    definition: &AgentDefinition,
    model: Arc<OpenAiCompatibleModel>,
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
        if let Err(error) = execute_task(config, definition, Arc::clone(&model), &task).await {
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
) -> anyhow::Result<()> {
    let client = Arc::new(FoundryClient::new(
        &config.foundry_url,
        &config.foundry_token,
        "",
    ));
    let tools = build_tools(definition, &client, config.sandbox_root.as_deref())?;
    let trajectory = Arc::new(InMemoryTrajectory::new());
    let runtime = AgentRuntime::new(model, Arc::new(tools), trajectory, Budget::default());

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
            let request = AgentRunRequest::new(
                definition.name.clone(),
                String::new(),
                definition.system_prompt.clone(),
                "Inspect the task pool now: call tasks_ready_list, assign every agent role \
                 that has no enabled agent, then call task_start for every task.",
            );
            tracing::info!("dispatcher sweep started");
            match runtime.run(request).await {
                Ok(RunOutcome::Finished(result)) => {
                    let final_message = result.final_message.as_deref().unwrap_or("");
                    tracing::info!(
                        stop = ?result.stop_reason,
                        steps = result.steps,
                        final = %truncate(final_message, 500),
                        "dispatcher sweep finished"
                    )
                }
                Ok(RunOutcome::WaitingForIssueComment { .. }) => {
                    tracing::warn!("dispatcher sweep unexpectedly paused")
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
async fn task_context(
    client: &FoundryClient,
    task_id: Uuid,
    account_id: Uuid,
) -> Option<String> {
    let task = match client.get_task(None, task_id).await {
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
                    && actor
                        .get("active")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
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
    let tools = build_tools(definition, &client, config.sandbox_root.as_deref())?;
    let runtime = AgentRuntime::new(
        model,
        Arc::new(tools),
        Arc::new(InMemoryTrajectory::new()),
        Budget::default(),
    );

    let task_id = task.task_id.to_string();
    let mut request = AgentRunRequest::new(
        definition.name.clone(),
        task_id.clone(),
        definition.system_prompt.clone(),
        "Inspect the task and decide what to do.",
    );
    request.context.push(
        "current_task",
        task_id.clone(),
        format!("task_id = {task_id}"),
    );
    if let Some(details) = task_context(&client, task.task_id, definition.account_id).await {
        request.context
            .push("task", task_id.clone(), details);
    }

    match runtime.run(request).await {
        Ok(RunOutcome::Finished(result)) => {
            tracing::info!(
                task_id = %task.task_id,
                stop_reason = ?result.stop_reason,
                steps = result.steps,
                input_tokens = result.usage.input_tokens,
                output_tokens = result.usage.output_tokens,
                final = %truncate(result.final_message.as_deref().unwrap_or(""), 1000),
                "agent task completed"
            );
        }
        Ok(RunOutcome::WaitingForIssueComment { .. }) => {
            tracing::warn!(task_id = %task.task_id, "agent asked to wait; no durable wait queue");
        }
        Err(error) => {
            tracing::warn!(task_id = %task.task_id, error = %error, "agent task failed");
        }
    }
    Ok(())
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(limit).collect();
    truncated.push('…');
    truncated
}

fn build_tools(
    definition: &AgentDefinition,
    client: &Arc<FoundryClient>,
    sandbox_root: Option<&str>,
) -> anyhow::Result<ToolRegistry> {
    let all = definition.tools.is_empty();
    let wants = |name: &str| all || definition.tools.iter().any(|tool| tool == name);
    let mut tools = ToolRegistry::new();
    if wants("echo") {
        tools.register(Arc::new(EchoTool));
    }
    if wants("project_brief_get") {
        tools.register(Arc::new(ProjectBriefGetTool::new(Arc::clone(client))));
    }
    if wants("project_brief_versions_list") {
        tools.register(Arc::new(ProjectBriefVersionsListTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_version_get") {
        tools.register(Arc::new(ProjectBriefVersionGetTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_drafts_list") {
        tools.register(Arc::new(ProjectBriefDraftsListTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_draft_create") {
        tools.register(Arc::new(ProjectBriefDraftCreateTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_draft_get") {
        tools.register(Arc::new(ProjectBriefDraftGetTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_draft_apply") {
        tools.register(Arc::new(ProjectBriefDraftApplyTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_draft_submit") {
        tools.register(Arc::new(ProjectBriefDraftSubmitTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_annotations_list") {
        tools.register(Arc::new(ProjectBriefAnnotationsListTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("project_brief_annotate") {
        tools.register(Arc::new(ProjectBriefAnnotateTool::new(Arc::clone(client))));
    }
    if wants("project_brief_resolve_annotation") {
        tools.register(Arc::new(ProjectBriefResolveAnnotationTool::new(
            Arc::clone(client),
        )));
    }
    if wants("task_links_list") {
        tools.register(Arc::new(TaskLinksListTool::new(Arc::clone(client))));
    }
    if wants("issue_get") {
        tools.register(Arc::new(IssueGetTool::new(Arc::clone(client))));
    }
    if wants("issue_comment") {
        tools.register(Arc::new(IssueCommentTool::new(Arc::clone(client))));
    }
    if wants("requirements_get") {
        tools.register(Arc::new(FoundryRequirementsGetTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("requirements_draft") {
        tools.register(Arc::new(FoundryRequirementsDraftTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("requirements_apply_operations") {
        tools.register(Arc::new(FoundryRequirementsApplyTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("requirements_validate") {
        tools.register(Arc::new(FoundryRequirementsValidateTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("requirements_annotations_list") {
        tools.register(Arc::new(RequirementsAnnotationsListTool::new(Arc::clone(
            client,
        ))));
    }
    if wants("requirements_annotate") {
        tools.register(Arc::new(RequirementsAnnotateTool::new(Arc::clone(client))));
    }
    if wants("requirements_resolve_annotation") {
        tools.register(Arc::new(RequirementsResolveAnnotationTool::new(
            Arc::clone(client),
        )));
    }
    if wants("sandbox_run_command")
        && let Some(root) = sandbox_root
    {
        tools.register(Arc::new(LocalSandbox::new(root)?));
    }
    if wants("agents_list") {
        tools.register(Arc::new(AgentsListTool::new(Arc::clone(client))));
    }
    if wants("agent_templates_list") {
        tools.register(Arc::new(AgentTemplatesListTool::new(Arc::clone(client))));
    }
    if wants("agent_create_for_role") {
        tools.register(Arc::new(AgentCreateForRoleTool::new(Arc::clone(client))));
    }
    if wants("agent_assign") {
        tools.register(Arc::new(AgentAssignTool::new(Arc::clone(client))));
    }
    if wants("tasks_ready_list") {
        tools.register(Arc::new(TasksReadyListTool::new(Arc::clone(client))));
    }
    if wants("task_start") {
        tools.register(Arc::new(TaskStartTool::new(Arc::clone(client))));
    }
    if wants("tasks_list") {
        tools.register(Arc::new(TasksListTool::new(Arc::clone(client))));
    }
    if wants("task_get") {
        tools.register(Arc::new(TaskGetTool::new(Arc::clone(client))));
    }
    if wants("task_allowed_operations") {
        tools.register(Arc::new(TaskAllowedOperationsTool::new(Arc::clone(client))));
    }
    if wants("task_operation") {
        tools.register(Arc::new(TaskOperationTool::new(Arc::clone(client))));
    }
    if wants("task_comment") {
        tools.register(Arc::new(TaskCommentTool::new(Arc::clone(client))));
    }
    if wants("task_history") {
        tools.register(Arc::new(TaskHistoryTool::new(Arc::clone(client))));
    }
    if wants("task_add_link") {
        tools.register(Arc::new(TaskAddLinkTool::new(Arc::clone(client))));
    }
    if wants("task_remove_link") {
        tools.register(Arc::new(TaskRemoveLinkTool::new(Arc::clone(client))));
    }
    if wants("task_assign_agent") {
        tools.register(Arc::new(TaskAssignAgentTool::new(Arc::clone(client))));
    }
    Ok(tools)
}
