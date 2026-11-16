//! Admin service for managed agents.
//!
//! Agents are code-defined and project-owned. The PM dispatcher uses this surface to list
//! agents/templates and to create/assign agents for unassigned task roles.
//! Human users do not configure agents.

use crate::platform_agents;
use crate::supervisor::AgentSupervisor;
use foundry_adapter_postgres::PostgresStore;
use foundry_app::{
    AccountRepository, AgentAdminUseCases, ServiceError, TaskRepository, hash_password,
    new_session_token,
};
use foundry_domain::{
    Account, AccountKind, AccountRole, AccountStatus, Actor, AgentDefinition, AgentOverview,
    AgentTemplateView, AssignAgentToTask, CreateAgentDefinition, CreateAgentFromTemplate,
    ReadyTaskView, RoleKind,
};
use std::sync::Arc;
use uuid::Uuid;

/// Random suffix for a generated agent handle.
///
/// UUID v7 starts with a millisecond timestamp, so its leading bytes repeat for
/// tens of seconds. Use the random tail instead; otherwise every agent created
/// in the same time window collides on `accounts.handle`.
fn agent_handle_suffix() -> String {
    let suffix = Uuid::now_v7().simple().to_string();
    suffix[suffix.len() - 8..].to_string()
}

pub struct PostgresAgentAdmin {
    store: Arc<PostgresStore>,
    supervisor: Arc<AgentSupervisor>,
    model_base_url: String,
    model_name: String,
}

impl PostgresAgentAdmin {
    pub fn new(
        store: Arc<PostgresStore>,
        supervisor: Arc<AgentSupervisor>,
        model_base_url: String,
        model_name: String,
    ) -> Self {
        Self {
            store,
            supervisor,
            model_base_url,
            model_name,
        }
    }

    async fn require_admin(&self, actor: &Actor) -> Result<Account, ServiceError> {
        let account = AccountRepository::get_by_handle(self.store.as_ref(), actor.id())
            .await?
            .ok_or_else(|| ServiceError::Unauthorized("unknown account".to_string()))?;
        if account.status != AccountStatus::Active || account.role != AccountRole::Admin {
            return Err(ServiceError::Forbidden(
                "agent management requires an active administrator".to_string(),
            ));
        }
        Ok(account)
    }

    /// Validate a `workflow:role` key against the task's workflow and the
    /// workflow's role kind.
    async fn validate_task_role(
        &self,
        task_id: Uuid,
        role: &str,
    ) -> Result<(Uuid, String, String), ServiceError> {
        let task = TaskRepository::get(self.store.as_ref(), task_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("task", task_id))?;
        let (workflow_id, role_name) = role
            .split_once(':')
            .ok_or_else(|| ServiceError::Validation("role must be workflow:role".to_string()))?;
        if workflow_id != task.workflow.as_str() {
            return Err(ServiceError::Validation(format!(
                "task {task_id} uses workflow {}, not {workflow_id}",
                task.workflow.as_str()
            )));
        }
        match self
            .store
            .workflow_role_kind(workflow_id, role_name)
            .await?
        {
            Some(RoleKind::Agent) => {}
            Some(_) => {
                return Err(ServiceError::Validation(format!(
                    "role {role_name} in {workflow_id} is not an agent role"
                )));
            }
            None => {
                return Err(ServiceError::Validation(format!(
                    "workflow {workflow_id} does not declare role {role_name}"
                )));
            }
        }
        Ok((
            task.project_id,
            workflow_id.to_string(),
            role_name.to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl AgentAdminUseCases for PostgresAgentAdmin {
    async fn overview(
        &self,
        actor: Actor,
        project_id: Option<Uuid>,
    ) -> Result<AgentOverview, ServiceError> {
        self.require_admin(&actor).await?;
        let definitions = self.store.list_agent_definitions().await?;
        let Some(project_id) = project_id else {
            return Ok(AgentOverview {
                definitions,
                bindings: self.store.list_agent_bindings().await?,
                processes: self.store.list_agent_processes().await?,
            });
        };
        // Every agent belongs to exactly one project.
        let definitions: Vec<_> = definitions
            .into_iter()
            .filter(|definition| definition.project_id == project_id)
            .collect();
        let definition_ids: std::collections::HashSet<_> =
            definitions.iter().map(|definition| definition.id).collect();
        Ok(AgentOverview {
            definitions,
            bindings: self
                .store
                .list_agent_bindings()
                .await?
                .into_iter()
                .filter(|binding| binding.project_id == project_id)
                .collect(),
            processes: self
                .store
                .list_agent_processes()
                .await?
                .into_iter()
                .filter(|process| definition_ids.contains(&process.agent_definition_id))
                .collect(),
        })
    }

    async fn agents(
        &self,
        actor: Actor,
        project_id: Option<Uuid>,
    ) -> Result<Vec<AgentDefinition>, ServiceError> {
        self.require_admin(&actor).await?;
        match project_id {
            Some(project_id) => Ok(self
                .store
                .list_project_agent_definitions(project_id)
                .await?),
            None => Ok(self.store.list_agent_definitions().await?),
        }
    }

    async fn ready_tasks(
        &self,
        actor: Actor,
        project_id: Option<Uuid>,
    ) -> Result<Vec<ReadyTaskView>, ServiceError> {
        self.require_admin(&actor).await?;
        Ok(self.store.list_ready_tasks(project_id).await?)
    }

    async fn ensure_project_pm(&self, project_id: Uuid) {
        if let Err(error) = platform_agents::ensure_project_pm(
            self.store.as_ref(),
            &self.supervisor,
            project_id,
            &self.model_base_url,
            &self.model_name,
        )
        .await
        {
            tracing::warn!(project_id = %project_id, %error, "cannot provision project PM agent");
        }
    }

    async fn templates(
        &self,
        actor: Actor,
        role: Option<String>,
    ) -> Result<Vec<AgentTemplateView>, ServiceError> {
        self.require_admin(&actor).await?;
        Ok(platform_agents::templates()
            .iter()
            .filter(|template| role.as_deref().is_none_or(|role| template.role == role))
            .map(|template| AgentTemplateView {
                id: template.id.to_string(),
                name: template.name.to_string(),
                role: template.role.to_string(),
                description: template.description.to_string(),
            })
            .collect())
    }

    async fn create_from_template(
        &self,
        actor: Actor,
        command: CreateAgentFromTemplate,
    ) -> Result<AgentDefinition, ServiceError> {
        self.require_admin(&actor).await?;
        let (project_id, workflow_id, role_name) = self
            .validate_task_role(command.task_id, &command.role)
            .await?;
        let template = platform_agents::template(&command.template_id).ok_or_else(|| {
            ServiceError::Validation(format!("unknown agent template {}", command.template_id))
        })?;
        if template.role != command.role {
            return Err(ServiceError::Validation(format!(
                "template {} fills {}, not {}",
                template.id, template.role, command.role
            )));
        }
        if let Some(existing) = self
            .store
            .list_project_agent_definitions(project_id)
            .await?
            .into_iter()
            .find(|definition| definition.role == command.role && definition.enabled)
        {
            return Ok(existing);
        }

        let short = agent_handle_suffix();
        let handle = format!("agent-{short}");
        let password = new_session_token();
        let hash = hash_password(&password)?;
        let account = Account::new(
            handle.clone(),
            template.name.to_string(),
            AccountKind::Agent,
            AccountRole::Member,
            None,
            Some(hash),
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
        AccountRepository::insert(self.store.as_ref(), &account).await?;
        self.store
            .ensure_project_member(project_id, account.id)
            .await?;

        let name = format!("{} {short}", template.name);
        let definition = self
            .store
            .insert_agent_definition(
                &CreateAgentDefinition {
                    name: name.clone(),
                    account_handle: handle,
                    role: command.role.clone(),
                    project_id,
                    model_base_url: self.model_base_url.clone(),
                    model_name: self.model_name.clone(),
                    api_key_env: "OPENAI_API_KEY".to_string(),
                    system_prompt: template.system_prompt.to_string(),
                    tools: template.tools.iter().map(|tool| tool.to_string()).collect(),
                    shell: template.shell,
                    skills: template
                        .skills
                        .iter()
                        .map(|skill| skill.to_string())
                        .collect(),
                    enabled: true,
                },
                account.id,
                &name,
            )
            .await?;
        if let Err(error) = self.supervisor.start(definition.id).await {
            tracing::warn!(
                agent = %definition.name,
                workflow = %workflow_id,
                role = %role_name,
                error = %error,
                "created agent but could not start it"
            );
        }
        Ok(definition)
    }

    async fn assign_task(
        &self,
        actor: Actor,
        command: AssignAgentToTask,
    ) -> Result<(), ServiceError> {
        self.require_admin(&actor).await?;
        let (project_id, _workflow_id, _role_name) = self
            .validate_task_role(command.task_id, &command.role)
            .await?;
        let definition = self
            .store
            .agent_definition(command.agent_definition_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("agent", command.agent_definition_id))?;
        if !definition.enabled {
            return Err(ServiceError::Validation(format!(
                "agent {} is disabled",
                definition.name
            )));
        }
        if definition.role != command.role {
            return Err(ServiceError::Validation(format!(
                "agent {} fills {}, not {}",
                definition.name, definition.role, command.role
            )));
        }
        if definition.project_id != project_id {
            return Err(ServiceError::Validation(format!(
                "agent {} belongs to another project",
                definition.name
            )));
        }
        self.store
            .ensure_project_member(project_id, definition.account_id)
            .await?;
        self.store
            .assign_task(command.task_id, &command.role, definition.id)
            .await?;
        if let Err(error) = self.supervisor.start(definition.id).await {
            tracing::warn!(
                agent = %definition.name,
                error = %error,
                "assigned agent but could not start it"
            );
        }
        Ok(())
    }

    async fn start(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError> {
        self.require_admin(&actor).await?;
        self.supervisor
            .start(id)
            .await
            .map_err(ServiceError::Validation)
    }

    async fn stop(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError> {
        self.require_admin(&actor).await?;
        self.supervisor
            .stop(id)
            .await
            .map_err(ServiceError::Validation)
    }

    async fn restart(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError> {
        self.require_admin(&actor).await?;
        self.supervisor
            .restart(id)
            .await
            .map_err(ServiceError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use super::agent_handle_suffix;

    #[test]
    fn agent_handle_suffix_is_random_in_the_same_time_window() {
        let suffix = agent_handle_suffix();
        assert_eq!(suffix.len(), 8);
        assert!(
            suffix
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        );

        let next = agent_handle_suffix();
        assert_ne!(suffix, next);
    }
}
