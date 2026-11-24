//! OpenAI-compatible chat completions provider.
//!
//! Covers OpenAI, DeepSeek, Ollama, vLLM and any other server exposing
//! `/chat/completions` with function calling.

use crate::llm::{escape_control_characters, truncate_for_error};
use crate::{
    LlmMessage, LlmRequest, LlmResponse, LlmRole, LlmToolCall, ModelError, ModelGateway,
    ToolDefinition, Usage,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// Prompt+completion context supported by the model.
    pub context_window: Option<usize>,
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
        let model = model.into();
        let context_window = context_window_for_model(&model);
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            model,
            context_window,
            timeout: Duration::from_secs(120),
            max_retries: 2,
            thinking: true,
            reasoning_effort: Some("high".to_string()),
        }
    }

    /// Override the inferred context window for a model.
    pub fn with_context_window(mut self, context_window: usize) -> Self {
        self.context_window = Some(context_window);
        self
    }
}

/// Context window of known model families. Unknown models get a conservative
/// default; callers can override it on [`OpenAiCompatibleConfig`].
pub fn context_window_for_model(model: &str) -> Option<usize> {
    let model = model.to_ascii_lowercase();
    if model.contains("deepseek") {
        Some(1_048_576)
    } else if model.contains("gpt-4.1") {
        Some(1_047_576)
    } else if model.contains("gpt-4o") {
        Some(128_000)
    } else if model.contains("claude") {
        Some(200_000)
    } else if model.contains("qwen") {
        Some(131_072)
    } else {
        Some(128_000)
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
    fn context_window(&self) -> Option<usize> {
        self.config.context_window
    }

    async fn complete(&self, request: LlmRequest) -> Result<LlmResponse, ModelError> {
        let body = request_body(&self.config, &request);
        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut rate_limit_attempt = 0u32;
        let mut transient_attempt = 0u32;

        loop {
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
                    if status.is_success() {
                        let text = response
                            .text()
                            .await
                            .map_err(|error| ModelError::Backend(error.to_string()))?;
                        let model = if request.model.as_str().is_empty() {
                            self.config.model.as_str()
                        } else {
                            request.model.as_str()
                        };
                        return parse_response(&text, model);
                    }
                    if status.as_u16() == 429 {
                        // A rate limit is not a terminal failure. Wait and
                        // retry until the provider admits the request; the
                        // outer agent/task lifecycle has no useful retry point
                        // for a 429. Never turn this response - including a
                        // dropped or invalid body - into an error.
                        let delay = retry_after_delay(response.headers())
                            .unwrap_or_else(|| backoff(rate_limit_attempt));
                        let _ = response.bytes().await;
                        tracing::warn!(
                            attempt = rate_limit_attempt + 1,
                            delay_ms = delay.as_millis(),
                            "LLM rate limited (429); retrying"
                        );
                        tokio::time::sleep(delay).await;
                        rate_limit_attempt = rate_limit_attempt.saturating_add(1);
                        continue;
                    }
                    let text = response
                        .text()
                        .await
                        .map_err(|error| ModelError::Backend(error.to_string()))?;
                    let message = format!("HTTP {status}: {text}");
                    if status.is_server_error() && transient_attempt < self.config.max_retries {
                        sleep_backoff(transient_attempt).await;
                        transient_attempt = transient_attempt.saturating_add(1);
                        continue;
                    }
                    return Err(ModelError::Backend(message));
                }
                Err(error) => {
                    let message = error.to_string();
                    if transient_attempt < self.config.max_retries {
                        sleep_backoff(transient_attempt).await;
                        transient_attempt = transient_attempt.saturating_add(1);
                        continue;
                    }
                    return Err(ModelError::Backend(message));
                }
            }
        }
    }
}

fn backoff(attempt: u32) -> Duration {
    let millis = 250u64.saturating_mul(1u64 << attempt.min(7)).min(60_000);
    Duration::from_millis(millis)
}

fn retry_after_delay(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

async fn sleep_backoff(attempt: u32) {
    tokio::time::sleep(backoff(attempt)).await;
}

fn request_body(config: &OpenAiCompatibleConfig, request: &LlmRequest) -> Value {
    let model = if request.model.as_str().is_empty() {
        config.model.as_str()
    } else {
        request.model.as_str()
    };
    let messages: Vec<Value> = request.messages.iter().map(message_json).collect();
    let mut body = json!({ "model": model, "messages": messages });
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools.iter().map(tool_json).collect::<Vec<_>>());
    }
    if let Some(temperature) = request.params.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = request.params.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if is_deepseek(&config.base_url, model) {
        body["thinking"] = json!({
            "type": if config.thinking { "enabled" } else { "disabled" }
        });
        body["response_format"] = json!({ "type": "json_object" });
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

fn message_json(message: &LlmMessage) -> Value {
    match message.role {
        LlmRole::System => json!({ "role": "system", "content": message.content }),
        LlmRole::User => {
            if message.images.is_empty() {
                json!({ "role": "user", "content": message.content })
            } else {
                let mut parts = vec![json!({
                    "type": "text",
                    "text": message.content,
                })];
                for image in &message.images {
                    parts.push(json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!(
                                "data:{};base64,{}",
                                image.media_type, image.data_base64
                            )
                        }
                    }));
                }
                json!({ "role": "user", "content": parts })
            }
        }
        LlmRole::Assistant => {
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
        LlmRole::Tool => json!({
            "role": "tool",
            "tool_call_id": message.tool_call_id.clone().unwrap_or_default(),
            "content": message.content
        }),
    }
}

fn tool_json(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema
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

fn parse_response(text: &str, model: &str) -> Result<LlmResponse, ModelError> {
    let response: ChatResponse = serde_json::from_str(text)
        .map_err(|error| ModelError::InvalidResponse(error.to_string()))?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| ModelError::InvalidResponse("response has no choices".to_string()))?;
    let mut tool_calls = Vec::new();
    for call in choice.message.tool_calls {
        let mut argument_error = None;
        let arguments = if call.function.arguments.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            match serde_json::from_str(&call.function.arguments) {
                Ok(arguments) => arguments,
                Err(first) => {
                    // Models frequently put raw newlines or tabs inside JSON
                    // strings. Repair only those before giving up.
                    let repaired = escape_control_characters(&call.function.arguments);
                    match serde_json::from_str(&repaired) {
                        Ok(arguments) => arguments,
                        Err(_) => {
                            // Keep the call and let the tool loop report the
                            // failure back to the model. Failing the whole run
                            // here discards otherwise usable reasoning and
                            // gives the model no chance to correct itself.
                            argument_error = Some(format!(
                                "arguments are not valid JSON: {first}; raw arguments: {}",
                                truncate_for_error(&call.function.arguments)
                            ));
                            Value::Object(Default::default())
                        }
                    }
                }
            }
        };
        tool_calls.push(LlmToolCall {
            id: call.id,
            name: call.function.name,
            arguments,
            argument_error,
        });
    }
    let mut content = choice.message.content.unwrap_or_default();
    if tool_calls.is_empty()
        && let Some((parsed, remaining)) = parse_dsml_tool_calls(&content)
    {
        tool_calls = parsed;
        content = remaining;
    }
    if content.trim().is_empty() && tool_calls.is_empty() {
        tracing::warn!(
            model = %model,
            finish_reason = ?choice.finish_reason,
            content_chars = content.chars().count(),
            reasoning_chars = choice
                .message
                .reasoning_content
                .as_deref()
                .map(|text| text.chars().count())
                .unwrap_or(0),
            "model returned empty content without tool calls"
        );
    }
    Ok(LlmResponse {
        message: LlmMessage::assistant_with_reasoning(
            content,
            tool_calls,
            choice.message.reasoning_content,
        ),
        usage: response
            .usage
            .map(|usage| Usage {
                input_tokens: usage.prompt_tokens,
                output_tokens: usage.completion_tokens,
            })
            .unwrap_or_default(),
    })
}

/// Some providers return tool calls as DSML markup in the assistant content
/// instead of the structured `tool_calls` field. Parse the markup so the node
/// can execute the calls instead of trying to parse it as JSON.
fn parse_dsml_tool_calls(content: &str) -> Option<(Vec<LlmToolCall>, String)> {
    if !content.contains("DSML") || !content.contains("invoke") {
        return None;
    }
    let marker = content.find("DSML")?;
    let start = content[..marker].rfind('<').unwrap_or(marker);
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    let seed = hasher.finish();
    let mut calls = Vec::new();
    let mut search_from = start;
    while let Some(relative) = content[search_from..].find("invoke") {
        let at = search_from + relative;
        let tag = &content[at..];
        let Some(name) = dsml_tag_attribute(tag, "name") else {
            search_from = at + "invoke".len();
            continue;
        };
        let Some(open_end) = tag.find('>') else {
            break;
        };
        let body_start = at + open_end + 1;
        let body_end = dsml_closing_tag(&content[body_start..], "invoke")
            .map(|offset| body_start + offset)
            .unwrap_or(content.len());
        let arguments = dsml_parameters(&content[body_start..body_end]);
        let index = calls.len();
        calls.push(LlmToolCall {
            id: format!("call_dsml_{seed:x}_{index}"),
            name,
            arguments,
            argument_error: None,
        });
        search_from = body_end.saturating_add(1);
        if calls.len() >= 64 {
            break;
        }
    }
    if calls.is_empty() {
        None
    } else {
        Some((calls, content[..start].trim().to_owned()))
    }
}

fn dsml_tag_attribute(tag: &str, name: &str) -> Option<String> {
    let end = tag.find('>')?;
    let head = &tag[..end];
    let needle = format!("{name}=\"");
    let start = head.find(&needle)? + needle.len();
    let rest = &head[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_owned())
}

fn dsml_closing_tag(input: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(relative) = input[from..].find("</") {
        let at = from + relative;
        let head: String = input[at..].chars().take(80).collect();
        if head.contains(name) {
            return Some(at);
        }
        from = at + 2;
    }
    None
}

fn dsml_parameters(body: &str) -> Value {
    let mut map = serde_json::Map::new();
    let mut from = 0;
    while let Some(relative) = body[from..].find("parameter") {
        let at = from + relative;
        let tag = &body[at..];
        let Some(name) = dsml_tag_attribute(tag, "name") else {
            from = at + "parameter".len();
            continue;
        };
        let Some(open_end) = tag.find('>') else {
            break;
        };
        let value_start = at + open_end + 1;
        let value_end = dsml_closing_tag(&body[value_start..], "parameter")
            .map(|offset| value_start + offset)
            .unwrap_or(body.len());
        let raw = &body[value_start..value_end];
        let is_string = dsml_tag_attribute(tag, "string").as_deref() == Some("true");
        let value = if is_string {
            Value::String(raw.to_owned())
        } else {
            serde_json::from_str(raw.trim()).unwrap_or_else(|_| Value::String(raw.to_owned()))
        };
        map.insert(name, value);
        from = value_end.saturating_add(1).min(body.len());
    }
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LlmParams, ModelProfileRef};
    use axum::Router;
    use axum::extract::Json;
    use axum::http::{StatusCode, header};
    use axum::response::IntoResponse;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};
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

    async fn mock_429_then_success() -> (String, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let state = calls.clone();
        let app = Router::new().route(
            "/chat/completions",
            post(move || {
                let state = state.clone();
                async move {
                    let call = state.fetch_add(1, Ordering::SeqCst);
                    if call < 30 {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [(header::RETRY_AFTER, "0")],
                            "rate limited",
                        )
                            .into_response()
                    } else {
                        Json(json!({
                            "choices": [{
                                "message": { "role": "assistant", "content": "done" }
                            }]
                        }))
                        .into_response()
                    }
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
        (format!("http://{addr}"), calls)
    }

    fn request() -> LlmRequest {
        LlmRequest {
            model: ModelProfileRef::new("test-model"),
            params: LlmParams {
                max_tokens: Some(100),
                temperature: Some(0.2),
                timeout: None,
            },
            messages: vec![LlmMessage::system("system"), LlmMessage::user("hello")],
            tools: vec![ToolDefinition {
                name: "echo".to_string(),
                description: "echo".to_string(),
                input_schema: json!({ "type": "object" }),
            }],
        }
    }

    #[test]
    fn infers_the_context_window_from_the_model() {
        assert_eq!(context_window_for_model("deepseek-flash"), Some(1_048_576));
        assert_eq!(context_window_for_model("gpt-4o"), Some(128_000));
        assert_eq!(context_window_for_model("claude-sonnet"), Some(200_000));
        let config = OpenAiCompatibleConfig::new("http://localhost", "key", "deepseek-flash");
        assert_eq!(config.context_window, Some(1_048_576));
        let overridden = OpenAiCompatibleConfig::new("http://localhost", "key", "unknown")
            .with_context_window(32_000);
        assert_eq!(overridden.context_window, Some(32_000));
    }

    #[test]
    fn serializes_user_images_as_content_parts() {
        let config = OpenAiCompatibleConfig::new("http://localhost", "key", "vision-model");
        let request = LlmRequest {
            model: ModelProfileRef::new("vision-model"),
            params: LlmParams::default(),
            messages: vec![LlmMessage::user_with_images(
                "look at this",
                vec![crate::LlmImage {
                    media_type: "image/png".to_string(),
                    data_base64: "aGVsbG8=".to_string(),
                }],
            )],
            tools: Vec::new(),
        };
        let body = request_body(&config, &request);
        let content = &body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "look at this");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,aGVsbG8="
        );
    }

    #[tokio::test]
    async fn parses_content_and_usage() {
        let (base_url, captured) = mock_server(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "done" }
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 4 }
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "fallback-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(response.message.content, "done");
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 4);

        let body = captured.lock().unwrap().first().cloned().unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["tools"][0]["function"]["name"], "echo");
        assert!(body.get("thinking").is_none());
        assert!(body.get("response_format").is_none());
    }

    #[tokio::test]
    async fn retries_429_until_the_provider_accepts() {
        let (base_url, calls) = mock_429_then_success().await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "fallback-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(response.message.content, "done");
        assert_eq!(calls.load(Ordering::SeqCst), 31);
    }

    #[tokio::test]
    async fn deepseek_requests_enable_thinking() {
        let (base_url, captured) = mock_server(json!({
            "choices": [{ "message": { "role": "assistant", "content": "done" } }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "deepseek-chat",
        ))
        .unwrap();
        let mut request = request();
        request.model = ModelProfileRef::new("deepseek-chat");
        model.complete(request).await.unwrap();

        let body = captured.lock().unwrap().first().cloned().unwrap();
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["response_format"]["type"], "json_object");
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
                }
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
            response.message.reasoning_content.as_deref(),
            Some("I should answer briefly.")
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
                }
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "test-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        assert_eq!(response.message.tool_calls.len(), 1);
        assert_eq!(response.message.tool_calls[0].name, "echo");
        assert_eq!(response.message.tool_calls[0].arguments["text"], "hi");
        assert!(response.message.tool_calls[0].argument_error.is_none());
    }

    #[tokio::test]
    async fn parses_dsml_tool_calls_returned_as_content() {
        let (base_url, _captured) = mock_server(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "<｜｜DSML｜｜ invoke name=\"shell\">\n<｜｜DSML｜｜ parameter name=\"command\" string=\"true\">ls -la</｜｜DSML｜｜ parameter>\n</｜｜DSML｜｜ invoke>\n<｜｜DSML｜｜ calls>\n</｜｜DSML｜｜ calls>"
                }
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
        assert!(response.message.content.is_empty());
        assert_eq!(response.message.tool_calls.len(), 1);
        let call = &response.message.tool_calls[0];
        assert_eq!(call.name, "shell");
        assert_eq!(call.arguments["command"], "ls -la");
        assert!(call.argument_error.is_none());
    }

    #[tokio::test]
    async fn repairs_raw_control_characters_in_tool_arguments() {
        // The provider must escape the newline inside the JSON string; models
        // often emit it raw instead.
        let (base_url, _captured) = mock_server(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "echo",
                            "arguments": "{\"text\":\"line1\nline2\"}"
                        }
                    }]
                }
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "test-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        let call = &response.message.tool_calls[0];
        assert_eq!(call.arguments["text"], "line1\nline2");
        assert!(call.argument_error.is_none());
    }

    #[tokio::test]
    async fn keeps_unparseable_tool_arguments_for_the_model_to_retry() {
        let (base_url, _captured) = mock_server(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "task_spawn_children",
                            "arguments": "{\"tasks\":[{\"title\":\"a\"}"
                        }
                    }]
                }
            }]
        }))
        .await;
        let model = OpenAiCompatibleModel::new(OpenAiCompatibleConfig::new(
            base_url,
            "test-key",
            "test-model",
        ))
        .unwrap();
        let response = model.complete(request()).await.unwrap();
        let call = &response.message.tool_calls[0];
        assert_eq!(call.name, "task_spawn_children");
        assert_eq!(call.arguments, json!({}));
        let error = call.argument_error.as_deref().expect("argument error");
        assert!(error.contains("arguments are not valid JSON"));
    }
}
