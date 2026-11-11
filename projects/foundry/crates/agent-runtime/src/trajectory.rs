//! Step-by-step trajectory.
//!
//! Every model call, tool call and final answer is recorded. The in-memory sink
//! is used in tests and Phase 0; Postgres persistence comes later.

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;

use crate::{TrajectoryError, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    Model,
    Tool,
    Final,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StepRecord {
    pub index: u32,
    pub kind: StepKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Provider-native reasoning for this step, when the model returns it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub usage: Usage,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[async_trait]
pub trait TrajectorySink: Send + Sync {
    async fn record(&self, step: &StepRecord) -> Result<(), TrajectoryError>;
}

#[derive(Default)]
pub struct InMemoryTrajectory {
    steps: Mutex<Vec<StepRecord>>,
}

impl InMemoryTrajectory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn steps(&self) -> Vec<StepRecord> {
        self.steps.lock().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl TrajectorySink for InMemoryTrajectory {
    async fn record(&self, step: &StepRecord) -> Result<(), TrajectoryError> {
        self.steps.lock().expect("lock poisoned").push(step.clone());
        Ok(())
    }
}
