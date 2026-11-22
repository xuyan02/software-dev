//! File tools for the agent's persistent shell.
//!
//! Agents already have a command runner, but reading, writing and editing a
//! file through `shell` requires careful quoting and, for edits, a full
//! read-modify-write cycle. These tools keep the same capsule access with a
//! small, structured interface.

use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::shell::Shell;
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};

const MAX_READ_BYTES: usize = 64 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct ReadFileTool {
    shell: Arc<dyn Shell>,
}

impl ReadFileTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "read_file".to_string(),
            description: "Read a text file from the agent's shell. Use `offset` and `limit` \
                          (1-based line numbers) for large files."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 1 },
                    "limit": { "type": "integer", "minimum": 1 }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = parse_string(&arguments, "path")?;
        let offset = parse_usize(&arguments, "offset")?;
        let limit = parse_usize(&arguments, "limit")?;
        let content = read_shell_file(&self.shell, &path).await?;
        Ok(ToolOutput::success(select_lines(&content, offset, limit)))
    }
}

pub struct WriteFileTool {
    shell: Arc<dyn Shell>,
}

impl WriteFileTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write_file".to_string(),
            description: "Write a text file in the agent's shell, replacing it if it exists. \
                          Parent directories are created by default."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["path", "content"],
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "create_parents": { "type": "boolean", "default": true }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = parse_string(&arguments, "path")?;
        let content = parse_string(&arguments, "content")?;
        let create_parents = arguments
            .get("create_parents")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let bytes = content.len();
        write_shell_file(&self.shell, &path, &content, create_parents).await?;
        Ok(ToolOutput::success(format!(
            "Wrote {bytes} bytes to {path}"
        )))
    }
}

pub struct EditFileTool {
    shell: Arc<dyn Shell>,
}

impl EditFileTool {
    pub fn new(shell: Arc<dyn Shell>) -> Self {
        Self { shell }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "edit_file".to_string(),
            description: "Replace an exact string in a text file in the agent's shell. \
                          `old_string` must occur once unless `replace_all` is true."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "required": ["path", "old_string", "new_string"],
                "properties": {
                    "path": { "type": "string" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "replace_all": { "type": "boolean", "default": false }
                }
            }),
        }
    }

    async fn call(
        &self,
        arguments: Value,
        _context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = parse_string(&arguments, "path")?;
        let old = parse_string(&arguments, "old_string")?;
        let new = parse_string(&arguments, "new_string")?;
        let replace_all = arguments
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if old.is_empty() {
            return Err(ToolError::InvalidArguments(
                "old_string must not be empty".to_string(),
            ));
        }
        let content = read_shell_file(&self.shell, &path).await?;
        let count = content.matches(&old).count();
        if count == 0 {
            return Err(ToolError::Failed(format!("old_string not found in {path}")));
        }
        if count > 1 && !replace_all {
            return Err(ToolError::Failed(format!(
                "old_string occurs {count} times in {path}; pass replace_all=true or include more context"
            )));
        }
        let updated = if replace_all {
            content.replace(&old, &new)
        } else {
            content.replacen(&old, &new, 1)
        };
        write_shell_file(&self.shell, &path, &updated, false).await?;
        Ok(ToolOutput::success(format!(
            "Edited {path}: replaced {count} occurrence(s)"
        )))
    }
}

async fn read_shell_file(shell: &Arc<dyn Shell>, path: &str) -> Result<String, ToolError> {
    let output = shell.exec(&format!("cat -- {}", shell_quote(path))).await?;
    if output.exit_code != Some(0) {
        let message = output.stderr.trim();
        return Err(ToolError::Failed(if message.is_empty() {
            format!("cannot read {path}")
        } else {
            format!("cannot read {path}: {message}")
        }));
    }
    Ok(output.stdout)
}

async fn write_shell_file(
    shell: &Arc<dyn Shell>,
    path: &str,
    content: &str,
    create_parents: bool,
) -> Result<(), ToolError> {
    if create_parents && let Some(parent) = Path::new(path).parent() {
        let parent = parent.to_string_lossy();
        if !parent.is_empty() && parent != "." {
            let output = shell
                .exec(&format!("mkdir -p -- {}", shell_quote(&parent)))
                .await?;
            if output.exit_code != Some(0) {
                return Err(ToolError::Failed(format!(
                    "cannot create parent directory {parent}: {}",
                    output.stderr.trim()
                )));
            }
        }
    }
    let temp = temp_path();
    tokio::fs::write(&temp, content.as_bytes())
        .await
        .map_err(|error| ToolError::Failed(format!("write temporary file: {error}")))?;
    let result = shell.upload(&temp, path).await;
    let _ = tokio::fs::remove_file(&temp).await;
    result
}

fn select_lines(content: &str, offset: Option<usize>, limit: Option<usize>) -> String {
    let start = offset.unwrap_or(1).max(1) - 1;
    let lines = content.split_inclusive('\n').collect::<Vec<_>>();
    if start >= lines.len() {
        return String::new();
    }
    let end = limit
        .map(|limit| start.saturating_add(limit))
        .unwrap_or(usize::MAX)
        .min(lines.len());
    truncate_read(lines[start..end].concat())
}

fn truncate_read(mut text: String) -> String {
    if text.len() <= MAX_READ_BYTES {
        return text;
    }
    let mut end = MAX_READ_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[read truncated at 64 KiB; use offset/limit]");
    text
}

fn parse_string(arguments: &Value, key: &str) -> Result<String, ToolError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ToolError::InvalidArguments(format!("missing {key}")))
}

fn parse_usize(arguments: &Value, key: &str) -> Result<Option<usize>, ToolError> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(|value| Some(value as usize))
            .ok_or_else(|| {
                ToolError::InvalidArguments(format!("{key} must be a positive integer"))
            }),
    }
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn temp_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "foundry-file-{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct MemoryShell {
        files: Mutex<HashMap<String, String>>,
    }

    impl MemoryShell {
        fn new() -> Self {
            Self {
                files: Mutex::new(HashMap::new()),
            }
        }

        fn content(&self, path: &str) -> String {
            self.files
                .lock()
                .expect("lock")
                .get(path)
                .cloned()
                .unwrap_or_default()
        }
    }

    #[async_trait]
    impl Shell for MemoryShell {
        async fn exec(&self, command: &str) -> Result<crate::ShellOutput, ToolError> {
            if let Some(path) = command.strip_prefix("cat -- ") {
                let path = unquote(path);
                return Ok(match self.files.lock().expect("lock").get(&path) {
                    Some(content) => crate::ShellOutput {
                        exit_code: Some(0),
                        stdout: content.clone(),
                        stderr: String::new(),
                    },
                    None => crate::ShellOutput {
                        exit_code: Some(1),
                        stdout: String::new(),
                        stderr: format!("cat: {path}: No such file or directory"),
                    },
                });
            }
            if command.starts_with("mkdir -p -- ") {
                return Ok(crate::ShellOutput {
                    exit_code: Some(0),
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            Err(ToolError::Failed(format!("unexpected command: {command}")))
        }

        async fn upload(&self, local: &Path, container_path: &str) -> Result<(), ToolError> {
            let content = std::fs::read_to_string(local)
                .map_err(|error| ToolError::Failed(error.to_string()))?;
            self.files
                .lock()
                .expect("lock")
                .insert(container_path.to_owned(), content);
            Ok(())
        }
    }

    fn unquote(value: &str) -> String {
        value
            .strip_prefix('\'')
            .and_then(|value| value.strip_suffix('\''))
            .unwrap_or(value)
            .replace("'\\''", "'")
    }

    fn context() -> ToolContext {
        ToolContext {
            agent_id: "agent".to_owned(),
            task_id: "task".to_owned(),
            project_id: None,
        }
    }

    #[tokio::test]
    async fn writes_reads_and_edits_a_file() {
        let shell = Arc::new(MemoryShell::new());
        let write = WriteFileTool::new(shell.clone());
        let read = ReadFileTool::new(shell.clone());
        let edit = EditFileTool::new(shell.clone());

        write
            .call(
                json!({"path": "/work/a.txt", "content": "hello\nworld\n"}),
                &context(),
            )
            .await
            .unwrap();
        let output = read
            .call(json!({"path": "/work/a.txt"}), &context())
            .await
            .unwrap();
        assert_eq!(output.content, "hello\nworld\n");

        edit.call(
            json!({
                "path": "/work/a.txt",
                "old_string": "world",
                "new_string": "rust"
            }),
            &context(),
        )
        .await
        .unwrap();
        assert_eq!(shell.content("/work/a.txt"), "hello\nrust\n");
    }

    #[tokio::test]
    async fn edits_every_match_only_when_asked() {
        let shell = Arc::new(MemoryShell::new());
        let write = WriteFileTool::new(shell.clone());
        let edit = EditFileTool::new(shell.clone());
        write
            .call(
                json!({"path": "/work/a.txt", "content": "x\nx\n"}),
                &context(),
            )
            .await
            .unwrap();

        let error = edit
            .call(
                json!({"path": "/work/a.txt", "old_string": "x", "new_string": "y"}),
                &context(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("occurs 2 times"), "{error}");

        edit.call(
            json!({
                "path": "/work/a.txt",
                "old_string": "x",
                "new_string": "y",
                "replace_all": true
            }),
            &context(),
        )
        .await
        .unwrap();
        assert_eq!(shell.content("/work/a.txt"), "y\ny\n");
    }

    #[tokio::test]
    async fn reads_a_line_range() {
        let shell = Arc::new(MemoryShell::new());
        let write = WriteFileTool::new(shell.clone());
        let read = ReadFileTool::new(shell.clone());
        write
            .call(
                json!({"path": "/work/a.txt", "content": "one\ntwo\nthree\n"}),
                &context(),
            )
            .await
            .unwrap();
        let output = read
            .call(
                json!({"path": "/work/a.txt", "offset": 2, "limit": 1}),
                &context(),
            )
            .await
            .unwrap();
        assert_eq!(output.content, "two\n");
    }

    #[tokio::test]
    async fn edit_reports_a_missing_match() {
        let shell = Arc::new(MemoryShell::new());
        let write = WriteFileTool::new(shell.clone());
        let edit = EditFileTool::new(shell.clone());
        write
            .call(json!({"path": "/work/a.txt", "content": "abc"}), &context())
            .await
            .unwrap();
        let error = edit
            .call(
                json!({"path": "/work/a.txt", "old_string": "missing", "new_string": "x"}),
                &context(),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not found"), "{error}");
    }
}
