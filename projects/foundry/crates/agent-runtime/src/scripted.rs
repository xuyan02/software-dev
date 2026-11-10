//! A deterministic model for tests and the Phase 0 CLI.

use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

use crate::{ModelError, ModelGateway, ModelRequest, ModelResponse};

pub struct ScriptedModel {
    responses: Mutex<VecDeque<ModelResponse>>,
    requests: Mutex<Vec<ModelRequest>>,
}

impl ScriptedModel {
    pub fn new(responses: Vec<ModelResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into()),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn requests(&self) -> Vec<ModelRequest> {
        self.requests.lock().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl ModelGateway for ScriptedModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        self.requests.lock().expect("lock poisoned").push(request);
        self.responses
            .lock()
            .expect("lock poisoned")
            .pop_front()
            .ok_or(ModelError::Exhausted)
    }
}
