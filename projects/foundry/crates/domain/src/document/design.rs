use crate::state_machine::StateMachine;
use crate::{Actor, Link};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesignStatus {
    Draft,
    InReview,
    Accepted,
    Superseded,
}

impl DesignStatus {
    pub fn allowed_transitions(self) -> &'static [DesignStatus] {
        use DesignStatus::*;
        match self {
            Draft => &[InReview, Superseded],
            InReview => &[Accepted, Draft, Superseded],
            Accepted => &[Superseded],
            Superseded => &[],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

impl StateMachine for DesignStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignOption {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignSections {
    pub context: String,
    #[serde(default)]
    pub goals: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub options: Vec<DesignOption>,
    pub tradeoffs: String,
    pub decision: String,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub failure_modes: Vec<String>,
    pub rollout: String,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignDoc {
    pub id: Uuid,
    pub title: String,
    #[serde(default)]
    pub requirement_ids: Vec<Uuid>,
    pub status: DesignStatus,
    #[serde(default)]
    pub authors: Vec<Actor>,
    #[serde(default)]
    pub reviewers: Vec<Actor>,
    pub sections: DesignSections,
    #[serde(default)]
    pub links: Vec<Link>,
    pub version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

impl DesignDoc {
    pub fn new(
        title: impl Into<String>,
        requirement_ids: Vec<Uuid>,
        author: Actor,
        sections: DesignSections,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id: Uuid::now_v7(),
            title: title.into(),
            requirement_ids,
            status: DesignStatus::Draft,
            authors: vec![author],
            reviewers: Vec::new(),
            sections,
            links: Vec::new(),
            version: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// A design cannot be accepted while it still has unresolved questions.
    pub fn validate_for_acceptance(&self) -> Result<(), DesignValidationError> {
        let mut errors = Vec::new();
        if self.sections.context.trim().is_empty() {
            errors.push("context is required".to_string());
        }
        if self.sections.decision.trim().is_empty() {
            errors.push("decision is required".to_string());
        }
        if self.sections.tradeoffs.trim().is_empty() {
            errors.push("tradeoffs are required".to_string());
        }
        if !self.sections.open_questions.is_empty() {
            errors.push("all open questions must be resolved before acceptance".to_string());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(DesignValidationError(errors))
        }
    }

    pub fn transition_to(&mut self, next: DesignStatus) -> Result<(), DesignTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(DesignTransitionError::Invalid {
                from: self.status,
                to: next,
            });
        }
        if next == DesignStatus::Accepted {
            self.validate_for_acceptance()
                .map_err(|error| DesignTransitionError::NotReady(error.to_string()))?;
        }
        self.status = next;
        self.version = self.version.saturating_add(1);
        self.updated_at = OffsetDateTime::now_utc();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("design validation failed: {}", .0.join("; "))]
pub struct DesignValidationError(pub Vec<String>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DesignTransitionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    Invalid {
        from: DesignStatus,
        to: DesignStatus,
    },
    #[error("design not ready: {0}")]
    NotReady(String),
}
