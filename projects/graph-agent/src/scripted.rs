//! Deterministic model for tests and local demos.

use crate::{LlmRequest, LlmResponse, ModelError, ModelGateway};
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct ScriptedModel {
    responses: Mutex<VecDeque<LlmResponse>>,
    requests: Mutex<Vec<LlmRequest>>,
    context_window: Option<usize>,
}

impl ScriptedModel {
    pub fn new(responses: Vec<LlmResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
            context_window: None,
        }
    }

    pub fn with_context_window(mut self, context_window: usize) -> Self {
        self.context_window = Some(context_window);
        self
    }

    pub fn requests(&self) -> Vec<LlmRequest> {
        self.requests.lock().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl ModelGateway for ScriptedModel {
    fn context_window(&self) -> Option<usize> {
        self.context_window
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError> {
        self.requests.lock().expect("lock poisoned").push(request);
        self.responses
            .lock()
            .expect("lock poisoned")
            .pop_front()
            .ok_or(ModelError::Exhausted)
    }
}
