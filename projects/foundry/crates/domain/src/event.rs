use crate::Actor;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainEventKind {
    AccountCreated,
    AccountUpdated,
    ProjectCreated,
    ProjectUpdated,
    ProjectMemberChanged,
    RequirementCreated,
    RequirementUpdated,
    RequirementTransitioned,
    IssueCreated,
    IssueUpdated,
    IssueCommented,
    IssueTransitioned,
    DesignCreated,
    DesignTransitioned,
    DecisionOpened,
    DecisionDecided,
    TaskCreated,
    TaskStateChanged,
    TaskStageChanged,
    TaskCompleted,
    ChildCreated,
    ChildStateChanged,
    OperationReceived,
    ActorBound,
    ActorActivated,
    ActorDeactivated,
    DispatchRequested,
    TaskReevaluated,
    WorkflowDefinitionCreated,
    WorkflowTitleChanged,
    WorkflowVersionPublished,
    RequirementsDocumentCreated,
    RequirementsDraftSaved,
    RequirementsVersionPublished,
    ProjectBriefDrafted,
    ProjectBriefSubmitted,
    ProjectBriefApproved,
    ProjectBriefChangesRequested,
    TaskLinkAdded,
    TaskLinkRemoved,
}

/// An immutable fact that something happened.
///
/// Events are appended to the log, never mutated. They are the durable record
/// of accepted inputs and effects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEvent {
    pub id: Uuid,
    pub kind: DomainEventKind,
    pub aggregate_id: Uuid,
    pub actor: Actor,
    pub payload: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub occurred_at: OffsetDateTime,
}

impl DomainEvent {
    pub fn new(
        kind: DomainEventKind,
        aggregate_id: Uuid,
        actor: Actor,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind,
            aggregate_id,
            actor,
            payload,
            occurred_at: OffsetDateTime::now_utc(),
        }
    }
}
