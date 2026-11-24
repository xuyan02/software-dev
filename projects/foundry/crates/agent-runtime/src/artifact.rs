//! Tools for launching and inspecting the final artifact inside a capsule.
//!
//! Test agents must exercise the real product instead of only running unit
//! tests. These tools start a long-running process, expose its logs, stop it,
//! and drive a headless browser while returning screenshots as images.

use crate::{Shell, Tool, ToolContext, ToolError, ToolImage, ToolOutput, ToolSpec};
use async_trait::async_trait;
use base64::Engine;
use serde_json::{Value, json};
use std::sync::Arc;

const ARTIFACT_DIR: &str = "/tmp/foundry-artifacts";

pub struct ArtifactStartTool {
    shell: Arc<dyn Shell>,
}

impl ArtifactStartTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ArtifactStartTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "artifact_start".to_string(),
            description: "Start the final artifact as a long-running process inside the capsule. Returns a PID and log path; use artifact_status/logs to inspect it."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": { "type": "string", "description": "Shell command that starts the artifact" },
                    "cwd": { "type": "string", "description": "Working directory; defaults to /work/<project>/repo" },
                    "name": { "type": "string", "description": "Artifact handle; defaults to artifact" }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let command = required(&arguments, "command")?;
        let name = artifact_name(&arguments);
        let cwd = arguments
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| default_cwd(context));
        let script = format!(
            "mkdir -p {dir}; cd {cwd}; nohup sh -c {command} > {dir}/{name}.log 2>&1 & pid=$!; echo \"$pid\" > {dir}/{name}.pid; sleep 0.3; if kill -0 \"$pid\" 2>/dev/null; then echo \"started pid=$pid log={dir}/{name}.log\"; else echo \"artifact failed to start\"; tail -n 100 {dir}/{name}.log 2>/dev/null || true; exit 1; fi",
            dir = ARTIFACT_DIR,
            cwd = quote(&cwd),
            command = quote(command),
            name = name,
        );
        let output = self
            .shell
            .exec(&script)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(shell_output(output))
    }
}

pub struct ArtifactStatusTool {
    shell: Arc<dyn Shell>,
}

impl ArtifactStatusTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ArtifactStatusTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "artifact_status".to_string(),
            description:
                "Check whether the artifact process is running and show the tail of its log."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Artifact handle; defaults to artifact" }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let _ = context;
        let name = artifact_name(&arguments);
        let script = format!(
            "if [ -f {dir}/{name}.pid ]; then pid=$(cat {dir}/{name}.pid); if kill -0 \"$pid\" 2>/dev/null; then echo \"running pid=$pid\"; else echo \"stopped pid=$pid\"; fi; else echo \"not started\"; fi; echo '--- log ---'; tail -n 80 {dir}/{name}.log 2>/dev/null || true",
            dir = ARTIFACT_DIR,
            name = name,
        );
        let output = self
            .shell
            .exec(&script)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(shell_output(output))
    }
}

pub struct ArtifactLogsTool {
    shell: Arc<dyn Shell>,
}

impl ArtifactLogsTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ArtifactLogsTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "artifact_logs".to_string(),
            description: "Read the tail of the artifact log.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Artifact handle; defaults to artifact" },
                    "lines": { "type": "integer", "minimum": 1, "maximum": 2000, "default": 200 }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = artifact_name(&arguments);
        let lines = arguments
            .get("lines")
            .and_then(Value::as_u64)
            .unwrap_or(200)
            .clamp(1, 2000);
        let output = self
            .shell
            .exec(&format!(
                "tail -n {lines} {ARTIFACT_DIR}/{name}.log 2>/dev/null || echo 'no log yet'"
            ))
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(shell_output(output))
    }
}

pub struct ArtifactStopTool {
    shell: Arc<dyn Shell>,
}

impl ArtifactStopTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ArtifactStopTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "artifact_stop".to_string(),
            description: "Stop the artifact process started by artifact_start.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Artifact handle; defaults to artifact" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = artifact_name(&arguments);
        let script = format!(
            "if [ -f {dir}/{name}.pid ]; then pid=$(cat {dir}/{name}.pid); kill \"$pid\" 2>/dev/null || true; for _ in 1 2 3 4 5; do kill -0 \"$pid\" 2>/dev/null || break; sleep 0.2; done; kill -9 \"$pid\" 2>/dev/null || true; rm -f {dir}/{name}.pid; echo \"stopped pid=$pid\"; else echo 'artifact was not running'; fi",
            dir = ARTIFACT_DIR,
            name = name,
        );
        let output = self
            .shell
            .exec(&script)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(shell_output(output))
    }
}

/// Drive a headless browser and return screenshots as model-visible images.
///
/// The capsule image must provide `/usr/local/bin/foundry-browser`, a small
/// Playwright harness installed by the shell Containerfile.
pub struct BrowserTool {
    shell: Arc<dyn Shell>,
}

impl BrowserTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "browser".to_string(),
            description: "Operate a headless browser: goto, click, fill, press, wait, read text, and screenshot. Screenshots are returned as images for visual inspection."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["actions"],
                "properties": {
                    "actions": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["type"],
                            "properties": {
                                "type": { "type": "string", "enum": ["goto", "click", "fill", "press", "wait", "text", "screenshot"] },
                                "url": { "type": "string" },
                                "selector": { "type": "string" },
                                "value": { "type": "string" },
                                "key": { "type": "string" },
                                "ms": { "type": "integer", "minimum": 0, "maximum": 60000 },
                                "name": { "type": "string", "description": "Screenshot file name without extension" }
                            }
                        }
                    }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let actions = arguments
            .get("actions")
            .cloned()
            .ok_or_else(|| ToolError::InvalidArguments("missing actions".to_string()))?;
        if !actions.is_array() {
            return Err(ToolError::InvalidArguments(
                "actions must be an array".to_string(),
            ));
        }
        let payload = serde_json::to_string(&actions)
            .map_err(|error| ToolError::InvalidArguments(error.to_string()))?;
        let script = format!(
            "mkdir -p {ARTIFACT_DIR} && foundry-browser {payload}",
            payload = quote(&payload),
        );
        let output = self
            .shell
            .exec(&script)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        if output.exit_code != Some(0) {
            return Ok(ToolOutput::error(format!(
                "browser failed\nexit: {:?}\nstdout:\n{}\nstderr:\n{}",
                output.exit_code, output.stdout, output.stderr
            )));
        }
        let value: Value = serde_json::from_str(output.stdout.trim()).map_err(|error| {
            ToolError::Failed(format!(
                "browser returned invalid JSON: {error}\nstdout:\n{}",
                output.stdout
            ))
        })?;
        let text = value
            .get("output")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        let mut images = Vec::new();
        if let Some(screenshots) = value.get("screenshots").and_then(Value::as_array) {
            for path in screenshots.iter().filter_map(Value::as_str) {
                let local =
                    std::env::temp_dir().join(format!("foundry-shot-{}.png", uuid::Uuid::now_v7()));
                self.shell.download(path, &local).await.map_err(|error| {
                    ToolError::Failed(format!("cannot download screenshot {path}: {error}"))
                })?;
                let bytes = tokio::fs::read(&local)
                    .await
                    .map_err(|error| ToolError::Failed(error.to_string()))?;
                let _ = tokio::fs::remove_file(&local).await;
                images.push(ToolImage {
                    media_type: "image/png".to_string(),
                    data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                });
            }
        }
        let content = if text.is_empty() {
            format!("browser completed; {} screenshot(s)", images.len())
        } else {
            format!("{}\n{} screenshot(s)", text, images.len())
        };
        Ok(ToolOutput::success(content).with_images(images))
    }
}

pub struct DesktopScreenshotTool {
    shell: Arc<dyn Shell>,
}

impl DesktopScreenshotTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for DesktopScreenshotTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "desktop_screenshot".to_string(),
            description: "Capture the desktop capsule screen and return it as an image for visual inspection."
                .to_string(),
            input_schema: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(
        &self,
        _arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let bytes = self
            .shell
            .screenshot()
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        let image = ToolImage {
            media_type: "image/png".to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        };
        Ok(ToolOutput::success("desktop screenshot").with_images(vec![image]))
    }
}

pub struct DesktopInputTool {
    shell: Arc<dyn Shell>,
}

impl DesktopInputTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for DesktopInputTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "desktop_input".to_string(),
            description: "Send one mouse or keyboard event to the desktop capsule.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": { "type": "string", "enum": ["move", "click", "scroll", "key", "text"] },
                    "x": { "type": "integer" },
                    "y": { "type": "integer" },
                    "button": { "type": "integer", "minimum": 1, "maximum": 3 },
                    "delta": { "type": "integer" },
                    "key": { "type": "string" },
                    "text": { "type": "string" }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let kind = required(&arguments, "type")?;
        let input = match kind {
            "move" => crate::ShellInput::Move {
                x: coordinate(&arguments, "x")?,
                y: coordinate(&arguments, "y")?,
            },
            "click" => crate::ShellInput::Click {
                x: coordinate(&arguments, "x")?,
                y: coordinate(&arguments, "y")?,
                button: arguments
                    .get("button")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .clamp(1, 3) as u8,
            },
            "scroll" => crate::ShellInput::Scroll {
                x: coordinate(&arguments, "x")?,
                y: coordinate(&arguments, "y")?,
                delta: arguments.get("delta").and_then(Value::as_i64).unwrap_or(0) as i32,
            },
            "key" => crate::ShellInput::Key {
                key: required(&arguments, "key")?.to_string(),
            },
            "text" => crate::ShellInput::Text {
                text: required(&arguments, "text")?.to_string(),
            },
            other => {
                return Err(ToolError::InvalidArguments(format!(
                    "unknown desktop input {other}"
                )));
            }
        };
        self.shell
            .input(input)
            .await
            .map_err(|error| ToolError::Failed(error.to_string()))?;
        Ok(ToolOutput::success(format!("desktop input: {kind}")))
    }
}

fn coordinate(arguments: &Value, key: &str) -> Result<i16, ToolError> {
    let value = arguments
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))?;
    i16::try_from(value).map_err(|_| ToolError::InvalidArguments(format!("{key} is out of range")))
}

fn required<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn artifact_name(arguments: &Value) -> String {
    arguments
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .unwrap_or("artifact")
        .to_string()
}

fn default_cwd(context: &ToolContext) -> String {
    context
        .project_id
        .as_deref()
        .map(|project| format!("/work/{project}/repo"))
        .unwrap_or_else(|| "/work".to_string())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shell_output(output: crate::ShellOutput) -> ToolOutput {
    let content = format!(
        "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.exit_code,
        output.stdout.trim(),
        output.stderr.trim()
    );
    if output.exit_code == Some(0) {
        ToolOutput::success(content)
    } else {
        ToolOutput::error(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_names_reject_path_characters() {
        assert_eq!(artifact_name(&json!({ "name": "api" })), "api");
        assert_eq!(artifact_name(&json!({ "name": "../api" })), "artifact");
        assert_eq!(artifact_name(&json!({})), "artifact");
    }

    #[test]
    fn default_cwd_uses_the_project_repo() {
        let context = ToolContext {
            agent_id: "agent".to_string(),
            task_id: "task".to_string(),
            project_id: Some("project".to_string()),
        };
        assert_eq!(default_cwd(&context), "/work/project/repo");
        let context = ToolContext {
            project_id: None,
            ..context
        };
        assert_eq!(default_cwd(&context), "/work");
    }
}
