//! Local process sandbox tool.
//!
//! Phase 2 starts with a deliberately simple local subprocess: a workspace
//! root, a command, arguments, a working directory inside the root, and a hard
//! timeout. It is **not** a security boundary. Production uses a container or
//! VM; the tool contract stays the same.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct LocalSandbox {
    root: PathBuf,
    max_output_bytes: usize,
}

impl LocalSandbox {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ToolError> {
        let root = root.into();
        let root = root
            .canonicalize()
            .map_err(|error| ToolError::Failed(format!("invalid sandbox root: {error}")))?;
        Ok(Self {
            root,
            max_output_bytes: 64 * 1024,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[async_trait]
impl Tool for LocalSandbox {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "sandbox_run_command".to_string(),
            description: "Run a program inside the workspace with a timeout. \
                          This is a local subprocess, not a security boundary."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } },
                    "cwd": { "type": "string" },
                    "timeout_ms": { "type": "integer", "minimum": 1 }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let command = arguments
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing command".to_string()))?;
        let args = arguments
            .get("args")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let cwd = match arguments.get("cwd").and_then(Value::as_str) {
            Some(cwd) => self.resolve_cwd(cwd)?,
            None => self.root.clone(),
        };
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(60_000);

        let mut process = Command::new(command);
        process
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        match tokio::time::timeout(Duration::from_millis(timeout_ms), process.output()).await {
            Err(_) => Ok(ToolOutput::error(format!(
                "command timed out after {timeout_ms}ms"
            ))),
            Ok(Err(error)) => Ok(ToolOutput::error(format!("failed to run command: {error}"))),
            Ok(Ok(output)) => {
                let stdout = truncate(
                    &String::from_utf8_lossy(&output.stdout),
                    self.max_output_bytes,
                );
                let stderr = truncate(
                    &String::from_utf8_lossy(&output.stderr),
                    self.max_output_bytes,
                );
                let content = format!(
                    "exit: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                    output.status.code().unwrap_or(-1)
                );
                Ok(ToolOutput {
                    content,
                    is_error: !output.status.success(),
                })
            }
        }
    }
}

impl LocalSandbox {
    fn resolve_cwd(&self, cwd: &str) -> Result<PathBuf, ToolError> {
        let candidate = self.root.join(cwd);
        let candidate = candidate
            .canonicalize()
            .map_err(|error| ToolError::InvalidArguments(format!("invalid cwd: {error}")))?;
        if !candidate.starts_with(&self.root) {
            return Err(ToolError::InvalidArguments(
                "cwd escapes the sandbox root".to_string(),
            ));
        }
        Ok(candidate)
    }
}

fn truncate(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[truncated]", &value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ToolContext {
        ToolContext {
            agent_id: "a".to_string(),
            task_id: "t".to_string(),
            project_id: None,
        }
    }

    #[tokio::test]
    async fn runs_a_command_in_the_root() {
        let root = std::env::temp_dir().join(format!("foundry-sandbox-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let sandbox = LocalSandbox::new(&root).unwrap();
        let output = sandbox
            .call(json!({ "command": "echo", "args": ["hello"] }), &context())
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn rejects_cwd_outside_the_root() {
        let root = std::env::temp_dir().join(format!("foundry-sandbox-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let sandbox = LocalSandbox::new(&root).unwrap();
        let error = sandbox
            .call(json!({ "command": "echo", "cwd": ".." }), &context())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("escapes"));
        std::fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn times_out() {
        let root = std::env::temp_dir().join(format!("foundry-sandbox-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&root).unwrap();
        let sandbox = LocalSandbox::new(&root).unwrap();
        let output = sandbox
            .call(
                json!({ "command": "sleep", "args": ["2"], "timeout_ms": 100 }),
                &context(),
            )
            .await
            .unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("timed out"));
        std::fs::remove_dir_all(root).ok();
    }
}
