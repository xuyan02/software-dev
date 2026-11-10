//! Postgres storage for managed agents.

use foundry_domain::{
    AgentBinding, AgentDefinition, AgentProcess, AssignedAgentView, CreateAgentBinding,
    CreateAgentDefinition, ReadyTaskRoleView, ReadyTaskView, RoleKind, WorkflowBody,
};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{PostgresStore, RepoError, backend, map_write_error};

/// Listen for task-change notifications. The returned receiver yields once per
/// notification; the listener task stops when the receiver is dropped.
pub async fn listen_task_changes(
    database_url: &str,
) -> Result<tokio::sync::mpsc::Receiver<()>, RepoError> {
    let mut listener = sqlx::postgres::PgListener::connect(database_url)
        .await
        .map_err(backend)?;
    listener
        .listen("foundry_task_changes")
        .await
        .map_err(backend)?;
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                received = listener.recv() => {
                    if received.is_err() || tx.send(()).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    Ok(rx)
}

type DefinitionRow = (
    Uuid,
    String,
    Uuid,
    String,
    String,
    String,
    String,
    String,
    String,
    serde_json::Value,
    bool,
    OffsetDateTime,
    OffsetDateTime,
);

type BindingRow = (
    Uuid,
    Uuid,
    String,
    String,
    String,
    Uuid,
    String,
    OffsetDateTime,
);

type ProcessRow = (
    Uuid,
    String,
    String,
    Option<i32>,
    Option<OffsetDateTime>,
    Option<String>,
    Option<OffsetDateTime>,
    Option<OffsetDateTime>,
);

type AssignedTaskRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<String>,
    String,
    OffsetDateTime,
    Option<Uuid>,
    Option<OffsetDateTime>,
);

/// A task where an agent definition is currently an active actor.
#[derive(Debug, Clone)]
pub struct AssignedTask {
    pub task_id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub workflow: String,
    pub stage: Option<String>,
    pub state: String,
    pub updated_at: OffsetDateTime,
    pub last_human_comment_id: Option<Uuid>,
    pub last_human_comment_at: Option<OffsetDateTime>,
}

impl PostgresStore {
    /// Tasks where this agent definition is assigned to the role that is
    /// currently active. Managed agents use this on startup and after a
    /// transient task/Issue notification.
    pub async fn assigned_active_tasks(
        &self,
        definition_id: Uuid,
    ) -> Result<Vec<AssignedTask>, RepoError> {
        let rows: Vec<AssignedTaskRow> = sqlx::query_as(
            "select t.id, t.project_id, t.title, t.workflow, t.stage, t.state, t.updated_at, \
                    (select c.id from issue_comments c \
                     join task_links l on l.target_id = c.issue_id \
                     where l.task_id = t.id and c.author_kind = 'human' \
                     order by c.created_at desc, c.id desc limit 1), \
                    (select max(c.created_at) from issue_comments c \
                     join task_links l on l.target_id = c.issue_id \
                     where l.task_id = t.id and c.author_kind = 'human') \
             from tasks t \
             join task_agent_assignments a \
               on a.task_id = t.id and a.agent_definition_id = $1 \
             join task_actors ac \
               on ac.task_id = t.id \
              and ac.account_id = (select account_id from agent_definitions where id = $1) \
              and ac.active \
              and a.role = t.workflow || ':' || ac.role_key \
             where t.state = 'in_progress' \
               and not exists ( \
                   select 1 from task_dependencies d \
                   join tasks dep on dep.id = d.depends_on_task_id \
                   where d.task_id = t.id and dep.state <> 'completed' \
               ) \
             order by t.updated_at",
        )
        .bind(definition_id)
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    task_id,
                    project_id,
                    title,
                    workflow,
                    stage,
                    state,
                    updated_at,
                    last_human_comment_id,
                    last_human_comment_at,
                )| AssignedTask {
                    task_id,
                    project_id,
                    title,
                    workflow,
                    stage,
                    state,
                    updated_at,
                    last_human_comment_id,
                    last_human_comment_at,
                },
            )
            .collect())
    }

    pub async fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, RepoError> {
        let rows: Vec<DefinitionRow> = sqlx::query_as(
            "select d.id, d.name, d.account_id, a.handle, d.role, d.model_base_url, d.model_name, \
             d.api_key_env, d.system_prompt, d.tools, d.enabled, d.created_at, d.updated_at \
             from agent_definitions d join accounts a on a.id = d.account_id order by d.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        rows.into_iter().map(definition_from_row).collect()
    }

    pub async fn agent_definition(&self, id: Uuid) -> Result<Option<AgentDefinition>, RepoError> {
        let row: Option<DefinitionRow> = sqlx::query_as(
            "select d.id, d.name, d.account_id, a.handle, d.role, d.model_base_url, d.model_name, \
             d.api_key_env, d.system_prompt, d.tools, d.enabled, d.created_at, d.updated_at \
             from agent_definitions d join accounts a on a.id = d.account_id where d.id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(definition_from_row).transpose()
    }

    pub async fn insert_agent_definition(
        &self,
        command: &CreateAgentDefinition,
        account_id: Uuid,
        name: &str,
    ) -> Result<AgentDefinition, RepoError> {
        let id = Uuid::now_v7();
        let now = OffsetDateTime::now_utc();
        let tools = serde_json::to_value(&command.tools)
            .map_err(|error| RepoError::Backend(error.to_string()))?;
        sqlx::query(
            "insert into agent_definitions \
             (id, name, account_id, role, model_base_url, model_name, api_key_env, system_prompt, \
              tools, enabled, created_at, updated_at) \
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)",
        )
        .bind(id)
        .bind(name)
        .bind(account_id)
        .bind(&command.role)
        .bind(&command.model_base_url)
        .bind(&command.model_name)
        .bind(&command.api_key_env)
        .bind(&command.system_prompt)
        .bind(tools)
        .bind(command.enabled)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        self.agent_definition(id)
            .await?
            .ok_or_else(|| RepoError::Backend(format!("agent definition {id} disappeared")))
    }

    pub async fn update_agent_definition(
        &self,
        id: Uuid,
        command: &CreateAgentDefinition,
    ) -> Result<(), RepoError> {
        let tools = serde_json::to_value(&command.tools)
            .map_err(|error| RepoError::Backend(error.to_string()))?;
        let affected = sqlx::query(
            "update agent_definitions set \
               model_base_url = $2, model_name = $3, api_key_env = $4, system_prompt = $5, \
               tools = $6, enabled = $7, role = $8, updated_at = now() \
             where id = $1",
        )
        .bind(id)
        .bind(&command.model_base_url)
        .bind(&command.model_name)
        .bind(&command.api_key_env)
        .bind(&command.system_prompt)
        .bind(tools)
        .bind(command.enabled)
        .bind(&command.role)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?
        .rows_affected();
        if affected == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    pub async fn set_agent_definition_enabled(
        &self,
        id: Uuid,
        enabled: bool,
    ) -> Result<(), RepoError> {
        let affected = sqlx::query(
            "update agent_definitions set enabled = $2, updated_at = now() where id = $1",
        )
        .bind(id)
        .bind(enabled)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?
        .rows_affected();
        if affected == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    pub async fn list_agent_bindings(&self) -> Result<Vec<AgentBinding>, RepoError> {
        let rows: Vec<BindingRow> = sqlx::query_as(
            "select b.id, b.project_id, p.name, b.workflow_id, b.role, b.agent_definition_id, \
             d.name, b.created_at \
             from agent_bindings b \
             join projects p on p.id = b.project_id \
             join agent_definitions d on d.id = b.agent_definition_id \
             order by p.name, b.workflow_id, b.role",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    project_id,
                    project_name,
                    workflow_id,
                    role,
                    agent_definition_id,
                    agent_name,
                    created_at,
                )| {
                    AgentBinding {
                        id,
                        project_id,
                        project_name,
                        workflow_id,
                        role,
                        agent_definition_id,
                        agent_name,
                        created_at,
                    }
                },
            )
            .collect())
    }

    pub async fn insert_agent_binding(
        &self,
        command: &CreateAgentBinding,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "insert into agent_bindings \
             (id, project_id, workflow_id, role, agent_definition_id, created_at) \
             values ($1, $2, $3, $4, $5, now())",
        )
        .bind(Uuid::now_v7())
        .bind(command.project_id)
        .bind(&command.workflow_id)
        .bind(&command.role)
        .bind(command.agent_definition_id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    pub async fn delete_agent_binding(&self, id: Uuid) -> Result<(), RepoError> {
        let affected = sqlx::query("delete from agent_bindings where id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(map_write_error)?
            .rows_affected();
        if affected == 0 {
            return Err(RepoError::NotFound);
        }
        Ok(())
    }

    /// Every Ready task with at least one active agent role.
    ///
    /// Ready tasks plus started tasks with an active agent role that has no
    /// usable assignment (for example a review stage that just became active).
    /// Only active agent roles with `min_actors > 0` are listed. Existing agent
    /// assignments are included so the PM can skip them.
    /// In-progress tasks whose transitions should be re-evaluated at startup.
    pub async fn in_progress_task_ids(&self) -> Result<Vec<Uuid>, RepoError> {
        sqlx::query_scalar("select id from tasks where state = 'in_progress' order by created_at")
            .fetch_all(&self.pool)
            .await
            .map_err(backend)
    }

    pub async fn list_ready_tasks(&self) -> Result<Vec<ReadyTaskView>, RepoError> {
        type Row = (
            Uuid,
            Uuid,
            String,
            String,
            String,
            Option<String>,
            String,
            serde_json::Value,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "select t.id, t.project_id, p.name, t.title, t.workflow, t.stage, t.state, v.body \
             from tasks t \
             join projects p on p.id = t.project_id \
             join workflow_versions v \
               on v.workflow_id = t.workflow and v.version = t.workflow_version \
             where t.state in ('pending', 'in_progress') \
               and not exists ( \
                   select 1 from task_dependencies d \
                   join tasks dep on dep.id = d.depends_on_task_id \
                   where d.task_id = t.id and dep.state <> 'completed' \
               ) \
             order by t.created_at",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;

        let mut ready = Vec::new();
        for (task_id, project_id, project_name, title, workflow, stage, state, body) in rows {
            let Some(stage_name) = stage else {
                continue;
            };
            let body: WorkflowBody = serde_json::from_value(body)
                .map_err(|error| RepoError::Backend(error.to_string()))?;
            if !body.stages.iter().any(|stage| stage.name == stage_name) {
                continue;
            }
            let assignments: Vec<(String, Uuid, String, bool)> = sqlx::query_as(
                "select a.role, a.agent_definition_id, d.name, d.enabled \
                 from task_agent_assignments a \
                 join agent_definitions d on d.id = a.agent_definition_id \
                 where a.task_id = $1",
            )
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(backend)?;

            // Bind every agent role in the workflow up front, not only the
            // roles active in the current stage. The PM sweeps a task once
            // before it starts; later stages must find their agents already
            // assigned.
            let mut roles = Vec::new();
            for role in &body.roles {
                if role.kind != RoleKind::Agent || role.min_actors == 0 {
                    continue;
                }
                let role_name = &role.name;
                let task_role = format!("{workflow}:{role_name}");
                let agent = assignments
                    .iter()
                    .find(|(assigned_role, ..)| assigned_role == &task_role)
                    .map(
                        |(_, agent_definition_id, name, enabled)| AssignedAgentView {
                            agent_definition_id: *agent_definition_id,
                            name: name.clone(),
                            enabled: *enabled,
                        },
                    );
                roles.push(ReadyTaskRoleView {
                    name: role_name.clone(),
                    task_role,
                    kind: role.kind,
                    min_actors: role.min_actors,
                    max_actors: role.max_actors,
                    agent,
                });
            }
            if roles.is_empty() {
                continue;
            }
            // A started task only re-enters the PM queue when an agent role is
            // missing a usable assignment. All roles are expected to be bound
            // before the task starts.
            if state == "in_progress"
                && roles
                    .iter()
                    .all(|role| role.agent.as_ref().is_some_and(|agent| agent.enabled))
            {
                continue;
            }
            ready.push(ReadyTaskView {
                project_id,
                project_name: project_name.clone(),
                task_id,
                title: title.clone(),
                workflow: workflow.clone(),
                stage: stage_name,
                roles,
            });
        }
        Ok(ready)
    }

    /// The platform dispatcher agent (role `dispatcher`).
    pub async fn dispatcher_agent(&self) -> Result<Option<(Uuid, String, Uuid)>, RepoError> {
        let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
            "select id, name, account_id from agent_definitions \
             where role = 'dispatcher' and enabled order by name limit 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row)
    }

    /// Task-level assignment for one role.
    pub async fn task_assignment(
        &self,
        task_id: Uuid,
        role: &str,
    ) -> Result<Option<(Uuid, String, Uuid)>, RepoError> {
        let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
            "select a.agent_definition_id, d.name, d.account_id \
             from task_agent_assignments a \
             join agent_definitions d on d.id = a.agent_definition_id \
             where a.task_id = $1 and a.role = $2 and d.enabled",
        )
        .bind(task_id)
        .bind(role)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row)
    }

    /// Roles active in the task's current stage.
    async fn current_stage_roles(&self, task_id: Uuid) -> Result<Vec<String>, RepoError> {
        let row: Option<(Option<String>, serde_json::Value)> = sqlx::query_as(
            "select t.stage, v.body from tasks t \
             join workflow_versions v \
               on v.workflow_id = t.workflow and v.version = t.workflow_version \
             where t.id = $1",
        )
        .bind(task_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some((Some(stage_name), body)) = row else {
            return Ok(Vec::new());
        };
        let body: WorkflowBody = serde_json::from_value(body)
            .map_err(|error| RepoError::Backend(error.to_string()))?;
        Ok(body
            .stages
            .into_iter()
            .find(|stage| stage.name == stage_name)
            .map(|stage| stage.active_roles)
            .unwrap_or_default())
    }

    pub async fn assign_task(
        &self,
        task_id: Uuid,
        role: &str,
        agent_definition_id: Uuid,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "insert into task_agent_assignments (task_id, role, agent_definition_id) \
             values ($1, $2, $3) \
             on conflict (task_id, role) do update set \
               agent_definition_id = excluded.agent_definition_id, created_at = now()",
        )
        .bind(task_id)
        .bind(role)
        .bind(agent_definition_id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        // The assigned agent account becomes a task actor for the role. Only
        // roles active in the current stage start active; later-stage roles are
        // bound now but activated when their stage is entered.
        let role_key = role.split_once(':').map(|(_, role)| role).unwrap_or(role);
        let active = self
            .current_stage_roles(task_id)
            .await?
            .iter()
            .any(|role| role == role_key);
        let account_id: Option<Uuid> = sqlx::query_scalar(
            "select account_id from agent_definitions where id = $1",
        )
        .bind(agent_definition_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        if let Some(account_id) = account_id {
            sqlx::query(
                "insert into task_actors (task_id, role_key, account_id, active, work) \
                 values ($1, $2, $3, $4, null) \
                 on conflict (task_id, role_key, account_id) do update set active = excluded.active",
            )
            .bind(task_id)
            .bind(role_key)
            .bind(account_id)
            .bind(active)
            .execute(&self.pool)
            .await
            .map_err(map_write_error)?;
        }
        Ok(())
    }

    /// Resolve the enabled agent definition bound to a project workflow role.
    pub async fn resolve_agent_binding(
        &self,
        project_id: Uuid,
        workflow_id: &str,
        role: &str,
    ) -> Result<Option<(Uuid, String, Uuid)>, RepoError> {
        let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
            "select b.agent_definition_id, d.name, d.account_id \
             from agent_bindings b \
             join agent_definitions d on d.id = b.agent_definition_id \
             where b.project_id = $1 and b.workflow_id = $2 and b.role = $3 and d.enabled",
        )
        .bind(project_id)
        .bind(workflow_id)
        .bind(role)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row)
    }

    /// Platform routing for built-in agents when no project binding exists.
    ///
    /// The PM agent owns project goal clarification. This is platform policy,
    /// not user configuration, so it lives here rather than in a settings UI.
    pub async fn platform_agent_for(
        &self,
        workflow_id: &str,
        role: &str,
    ) -> Result<Option<(Uuid, String, Uuid)>, RepoError> {
        if workflow_id != "goal_clarification" || role != "clarifier" {
            return Ok(None);
        }
        let row: Option<(Uuid, String, Uuid)> = sqlx::query_as(
            "select id, name, account_id from agent_definitions \
             where name = 'PM Agent' and enabled",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        Ok(row)
    }

    /// Make the agent account a member of the project. Platform agents are
    /// members automatically; users never add them by hand.
    pub async fn ensure_project_member(
        &self,
        project_id: Uuid,
        account_id: Uuid,
    ) -> Result<(), RepoError> {
        sqlx::query(
            "insert into project_members (project_id, account_id, role) \
             values ($1, $2, 'member') on conflict do nothing",
        )
        .bind(project_id)
        .bind(account_id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    /// Kind of a role in the latest published version of a workflow.
    pub async fn workflow_role_kind(
        &self,
        workflow_id: &str,
        role: &str,
    ) -> Result<Option<RoleKind>, RepoError> {
        let body: Option<serde_json::Value> = sqlx::query_scalar(
            "select body from workflow_versions where workflow_id = $1 \
             order by version desc limit 1",
        )
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        let Some(body) = body else {
            return Ok(None);
        };
        let body: WorkflowBody =
            serde_json::from_value(body).map_err(|error| RepoError::Backend(error.to_string()))?;
        Ok(body
            .roles
            .into_iter()
            .find(|spec| spec.name == role)
            .map(|spec| spec.kind))
    }

    pub async fn list_agent_processes(&self) -> Result<Vec<AgentProcess>, RepoError> {
        let rows: Vec<ProcessRow> = sqlx::query_as(
            "select d.id, d.name, coalesce(p.status, 'stopped'), p.pid, p.heartbeat_at, \
             p.last_error, p.started_at, p.stopped_at \
             from agent_definitions d \
             left join agent_processes p on p.agent_definition_id = d.id \
             order by d.name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    agent_definition_id,
                    agent_name,
                    status,
                    pid,
                    heartbeat_at,
                    last_error,
                    started_at,
                    stopped_at,
                )| AgentProcess {
                    agent_definition_id,
                    agent_name,
                    status,
                    pid,
                    heartbeat_at,
                    last_error,
                    started_at,
                    stopped_at,
                },
            )
            .collect())
    }

    pub async fn agent_process_started(&self, id: Uuid, pid: i32) -> Result<(), RepoError> {
        sqlx::query(
            "insert into agent_processes \
             (agent_definition_id, status, pid, heartbeat_at, started_at) \
             values ($1, 'starting', $2, now(), now()) \
             on conflict (agent_definition_id) do update set \
               status = 'starting', pid = $2, heartbeat_at = now(), \
               last_error = null, started_at = now(), stopped_at = null",
        )
        .bind(id)
        .bind(pid)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    pub async fn agent_process_idle(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            "update agent_processes \
             set status = 'idle', heartbeat_at = now() \
             where agent_definition_id = $1 and status in ('running', 'starting')",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    /// Mark an agent process as working.
    pub async fn agent_process_busy(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            "insert into agent_processes \
             (agent_definition_id, status, heartbeat_at) \
             values ($1, 'running', now()) \
             on conflict (agent_definition_id) do update set \
               status = 'running', heartbeat_at = now()",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    pub async fn agent_process_heartbeat(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            "update agent_processes set heartbeat_at = now() where agent_definition_id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    pub async fn agent_process_stopped(&self, id: Uuid) -> Result<(), RepoError> {
        sqlx::query(
            "insert into agent_processes \
             (agent_definition_id, status, stopped_at, heartbeat_at, pid) \
             values ($1, 'stopped', now(), now(), null) \
             on conflict (agent_definition_id) do update set \
               status = 'stopped', pid = null, stopped_at = now(), heartbeat_at = now()",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }

    pub async fn agent_process_failed(&self, id: Uuid, error: &str) -> Result<(), RepoError> {
        sqlx::query(
            "insert into agent_processes \
             (agent_definition_id, status, last_error, stopped_at, heartbeat_at, pid) \
             values ($1, 'failed', $2, now(), now(), null) \
             on conflict (agent_definition_id) do update set \
               status = 'failed', last_error = $2, pid = null, stopped_at = now(), \
               heartbeat_at = now()",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await
        .map_err(map_write_error)?;
        Ok(())
    }
}

fn definition_from_row(row: DefinitionRow) -> Result<AgentDefinition, RepoError> {
    let (
        id,
        name,
        account_id,
        account_handle,
        role,
        model_base_url,
        model_name,
        api_key_env,
        system_prompt,
        tools,
        enabled,
        created_at,
        updated_at,
    ) = row;
    let tools: Vec<String> =
        serde_json::from_value(tools).map_err(|error| RepoError::Backend(error.to_string()))?;
    Ok(AgentDefinition {
        id,
        name,
        account_id,
        account_handle,
        role,
        model_base_url,
        model_name,
        api_key_env,
        system_prompt,
        tools,
        enabled,
        created_at,
        updated_at,
    })
}
