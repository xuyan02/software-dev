//! Project-scoped triggers. A trigger creates a Task either on a schedule or
//! when a user runs it manually.

use async_trait::async_trait;
use foundry_domain::{
    Actor, ProjectRole, Trigger, TriggerRun, TriggerRunReason, TriggerSchedule,
};
use serde_json::{Value, json};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    AccountRepository, NewTask, ProjectRepository, RepoError, ServiceError, TaskUseCases,
};

/// Workflows that a trigger may start. Triggers are intentionally limited to
/// known inspection/quality tasks.
pub const SCHEDULABLE_WORKFLOWS: &[&str] = &[
    "style_inspection",
    "logic_inspection",
    "cohesion_inspection",
    "consistency_inspection",
    "design_inspection",
    "design_consistency_maintenance",
];

#[derive(Debug, Clone)]
pub struct CreateTrigger {
    pub project_id: Uuid,
    pub name: String,
    pub description: String,
    pub workflow_id: String,
    pub input: Value,
    pub schedule: String,
    pub title_template: String,
    pub description_template: String,
    pub created_by_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct UpdateTrigger {
    pub project_id: Uuid,
    pub id: Uuid,
    pub name: Option<String>,
    pub description: Option<String>,
    pub input: Option<Value>,
    pub schedule: Option<String>,
    pub title_template: Option<String>,
    pub description_template: Option<String>,
    pub enabled: Option<bool>,
}

#[async_trait]
pub trait TriggerRepository: Send + Sync {
    async fn insert(&self, trigger: &Trigger) -> Result<(), RepoError>;
    async fn save(&self, trigger: &Trigger, expected_version: u32) -> Result<(), RepoError>;
    async fn get(&self, project_id: Uuid, id: Uuid) -> Result<Option<Trigger>, RepoError>;
    async fn list(&self, project_id: Uuid) -> Result<Vec<Trigger>, RepoError>;
    async fn delete(
        &self,
        project_id: Uuid,
        id: Uuid,
        expected_version: u32,
    ) -> Result<(), RepoError>;
    async fn list_due(&self, now: OffsetDateTime) -> Result<Vec<Trigger>, RepoError>;
    async fn insert_run(&self, run: &TriggerRun) -> Result<(), RepoError>;
    async fn runs(
        &self,
        project_id: Uuid,
        trigger_id: Uuid,
    ) -> Result<Vec<TriggerRun>, RepoError>;
    async fn has_active_run(&self, trigger_id: Uuid) -> Result<bool, RepoError>;
}

#[async_trait]
pub trait TriggerUseCases: Send + Sync {
    async fn list(&self, actor: Actor, project_id: Uuid) -> Result<Vec<Trigger>, ServiceError>;
    async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Trigger>, ServiceError>;
    async fn create(
        &self,
        actor: Actor,
        command: CreateTrigger,
    ) -> Result<Trigger, ServiceError>;
    async fn update(
        &self,
        actor: Actor,
        command: UpdateTrigger,
    ) -> Result<Trigger, ServiceError>;
    async fn delete(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<(), ServiceError>;
    async fn run(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<TriggerRun, ServiceError>;
    async fn runs(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Vec<TriggerRun>, ServiceError>;
}

pub struct TriggerService<TR, P, A, T> {
    triggers: Arc<TR>,
    projects: Arc<P>,
    accounts: Arc<A>,
    tasks: Arc<T>,
}

impl<TR, P, A, T> TriggerService<TR, P, A, T>
where
    TR: TriggerRepository,
    P: ProjectRepository,
    A: AccountRepository,
    T: TaskUseCases,
{
    pub fn new(
        triggers: Arc<TR>,
        projects: Arc<P>,
        accounts: Arc<A>,
        tasks: Arc<T>,
    ) -> Self {
        Self {
            triggers,
            projects,
            accounts,
            tasks,
        }
    }

    async fn require_role(
        &self,
        actor: &Actor,
        project_id: Uuid,
        minimum: ProjectRole,
    ) -> Result<(), ServiceError> {
        let account = self
            .accounts
            .get_by_handle(actor.id())
            .await?
            .ok_or_else(|| ServiceError::Unauthorized("unknown actor".to_string()))?;
        let project = self
            .projects
            .get(project_id)
            .await?
            .ok_or_else(|| ServiceError::not_found("project", project_id))?;
        if account.is_admin() {
            return Ok(());
        }
        let role = project
            .role_of(account.id)
            .ok_or_else(|| ServiceError::Forbidden("not a project member".to_string()))?;
        if role < minimum {
            return Err(ServiceError::Forbidden(
                "project role does not allow this trigger action".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn list(
        &self,
        actor: Actor,
        project_id: Uuid,
    ) -> Result<Vec<Trigger>, ServiceError> {
        self.require_role(&actor, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.triggers.list(project_id).await?)
    }

    pub async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Trigger>, ServiceError> {
        self.require_role(&actor, project_id, ProjectRole::Viewer)
            .await?;
        Ok(self.triggers.get(project_id, id).await?)
    }

    pub async fn create(
        &self,
        actor: Actor,
        command: CreateTrigger,
    ) -> Result<Trigger, ServiceError> {
        self.require_role(&actor, command.project_id, ProjectRole::Maintainer)
            .await?;
        if !SCHEDULABLE_WORKFLOWS.contains(&command.workflow_id.as_str()) {
            return Err(ServiceError::Validation(format!(
                "workflow cannot be triggered: {}",
                command.workflow_id
            )));
        }
        let input = if command.input.is_null() {
            json!({})
        } else {
            command.input
        };
        let trigger = Trigger::new(
            command.project_id,
            command.name,
            command.description,
            command.workflow_id,
            input,
            command.schedule,
            command.title_template,
            if command.description_template.trim().is_empty() {
                "由触发器 {trigger} 启动。".to_string()
            } else {
                command.description_template
            },
            actor,
            command.created_by_id,
        )
        .map_err(|error| ServiceError::Validation(error.to_string()))?;
        self.triggers.insert(&trigger).await?;
        Ok(trigger)
    }

    pub async fn update(
        &self,
        actor: Actor,
        command: UpdateTrigger,
    ) -> Result<Trigger, ServiceError> {
        self.require_role(&actor, command.project_id, ProjectRole::Maintainer)
            .await?;
        let mut trigger = self
            .triggers
            .get(command.project_id, command.id)
            .await?
            .ok_or_else(|| ServiceError::not_found("trigger", command.id))?;
        let previous = trigger.version;
        if let Some(name) = command.name {
            trigger.name = name;
        }
        if let Some(description) = command.description {
            trigger.description = description;
        }
        if let Some(input) = command.input {
            trigger.input = input;
        }
        if let Some(schedule) = command.schedule {
            trigger.schedule = schedule;
            trigger.next_run_at = TriggerSchedule::parse(&trigger.schedule)
                .and_then(|schedule| schedule.next_after(OffsetDateTime::now_utc()))
                .map_err(|error| ServiceError::Validation(error.to_string()))?;
        }
        if let Some(title) = command.title_template {
            trigger.title_template = title;
        }
        if let Some(description) = command.description_template {
            trigger.description_template = description;
        }
        if let Some(enabled) = command.enabled {
            trigger.enabled = enabled;
        }
        trigger
            .validate()
            .map_err(|error| ServiceError::Validation(error.to_string()))?;
        trigger.touch();
        self.triggers.save(&trigger, previous).await?;
        Ok(trigger)
    }

    pub async fn delete(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<(), ServiceError> {
        self.require_role(&actor, project_id, ProjectRole::Maintainer)
            .await?;
        let trigger = self
            .triggers
            .get(project_id, id)
            .await?
            .ok_or_else(|| ServiceError::not_found("trigger", id))?;
        self.triggers
            .delete(project_id, id, trigger.version)
            .await?;
        Ok(())
    }

    pub async fn run(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<TriggerRun, ServiceError> {
        self.require_role(&actor, project_id, ProjectRole::Member)
            .await?;
        self.run_trigger(project_id, id, TriggerRunReason::Manual, actor)
            .await
    }

    pub async fn runs(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Vec<TriggerRun>, ServiceError> {
        self.require_role(&actor, project_id, ProjectRole::Viewer)
            .await?;
        self.triggers
            .runs(project_id, id)
            .await
            .map_err(ServiceError::from)
    }

    /// Run every enabled trigger whose `next_run_at` is due. The scheduler
    /// calls this sequentially; no run overlaps.
    pub async fn run_due(&self, now: OffsetDateTime) -> Result<usize, ServiceError> {
        let due = self.triggers.list_due(now).await?;
        let mut started = 0;
        for trigger in due {
            if self
                .triggers
                .has_active_run(trigger.id)
                .await
                .unwrap_or(true)
            {
                continue;
            }
            let actor = Actor::System;
            if self
                .run_trigger(
                    trigger.project_id,
                    trigger.id,
                    TriggerRunReason::Scheduled,
                    actor,
                )
                .await
                .is_ok()
            {
                started += 1;
            }
        }
        Ok(started)
    }

    async fn run_trigger(
        &self,
        project_id: Uuid,
        id: Uuid,
        reason: TriggerRunReason,
        triggered_by: Actor,
    ) -> Result<TriggerRun, ServiceError> {
        let mut trigger = self
            .triggers
            .get(project_id, id)
            .await?
            .ok_or_else(|| ServiceError::not_found("trigger", id))?;
        if !trigger.enabled && reason == TriggerRunReason::Scheduled {
            return Err(ServiceError::Validation(
                "trigger is disabled".to_string(),
            ));
        }
        if self.triggers.has_active_run(trigger.id).await? {
            return Err(ServiceError::Validation(
                "上一轮触发器任务还没有结束".to_string(),
            ));
        }
        let mut input = trigger.input.clone();
        if let Some(object) = input.as_object_mut() {
            object.insert("trigger_id".to_string(), json!(trigger.id));
            object.insert("trigger_name".to_string(), json!(trigger.name));
            object.insert("trigger_reason".to_string(), json!(reason.as_str()));
        }
        let task = self
            .tasks
            .create_task(
                project_id,
                NewTask {
                    workflow: trigger.workflow_id.clone(),
                    title: render_template(&trigger.title_template, &trigger),
                    description: render_template(&trigger.description_template, &trigger),
                    input,
                },
                trigger.created_by_id,
            )
            .await?;
        let run = TriggerRun {
            id: Uuid::now_v7(),
            trigger_id: trigger.id,
            task_id: task.id,
            reason,
            triggered_by,
            created_at: OffsetDateTime::now_utc(),
        };
        self.triggers.insert_run(&run).await?;
        trigger.last_run_at = Some(run.created_at);
        if reason == TriggerRunReason::Scheduled {
            trigger.next_run_at = TriggerSchedule::parse(&trigger.schedule)
                .and_then(|schedule| schedule.next_after(run.created_at))
                .map_err(|error| ServiceError::Validation(error.to_string()))?;
        }
        trigger.touch();
        self.triggers.save(&trigger, trigger.version.saturating_sub(1)).await?;
        Ok(run)
    }
}

fn render_template(template: &str, trigger: &Trigger) -> String {
    template
        .replace("{trigger}", &trigger.name)
        .replace("{workflow}", &trigger.workflow_id)
        .replace("{project}", &trigger.project_id.to_string())
}


#[async_trait]
impl<TR, P, A, T> TriggerUseCases for TriggerService<TR, P, A, T>
where
    TR: TriggerRepository,
    P: ProjectRepository,
    A: AccountRepository,
    T: TaskUseCases,
{
    async fn list(&self, actor: Actor, project_id: Uuid) -> Result<Vec<Trigger>, ServiceError> {
        TriggerService::list(self, actor, project_id).await
    }

    async fn get(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Trigger>, ServiceError> {
        TriggerService::get(self, actor, project_id, id).await
    }

    async fn create(
        &self,
        actor: Actor,
        command: CreateTrigger,
    ) -> Result<Trigger, ServiceError> {
        TriggerService::create(self, actor, command).await
    }

    async fn update(
        &self,
        actor: Actor,
        command: UpdateTrigger,
    ) -> Result<Trigger, ServiceError> {
        TriggerService::update(self, actor, command).await
    }

    async fn delete(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<(), ServiceError> {
        TriggerService::delete(self, actor, project_id, id).await
    }

    async fn run(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<TriggerRun, ServiceError> {
        TriggerService::run(self, actor, project_id, id).await
    }

    async fn runs(
        &self,
        actor: Actor,
        project_id: Uuid,
        id: Uuid,
    ) -> Result<Vec<TriggerRun>, ServiceError> {
        TriggerService::runs(self, actor, project_id, id).await
    }
}
