use crate::Actor;
use crate::state_machine::StateMachine;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdrStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Deprecated,
}

impl AdrStatus {
    pub fn allowed_transitions(self) -> &'static [AdrStatus] {
        use AdrStatus::*;
        match self {
            Proposed => &[Accepted, Rejected],
            Accepted => &[Superseded, Deprecated],
            Rejected => &[],
            Superseded => &[],
            Deprecated => &[],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

impl StateMachine for AdrStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

/// Architecture Decision Record.
///
/// ADRs are immutable once accepted. Corrections happen by writing a new ADR
/// that supersedes the old one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adr {
    pub id: Uuid,
    pub title: String,
    pub status: AdrStatus,
    pub context: String,
    pub decision: String,
    #[serde(default)]
    pub consequences: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default)]
    pub supersedes: Option<Uuid>,
    pub author: Actor,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl Adr {
    pub fn new(
        title: impl Into<String>,
        context: impl Into<String>,
        decision: impl Into<String>,
        author: Actor,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            status: AdrStatus::Proposed,
            context: context.into(),
            decision: decision.into(),
            consequences: Vec::new(),
            alternatives: Vec::new(),
            supersedes: None,
            author,
            created_at: OffsetDateTime::now_utc(),
        }
    }

    pub fn transition_to(&mut self, next: AdrStatus) -> Result<(), AdrTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(AdrTransitionError::Invalid {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdrTransitionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    Invalid { from: AdrStatus, to: AdrStatus },
}
