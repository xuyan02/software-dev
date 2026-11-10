//! Hard limits for one run.

use std::time::Duration;

use crate::Usage;

#[derive(Debug, Clone)]
pub struct Budget {
    pub max_steps: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_tool_calls: u32,
    pub max_duration: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_steps: 20,
            max_input_tokens: 200_000,
            max_output_tokens: 50_000,
            max_tool_calls: 50,
            max_duration: Duration::from_secs(600),
        }
    }
}

impl Budget {
    pub fn small() -> Self {
        Self {
            max_steps: 6,
            max_input_tokens: 20_000,
            max_output_tokens: 5_000,
            max_tool_calls: 5,
            max_duration: Duration::from_secs(60),
        }
    }

    pub fn tokens_exceeded(&self, usage: &Usage) -> bool {
        usage.input_tokens > self.max_input_tokens || usage.output_tokens > self.max_output_tokens
    }
}
