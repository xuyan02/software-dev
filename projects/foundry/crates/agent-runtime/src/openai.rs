//! OpenAI-compatible chat completions provider.
//!
//! Covers OpenAI, DeepSeek, Ollama, vLLM and any other server exposing
//! `/chat/completions` with function calling.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

use crate::{
    Message, MessageRole, ModelError, ModelGateway, ModelRequest, ModelResponse, ModelStopReason,
    ToolCall, ToolSpec, Usage,
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub timeout: Duration,
    pub max_retries: u32,
    /// Enable provider-native reasoning for DeepSeek-compatible endpoints.
    pub thinking: bool,
    /// DeepSeek reasoning effort: "low", "high" or "max".
    pub reasoning_effort: Option<String>,
}

impl OpenAiCompatibleConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
            timeout: Duration::from_secs(120),
            max_retries: 2,
            thinking: true,
            reasoning_effort: Some("high".to_string()),
        }
    }
}

pub struct OpenAiCompatibleModel {
    client: reqwest::Client,
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleModel {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, ModelError> {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|error| ModelError::Backend(error.to_string()))?;
        Ok(Self { client, config })
    }
}

#[async_trait]
impl ModelGateway for OpenAiCompatibleModel {
    async fn complete(&self, request: ModelRequest) -> Result<ModelResponse, ModelError> {
        let body = request_body(&self.config, &request);
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            let response = self
                .client
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(&body)
                .send()
                .await;
            match response {
                Ok(response) => {
                    let status = response.status();
                    let text = response
                        .text()
                        .await
                        .map_err(|error| ModelError::Backend(error.to_string()))?;
                    if status.is_success() {
                        return parse_response(&text);
                    }
                    last_error = Some(format!("HTTP {status}: {text}"));
                    if (status.as_u16() == 429 || status.is_server_error())
                        && attempt < self.config.max_retries
                    {
                        sleep_backoff(attempt).await;
                        continue;
                    }
                    return Err(ModelError::Backend(
                        last_error.unwrap_or_else(|| format!("HTTP {status}")),
                    ));
                }
                Err(error) => {
                    last_error = Some(error.to_string());
                    if attempt < self.config.max_retries {
                        sleep_backoff(attempt).await;
                        continue;
                    }
                }
            }
        }

        Err(ModelError::Backend(
            last_error.unwrap_or_else(|| "request failed".to_string()),
        ))
    }
}

async fn sleep_backoff(attempt: u32) {
    let millis = 250u64 * 2u64.pow(attempt.min(4));
    tokio::time::sleep(Duration::from_millis(millis)).await;
}

fn request_body(config: &OpenAiCompatibleConfig, request: &ModelRequest) -> Value {
    let messages: Vec<Value> = request.messages.iter().map(message_json).collect();
    let mut body = json!({ "model": config.model, "messages": messages });
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools.iter().map(tool_json).collect::<Vec<_>>());
    }
    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if is_deepseek(&config.base_url, &config.model) {
        body["thinking"] = json!({
            "type": if config.thinking { "enabled" } else { "disabled" }
        });
        if config.thinking
            && let Some(effort) = &config.reasoning_effort
        {
            body["reasoning_effort"] = json!(effort);
        }
    }
    body
}

fn is_deepseek(base_url: &str, model: &str) -> bool {
    let base_url = base_url.to_ascii_lowercase();
    if base_url.contains("deepseek") {
        return true;
    }
    let model = model.to_ascii_lowercase();
    model == "deepseek-chat"
        || model == "deepseek-reasoner"
        || model == "deepseek-flash"
        || model.starts_with("deepseek-v")
}

fn message_json(message: &Message) -> Value {
    match message.role {
        MessageRole::System => json!({ "role": "system", "content": message.content }),
        MessageRole::User => json!({ "role": "user", "content": message.content }),
        MessageRole::Assistant => {
            let mut value = json!({ "role": "assistant", "content": message.content });
            if let Some(reasoning_content) = &message.reasoning_content {
                value["reasoning_content"] = json!(reasoning_content);
            }
            if !message.tool_calls.is_empty() {
                let calls: Vec<Value> = message
                    .tool_calls
                    .iter()
                    .map(|call| {
                        json!({
                            "id": call.id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": serde_json::to_string(&call.arguments)
                                    .unwrap_or_else(|_| "{}".to_string())
                            }
                        })
                    })
                    .collect();
                value["tool_calls"] = json!(calls);
            }
            value
        }
        MessageRole::Tool => json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id.clone().unwrap_or_default(),
            "content": message.content
        }),
    }
}

fn tool_json(spec: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.input_schema
        }
    })
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<UsageJson>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ChatToolCall>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCall {
    id: String,
    function: ChatFunction,
}

#[derive(Debug, Deserialize)]
struct ChatFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct UsageJson {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

fn parse_response(text: &str) -> Result<ModelResponse, ModelError> {
    let response: ChatResponse = serde_json::from_str(text)
        .map_err(|error| ModelError::InvalidResponse(error.to_string()))?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ModelError::InvalidResponse("response has no choices".to_string()))?;
    let mut tool_calls = Vec::new();
    for call in choice.message.tool_calls {
        let arguments = if call.function.arguments.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(&call.function.arguments).map_err(|error| {
                ModelError::InvalidResponse(format!(
                    "tool call {} has invalid arguments: {error}",
                    call.function.name
                ))
            })?
        };
        tool_calls.push(ToolCall {
            id: call.id,
            name: call.function.name,
            arguments,
        });
    }
    let stop_reason = if !tool_calls.is_empty() {
        ModelStopReason::ToolCalls
    } else {
        match choice.finish_reason.as_deref() {
            Some("tool_calls") => ModelStopReason::ToolCalls,
            Some("length") => ModelStopReason::Length,
            Some("stop") | None => ModelStopReason::Stop,
            Some(_) => ModelStopReason::Other,
        }
    };
    Ok(ModelResponse {
        content: choice.message.content,
        reasoning_content: choice.message.reasoning_content,
        tool_calls,
        usage: response
            .usage
            .map(|usage| Usage::new(usage.prompt_tokens, usage.completion_tokens))
            .unwrap_or_default(),
        stop_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::Json;
    use axum::routing::post;
    use std::sync::{Arc, Mutex};

    async fn mock_server(response: Value) -> (String, Arc<Mutex<Vec<Value>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let state = captured.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move |Json(body): Json<Value>| {
                let state = state.clone();
                async move {
                    state.lock().expect("lock poisoned").push(body);
                    Json(response)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}"), captured)
    }

    fn request() -> ModelRequest {
        ModelRequest {
            messages: vec![Message::system("system"), Message::user("hello")],
            tools: vec![ToolSpec {
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: json!({ "type": "object" }),
            }],
            max_tokens: Some(100),
            temperature: Some(0.2),
        }
    }

    #[tokio::test]
    async fn parses_content_and_usage() {
        let (base_url, captured) = mock_server(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 4 }
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "test-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(response.content.as_deref(), Some("done"));
        assert_eq!(response.usage, Usage::new(10, 4));
        assert_eq!(response.stop_reason, ModelStopReason::Stop);

        let body = captured.lock().unwrap().first().cloned().unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["tools"][0]["function"]["name"], "echo");
        assert!(body.get("thinking").is_none());
    }

    #[tokio::test]
    async fn deepseek_requests_enable_thinking() {
        let (base_url, captured) = mock_server(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "deepseek-chat",
        ))
        .unwrap();
        model.complete(request()).await.unwrap();

        let body = captured.lock().unwrap().first().cloned().unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[tokio::test]
    async fn parses_reasoning_content() {
        let (base_url, _captured) = mock_server(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "done",
                    "reasoning_content": "I should answer briefly."
                },
                "finish_reason": "stop"
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "deepseek-chat",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(
            response.reasoning_content.as_deref(),
            Some("I should answer briefly.")
        );
    }

    #[tokio::test]
    async fn sends_prior_reasoning_content_back() {
        let (base_url, captured) = mock_server(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "done" },
                "finish_reason": "stop"
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "deepseek-chat",
        ))
        .unwrap();
        let mut request = request();
        request.messages.push(Message::assistant_with_reasoning(
            "working",
            Vec::new(),
            Some("I need to inspect it.".to_string()),
        ));
        model.complete(request).await.unwrap();

        let body = captured.lock().unwrap().first().cloned().unwrap();
        assert_eq!(
            body["messages"][2]["reasoning_content"],
            "I need to inspect it."
        );
    }

    #[tokio::test]
    async fn parses_tool_calls() {
        let (base_url, _captured) = mock_server(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": { "name": "echo", "arguments": "{\"text\":\"hi\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 2 }
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "test-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(response.stop_reason, ModelStopReason::ToolCalls);
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "echo");
        assert_eq!(response.tool_calls[0].arguments, json!({ "text": "hi" }));
    }
}
