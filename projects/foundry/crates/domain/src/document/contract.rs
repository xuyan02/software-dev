use crate::state_machine::StateMachine;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractKind {
    OpenApi,
    Protobuf,
    JsonSchema,
    Database,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractStatus {
    Draft,
    Active,
    Deprecated,
}

impl ContractStatus {
    pub fn allowed_transitions(self) -> &'static [ContractStatus] {
        use ContractStatus::*;
        match self {
            Draft => &[Active, Deprecated],
            Active => &[Deprecated],
            Deprecated => &[],
        }
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

impl StateMachine for ContractStatus {
    fn can_transition_to(self, next: Self) -> bool {
        self.allowed_transitions().contains(&next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compatibility {
    Backward,
    Full,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: Uuid,
    pub name: String,
    pub kind: ContractKind,
    pub version: String,
    pub status: ContractStatus,
    pub compatibility: Compatibility,
    /// Reference to the canonical schema, for example `contracts/payments.yaml#/v9`.
    pub schema_ref: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl Contract {
    pub fn new(
        name: impl Into<String>,
        kind: ContractKind,
        version: impl Into<String>,
        schema_ref: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: name.into(),
            kind,
            version: version.into(),
            status: ContractStatus::Draft,
            compatibility: Compatibility::Backward,
            schema_ref: schema_ref.into(),
            created_at: OffsetDateTime::now_utc(),
        }
    }

    pub fn transition_to(&mut self, next: ContractStatus) -> Result<(), ContractTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(ContractTransitionError::Invalid {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractTransitionError {
    #[error("invalid transition from {from:?} to {to:?}")]
    Invalid {
        from: ContractStatus,
        to: ContractStatus,
    },
}
