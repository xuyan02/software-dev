//! A small MCP (Model Context Protocol) client.
//!
//! Phase 1 implements the JSON-RPC subset needed for tools: initialize,
//! tools/list and tools/call, over a pluggable transport. `StdioTransport`
//! spawns a subprocess; tests use an in-memory transport.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("transport error: {0}")]
    Transport(String),
    #[error("protocol error: {0}")]
    Protocol(String),
    #[error("rpc error {code}: {message}")]
    Rpc { code: i64, message: String },
}

#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn send(&self, message: Value) -> Result<(), McpError>;
    async fn receive(&self) -> Result<Option<Value>, McpError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpToolSpec {
    fn from_json(value: &Value) -> Result<Self, McpError> {
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::Protocol("tool has no name".to_string()))?
            .to_string();
        Ok(Self {
            name,
            description: value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input_schema: value
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object" })),
        })
    }
}

pub struct McpClient {
    transport: Arc<dyn McpTransport>,
    next_id: AtomicU64,
    protocol_version: String,
}

impl McpClient {
    pub fn new(transport: Arc<dyn McpTransport>) -> Self {
        Self {
            transport,
            next_id: AtomicU64::new(1),
            protocol_version: "2025-06-18".to_string(),
        }
    }

    pub async fn initialize(&self) -> Result<Value, McpError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": self.protocol_version,
                    "capabilities": {},
                    "clientInfo": {
                        "name": "foundry-agent-runtime",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }),
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolSpec>, McpError> {
        let result = self.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| McpError::Protocol("tools/list result has no tools".to_string()))?;
        tools.iter().map(McpToolSpec::from_json).collect()
    }

    pub async fn tools(self: &Arc<Self>) -> Result<Vec<McpTool>, McpError> {
        let specs = self.list_tools().await?;
        Ok(specs
            .into_iter()
            .map(|spec| McpTool::new(self, spec))
            .collect())
    }

    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolOutput, McpError> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let is_error = result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut parts = Vec::new();
        if let Some(content) = result.get("content").and_then(Value::as_array) {
            for block in content {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    parts.push(text.to_string());
                }
            }
        }
        if parts.is_empty()
            && let Some(structured) = result.get("structuredContent")
        {
            parts.push(
                serde_json::to_string_pretty(structured)
                    .map_err(|error| McpError::Protocol(error.to_string()))?,
            );
        }
        Ok(ToolOutput {
            content: parts.join("\n"),
            is_error,
        })
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.transport
            .send(json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }))
            .await?;
        loop {
            let Some(message) = self.transport.receive().await? else {
                return Err(McpError::Transport(
                    "transport closed before response".to_string(),
                ));
            };
            let Some(response_id) = message.get("id").and_then(Value::as_u64) else {
                // Notification or server request; Phase 1 ignores them.
                continue;
            };
            if response_id != id {
                continue;
            }
            if let Some(error) = message.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("rpc error")
                    .to_string();
                return Err(McpError::Rpc { code, message });
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        self.transport
            .send(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await
    }
}

pub struct McpTool {
    client: Arc<McpClient>,
    spec: McpToolSpec,
}

impl McpTool {
    pub fn new(client: &Arc<McpClient>, spec: McpToolSpec) -> Self {
        Self {
            client: Arc::clone(client),
            spec,
        }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.spec.name.clone(),
            description: self.spec.description.clone(),
            input_schema: self.spec.input_schema.clone(),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        self.client
            .call_tool(&self.spec.name, arguments)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))
    }
}

pub struct StdioTransport {
    child: Mutex<Option<Child>>,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
}

impl StdioTransport {
    pub async fn spawn(
        command: impl AsRef<str>,
        args: &[String],
        env: &[(String, String)],
    ) -> Result<Self, McpError> {
        let mut child = Command::new(command.as_ref());
        child
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        for (key, value) in env {
            child.env(key, value);
        }
        let mut spawned = child
            .spawn()
            .map_err(|error| McpError::Transport(error.to_string()))?;
        let stdin = spawned
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("child has no stdin".to_string()))?;
        let stdout = spawned
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("child has no stdout".to_string()))?;
        Ok(Self {
            child: Mutex::new(Some(spawned)),
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
        })
    }

    pub async fn close(&self) -> Result<(), McpError> {
        if let Some(mut child) = self.child.lock().await.take() {
            child
                .kill()
                .await
                .map_err(|error| McpError::Transport(error.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send(&self, message: Value) -> Result<(), McpError> {
        let mut line = serde_json::to_string(&message)
            .map_err(|error| McpError::Transport(error.to_string()))?;
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| McpError::Transport(error.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|error| McpError::Transport(error.to_string()))
    }

    async fn receive(&self) -> Result<Option<Value>, McpError> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .await
            .map_err(|error| McpError::Transport(error.to_string()))?;
        if read == 0 {
            return Ok(None);
        }
        serde_json::from_str(&line)
            .map(Some)
            .map_err(|error| McpError::Protocol(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;

    struct MockTransport {
        incoming: StdMutex<VecDeque<Value>>,
        sent: StdMutex<Vec<Value>>,
    }

    impl MockTransport {
        fn new(incoming: Vec<Value>) -> Self {
            Self {
                incoming: StdMutex::new(incoming.into()),
                sent: StdMutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        async fn send(&self, message: Value) -> Result<(), McpError> {
            self.sent.lock().expect("lock poisoned").push(message);
            Ok(())
        }

        async fn receive(&self) -> Result<Option<Value>, McpError> {
            Ok(self.incoming.lock().expect("lock poisoned").pop_front())
        }
    }

    #[tokio::test]
    async fn initializes_lists_and_calls_tools() {
        let transport = Arc::new(MockTransport::new(vec![
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mock", "version": "1" }
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "result": {
                    "tools": [{
                        "name": "echo",
                        "description": "Echo",
                        "inputSchema": { "type": "object" }
                    }]
                }
            }),
            json!({
                "jsonrpc": "2.0",
                "id": 3,
                "result": {
                    "content": [{ "type": "text", "text": "ok" }],
                    "isError": false
                }
            }),
        ]));
        let client = Arc::new(McpClient::new(transport.clone()));
        client.initialize().await.unwrap();
        let tools = client.tools().await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].spec().name, "echo");
        let output = client
            .call_tool("echo", json!({ "text": "hi" }))
            .await
            .unwrap();
        assert_eq!(output.content, "ok");
        assert!(!output.is_error);

        let sent = transport.sent.lock().unwrap();
        assert_eq!(sent[0]["method"], "initialize");
        assert_eq!(sent[1]["method"], "notifications/initialized");
        assert_eq!(sent[2]["method"], "tools/list");
        assert_eq!(sent[3]["method"], "tools/call");
        assert_eq!(sent[3]["params"]["name"], "echo");
    }

    #[tokio::test]
    async fn surfaces_rpc_errors() {
        let transport = Arc::new(MockTransport::new(vec![json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32601, "message": "not found" }
        })]));
        let client = McpClient::new(transport);
        let error = client.initialize().await.unwrap_err();
        assert!(error.to_string().contains("not found"));
    }
}
