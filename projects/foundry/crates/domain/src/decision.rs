use crate::Actor;
use crate::state_machine::StateMachine;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Open,
    Decided,
    Reopened,
    Superseded,
}

impl DecisionStatus {
    pub fn allowed_transitions(self) -> &'static [DecisionStatus] {
        use DecisionStatus::*;
        match self {
            Open => &[Decided, Superseded],
            Decided => &[Reopened, Superseded],
            Reopened => &[Decided, Superseded],
            Superseded => &[],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

impl StateMachine for DecisionStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    Easy,
    Hard,
    Irreversible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionOption {
    pub id: String,
    pub title: String,
    pub tradeoffs: String,
}

/// A decision that needs a human (or an authorized agent) to choose.
///
/// Agents are expected to bring options, criteria, evidence and a
/// recommendation, not an open-ended "what should I do?".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: Uuid,
    pub question: String,
    pub context: String,
    #[serde(default)]
    pub options: Vec<DecisionOption>,
    #[serde(default)]
    pub criteria: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
    pub recommendation: Option<String>,
    pub decision: Option<String>,
    pub authority: Actor,
    pub reversibility: Reversibility,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub deadline: Option<OffsetDateTime>,
    pub status: DecisionStatus,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub decided_at: Option<OffsetDateTime>,
}

impl Decision {
    pub fn new(
        question: impl Into<String>,
        context: impl Into<String>,
        authority: Actor,
        reversibility: Reversibility,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            question: question.into(),
            context: context.into(),
            options: Vec::new(),
            criteria: Vec::new(),
            evidence: Vec::new(),
            recommendation: None,
            decision: None,
            authority,
            reversibility,
            deadline: None,
            status: DecisionStatus::Open,
            created_at: OffsetDateTime::now_utc(),
            decided_at: None,
        }
    }

    pub fn decide(&mut self, choice: impl Into<String>) -> Result<(), DecisionError> {
        if !self.status.can_transition_to(DecisionStatus::Decided) {
            return Err(DecisionError::InvalidStatus(self.status));
        }
        if self.options.is_empty() {
            return Err(DecisionError::NoOptions);
        }
        self.decision = Some(choice.into());
        self.status = DecisionStatus::Decided;
        self.decided_at = Some(OffsetDateTime::now_utc());
        Ok(())
    }

    pub fn reopen(&mut self) -> Result<(), DecisionError> {
        if !self.status.can_transition_to(DecisionStatus::Reopened) {
            return Err(DecisionError::InvalidStatus(self.status));
        }
        self.status = DecisionStatus::Reopened;
        self.decided_at = None;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecisionError {
    #[error("decision cannot be made from status {0:?}")]
    InvalidStatus(DecisionStatus),
    #[error("a decision needs at least one option")]
    NoOptions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_requires_options() {
        let mut decision = Decision::new(
            "退款是否异步？",
            "同步会拖垮主库",
            Actor::human("architect"),
            Reversibility::Easy,
        );
        assert!(decision.decide("异步").is_err());
        decision.options.push(DecisionOption {
            id: "async".to_string(),
            title: "异步".to_string(),
            tradeoffs: "最终一致，需要补偿".to_string(),
        });
        decision.decide("异步").unwrap();
        assert_eq!(decision.status, DecisionStatus::Decided);
    }
}
