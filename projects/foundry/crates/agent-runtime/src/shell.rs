//! The agent's persistent shell.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::sync::Arc;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    Move { x: i16, y: i16 },
    Click { x: i16, y: i16, button: u8 },
    Scroll { x: i16, y: i16, delta: i32 },
    Key { key: String },
    Text { text: String },
}

/// A shell owned by one agent process.
///
/// The implementation decides how the shell is isolated; agents only see a
/// command runner that keeps its filesystem between calls and tasks.
#[async_trait]
pub trait Shell: Send + Sync {
    async fn exec(&self, command: &str) -> Result<ShellOutput, ToolError>;

    /// Upload one local file into the shell.
    async fn upload(&self, local: &std::path::Path, container_path: &str) -> Result<(), ToolError>;

    /// Download one file from the shell. Implementations that cannot copy
    /// files keep the default error; tools that need images must then fall
    /// back to base64 over stdout.
    async fn download(
        &self,
        _container_path: &str,
        _local: &std::path::Path,
    ) -> Result<(), ToolError> {
        Err(ToolError::Failed(
            "this shell does not support file download".to_string(),
        ))
    }

    /// Capture the desktop capsule screen, when the shell has a desktop.
    async fn screenshot(&self) -> Result<Vec<u8>, ToolError> {
        Err(ToolError::Failed(
            "this shell has no desktop screenshot".to_string(),
        ))
    }

    /// Send one mouse or keyboard event to a desktop capsule.
    async fn input(&self, _input: ShellInput) -> Result<(), ToolError> {
        Err(ToolError::Failed(
            "this shell has no desktop input".to_string(),
        ))
    }
}

pub struct ShellTool {
    shell: Arc<dyn Shell>,
}

impl ShellTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell".to_string(),
            description: "Run one command in the agent's shell. The shell keeps its files \
                          between calls and across tasks."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": { "type": "string" }
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
        let output = self.shell.exec(command).await?;
        let text = format!(
            "exit code: {}\nstdout:\n{}\nstderr:\n{}",
            output
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            output.stdout,
            output.stderr,
        );
        Ok(ToolOutput::success(truncate(text)))
    }
}

fn truncate(text: String) -> String {
    if text.len() <= MAX_OUTPUT_BYTES {
        return text;
    }
    let mut end = MAX_OUTPUT_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = text[..end].to_owned();
    truncated.push_str("\n[output truncated]");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;

    struct FakeShell;

    #[async_trait]
    impl Shell for FakeShell {
        async fn upload(
            &self,
            _local: &std::path::Path,
            _container_path: &str,
        ) -> Result<(), ToolError> {
            Ok(())
        }

        async fn exec(&self, command: &str) -> Result<ShellOutput, ToolError> {
            if command == "fail" {
                return Err(ToolError::Failed("shell failed".to_owned()));
            }
            Ok(ShellOutput {
                exit_code: Some(0),
                stdout: "out".to_owned(),
                stderr: "err".to_owned(),
            })
        }
    }

    fn context() -> ToolContext {
        ToolContext {
            agent_id: "agent".to_owned(),
            task_id: "task".to_owned(),
            project_id: None,
        }
    }

    #[tokio::test]
    async fn runs_a_command_and_returns_exit_code_and_streams() {
        let tool = ShellTool::new(Arc::new(FakeShell));
        let output = tool
            .call(json!({ "command": "echo out" }), &context())
            .await
            .unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("exit code: 0"));
        assert!(output.content.contains("stdout:\nout"));
        assert!(output.content.contains("stderr:\nerr"));
    }

    #[tokio::test]
    async fn reports_shell_failures() {
        let tool = ShellTool::new(Arc::new(FakeShell));
        let error = tool
            .call(json!({ "command": "fail" }), &context())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("shell failed"));
    }

    #[tokio::test]
    async fn requires_a_command() {
        let tool = ShellTool::new(Arc::new(FakeShell));
        let error = tool.call(json!({}), &context()).await.unwrap_err();
        assert!(error.to_string().contains("missing command"));
    }
}
