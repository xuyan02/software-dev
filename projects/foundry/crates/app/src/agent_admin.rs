//! Agent management use cases.
//!
//! Agents are platform-defined. This surface only exposes runtime management:
//! overview and process lifecycle. Users do not create or configure agents.

use crate::ServiceError;
use foundry_domain::{
    Actor, AgentDefinition, AgentOverview, AgentTemplateView, AssignAgentToTask,
    CreateAgentFromTemplate, ReadyTaskView,
};
use uuid::Uuid;

#[async_trait::async_trait]
pub trait AgentAdminUseCases: Send + Sync {
    async fn overview(&self, actor: Actor) -> Result<AgentOverview, ServiceError>;
    async fn agents(&self, actor: Actor) -> Result<Vec<AgentDefinition>, ServiceError>;
    async fn ready_tasks(&self, actor: Actor) -> Result<Vec<ReadyTaskView>, ServiceError>;
    async fn templates(
        &self,
        actor: Actor,
        role: Option<String>,
    ) -> Result<Vec<AgentTemplateView>, ServiceError>;
    async fn create_from_template(
        &self,
        actor: Actor,
        command: CreateAgentFromTemplate,
    ) -> Result<AgentDefinition, ServiceError>;
    async fn assign_task(
        &self,
        actor: Actor,
        command: AssignAgentToTask,
    ) -> Result<(), ServiceError>;
    async fn start(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError>;
    async fn stop(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError>;
    async fn restart(&self, actor: Actor, id: Uuid) -> Result<(), ServiceError>;
}
