//! Project workspace tools for implementation, review and test execution.
//!
//! The workspace is a local directory per project under the configured base
//! root. It is a development workspace, not a security boundary; production
//! should run it in a container or VM.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

pub struct ProjectWorkspace {
    base: PathBuf,
    max_output_bytes: usize,
}

impl ProjectWorkspace {
    pub fn new(base: impl Into<PathBuf>) -> Result<Self, ToolError> {
        let base = base.into();
        std::fs::create_dir_all(&base)
            .map_err(|error| ToolError::Failed(format!("cannot create workspace root: {error}")))?;
        let base = base
            .canonicalize()
            .map_err(|error| ToolError::Failed(format!("invalid workspace root: {error}")))?;
        Ok(Self {
            base,
            max_output_bytes: 128 * 1024,
        })
    }

    fn root(&self, context: &ToolContext) -> Result<PathBuf, ToolError> {
        let project = context.project_id.as_deref().ok_or_else(|| {
            ToolError::InvalidArguments("workspace tools require a project id".to_string())
        })?;
        let root = self.base.join(project);
        std::fs::create_dir_all(&root)
            .map_err(|error| ToolError::Failed(format!("cannot create workspace: {error}")))?;
        let root = root
            .canonicalize()
            .map_err(|error| ToolError::Failed(format!("invalid workspace: {error}")))?;
        if !root.starts_with(&self.base) {
            return Err(ToolError::Failed(
                "workspace escapes the base root".to_string(),
            ));
        }
        Ok(root)
    }

    fn resolve(&self, root: &Path, path: &str) -> Result<PathBuf, ToolError> {
        let path = Path::new(path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(ToolError::InvalidArguments(
                "path must be relative and stay inside the workspace".to_string(),
            ));
        }
        let candidate = root.join(path);
        let mut parent = candidate.parent().unwrap_or(root);
        while !parent.exists() {
            parent = parent.parent().ok_or_else(|| {
                ToolError::InvalidArguments("path has no existing parent".to_string())
            })?;
        }
        let parent = parent.canonicalize().map_err(|error| {
            ToolError::InvalidArguments(format!("invalid path parent: {error}"))
        })?;
        if !parent.starts_with(root) {
            return Err(ToolError::InvalidArguments(
                "path escapes the workspace".to_string(),
            ));
        }
        if candidate.exists() {
            let canonical = candidate
                .canonicalize()
                .map_err(|error| ToolError::InvalidArguments(format!("invalid path: {error}")))?;
            if !canonical.starts_with(root) {
                return Err(ToolError::InvalidArguments(
                    "path escapes the workspace".to_string(),
                ));
            }
        }
        Ok(candidate)
    }

    fn truncate(&self, value: &str) -> String {
        if value.len() <= self.max_output_bytes {
            return value.to_string();
        }
        let mut end = self.max_output_bytes;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…[truncated]", &value[..end])
    }

    fn truncate_tail(&self, value: &str) -> String {
        if value.len() <= self.max_output_bytes {
            return value.to_string();
        }
        let mut start = value.len() - self.max_output_bytes;
        while !value.is_char_boundary(start) {
            start += 1;
        }
        format!("…[truncated]\n{}", &value[start..])
    }
}

async fn run(
    workspace: &ProjectWorkspace,
    root: &Path,
    command: &str,
    args: &[String],
    cwd: &Path,
    timeout_ms: u64,
) -> Result<ToolOutput, ToolError> {
    let mut process = Command::new(command);
    process
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Workspace commands are project code, not the platform. Do not leak the
    // agent's database, session or model credentials to them.
    for key in [
        "DATABASE_URL",
        "FOUNDRY_URL",
        "FOUNDRY_TOKEN",
        "FOUNDRY_AGENT_ID",
        "FOUNDRY_AGENT_WORKER_ID",
        "OPENAI_API_KEY",
    ] {
        process.env_remove(key);
    }
    match tokio::time::timeout(Duration::from_millis(timeout_ms), process.output()).await {
        Err(_) => Ok(ToolOutput::error(format!(
            "command timed out after {timeout_ms}ms"
        ))),
        Ok(Err(error)) => Ok(ToolOutput::error(format!("failed to run command: {error}"))),
        Ok(Ok(output)) => {
            let stdout = workspace.truncate_tail(&String::from_utf8_lossy(&output.stdout));
            let stderr = workspace.truncate_tail(&String::from_utf8_lossy(&output.stderr));
            let content = format!(
                "cwd: {}\nexit: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                cwd.strip_prefix(root).unwrap_or(cwd).display(),
                output.status.code().unwrap_or(-1)
            );
            Ok(ToolOutput {
                content,
                is_error: !output.status.success(),
            })
        }
    }
}

pub struct WorkspaceListTool {
    workspace: Arc<ProjectWorkspace>,
}

impl WorkspaceListTool {
    pub fn new(workspace: Arc<ProjectWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WorkspaceListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_list".to_string(),
            description: "List files and directories in the project workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Relative path; defaults to ." }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let root = self.workspace.root(context)?;
        let path = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
        let dir = self.workspace.resolve(&root, path)?;
        let mut entries = std::fs::read_dir(&dir)
            .map_err(|error| ToolError::Failed(format!("cannot list {}: {error}", dir.display())))?
            .filter_map(Result::ok)
            .map(|entry| {
                let metadata = entry.metadata().ok();
                let kind = if metadata.as_ref().is_some_and(|m| m.is_dir()) {
                    "d"
                } else {
                    "f"
                };
                let size = metadata.map(|m| m.len()).unwrap_or(0);
                format!("{kind} {size:>8} {}", entry.file_name().to_string_lossy())
            })
            .collect::<Vec<_>>();
        entries.sort();
        Ok(ToolOutput::success(if entries.is_empty() {
            "(empty)".to_string()
        } else {
            entries.join("\n")
        }))
    }
}

pub struct WorkspaceReadTool {
    workspace: Arc<ProjectWorkspace>,
}

impl WorkspaceReadTool {
    pub fn new(workspace: Arc<ProjectWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WorkspaceReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_read".to_string(),
            description: "Read a file from the project workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "start_line": { "type": "integer", "minimum": 1 },
                    "end_line": { "type": "integer", "minimum": 1 }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let root = self.workspace.root(context)?;
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing path".to_string()))?;
        let file = self.workspace.resolve(&root, path)?;
        let content = std::fs::read_to_string(&file).map_err(|error| {
            ToolError::Failed(format!("cannot read {}: {error}", file.display()))
        })?;
        let start = arguments
            .get("start_line")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;
        let end = arguments
            .get("end_line")
            .and_then(Value::as_u64)
            .map(|value| value as usize);
        let lines: Vec<&str> = content.lines().collect();
        let end = end.unwrap_or(lines.len()).min(lines.len());
        let slice = if start <= end {
            lines[start - 1..end].join("\n")
        } else {
            String::new()
        };
        Ok(ToolOutput::success(self.workspace.truncate(&slice)))
    }
}

pub struct WorkspaceWriteTool {
    workspace: Arc<ProjectWorkspace>,
}

impl WorkspaceWriteTool {
    pub fn new(workspace: Arc<ProjectWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WorkspaceWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_write".to_string(),
            description: "Create or replace a file in the project workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "append": { "type": "boolean", "default": false }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let root = self.workspace.root(context)?;
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing path".to_string()))?;
        let content = arguments
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArguments("missing content".to_string()))?;
        let append = arguments
            .get("append")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let file = self.workspace.resolve(&root, path)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ToolError::Failed(format!(
                    "cannot create directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        let result = if append {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)
                .and_then(|mut file| file.write_all(content.as_bytes()))
        } else {
            std::fs::write(&file, content)
        };
        result.map_err(|error| {
            ToolError::Failed(format!("cannot write {}: {error}", file.display()))
        })?;
        Ok(ToolOutput::success(format!(
            "wrote {} bytes to {path}",
            content.len()
        )))
    }
}

pub struct WorkspaceRunTool {
    workspace: Arc<ProjectWorkspace>,
}

impl WorkspaceRunTool {
    pub fn new(workspace: Arc<ProjectWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WorkspaceRunTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_run".to_string(),
            description: "Run a command inside the project workspace with a timeout.".to_string(),
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

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let root = self.workspace.root(context)?;
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
            Some(cwd) => self.workspace.resolve(&root, cwd)?,
            None => root.clone(),
        };
        let timeout_ms = arguments
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(120_000)
            .min(600_000);
        run(&self.workspace, &root, command, &args, &cwd, timeout_ms).await
    }
}

pub struct WorkspaceDiffTool {
    workspace: Arc<ProjectWorkspace>,
}

impl WorkspaceDiffTool {
    pub fn new(workspace: Arc<ProjectWorkspace>) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for WorkspaceDiffTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "workspace_diff".to_string(),
            description: "Show the Git diff in the project workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "base": { "type": "string", "description": "Git revision; defaults to HEAD" },
                    "path": { "type": "string" }
                }
            }),
        }
    }

    async fn call(&self, arguments: Value, context: &ToolContext) -> Result<ToolOutput, ToolError> {
        let root = self.workspace.root(context)?;
        let base = arguments
            .get("base")
            .and_then(Value::as_str)
            .unwrap_or("HEAD");
        let mut args = vec![
            "-C".to_string(),
            root.display().to_string(),
            "diff".to_string(),
            "--no-color".to_string(),
            base.to_string(),
        ];
        if let Some(path) = arguments.get("path").and_then(Value::as_str) {
            args.push("--".to_string());
            args.push(path.to_string());
        }
        run(&self.workspace, &root, "git", &args, &root, 30_000).await
    }
}

/// Register the workspace tool family on a registry.
pub fn register(
    registry: &mut crate::ToolRegistry,
    base: impl Into<PathBuf>,
) -> Result<(), ToolError> {
    let workspace = Arc::new(ProjectWorkspace::new(base)?);
    registry.register(Arc::new(WorkspaceListTool::new(Arc::clone(&workspace))));
    registry.register(Arc::new(WorkspaceReadTool::new(Arc::clone(&workspace))));
    registry.register(Arc::new(WorkspaceWriteTool::new(Arc::clone(&workspace))));
    registry.register(Arc::new(WorkspaceRunTool::new(Arc::clone(&workspace))));
    registry.register(Arc::new(WorkspaceDiffTool::new(workspace)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(project: &str) -> ToolContext {
        ToolContext {
            agent_id: "a".to_string(),
            task_id: "t".to_string(),
            project_id: Some(project.to_string()),
        }
    }

    #[tokio::test]
    async fn writes_reads_and_lists_project_files() {
        let base = std::env::temp_dir().join(format!("foundry-workspace-{}", uuid::Uuid::now_v7()));
        let workspace = Arc::new(ProjectWorkspace::new(&base).unwrap());
        let ctx = context("project");

        WorkspaceWriteTool::new(Arc::clone(&workspace))
            .call(
                json!({ "path": "src/main.rs", "content": "fn main() {}\n" }),
                &ctx,
            )
            .await
            .unwrap();
        let read = WorkspaceReadTool::new(Arc::clone(&workspace))
            .call(json!({ "path": "src/main.rs" }), &ctx)
            .await
            .unwrap();
        assert!(read.content.contains("fn main"));
        let list = WorkspaceListTool::new(workspace)
            .call(json!({ "path": "src" }), &ctx)
            .await
            .unwrap();
        assert!(list.content.contains("main.rs"));

        std::fs::remove_dir_all(base).ok();
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_workspace() {
        let base = std::env::temp_dir().join(format!("foundry-workspace-{}", uuid::Uuid::now_v7()));
        let workspace = Arc::new(ProjectWorkspace::new(&base).unwrap());
        let ctx = context("project");
        let result = WorkspaceReadTool::new(workspace)
            .call(json!({ "path": "../outside" }), &ctx)
            .await;
        assert!(result.is_err());
        std::fs::remove_dir_all(base).ok();
    }
}
