//! Hard limits for one run.

use std::time::Duration;

use crate::Usage;

#[derive(Debug, Clone, Default)]
pub struct Budget {
    /// `None` means the run has no step limit.
    pub max_steps: Option<u32>,
    /// `None` means the run has no input-token limit.
    pub max_input_tokens: Option<u64>,
    /// `None` means the run has no output-token limit.
    pub max_output_tokens: Option<u64>,
    /// `None` means the run has no tool-call limit.
    pub max_tool_calls: Option<u32>,
    /// `None` means the run has no duration limit.
    pub max_duration: Option<Duration>,
}

impl Budget {
    pub fn small() -> Self {
        Self {
            max_steps: Some(6),
            max_input_tokens: Some(20_000),
            max_output_tokens: Some(5_000),
            max_tool_calls: Some(5),
            max_duration: Some(Duration::from_secs(60)),
        }
    }

    pub fn tokens_exceeded(&self, usage: &Usage) -> bool {
        self.max_input_tokens
            .is_some_and(|max| usage.input_tokens > max)
            || self
                .max_output_tokens
                .is_some_and(|max| usage.output_tokens > max)
    }
}
