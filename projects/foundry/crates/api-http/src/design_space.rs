//! Read-only access to a local Arche design repository for the console.
//!
//! Foundry owns the repository root. Each project is one Arche repository
//! under that root. The console reads committed snapshots, objects, document
//! fields and comments directly from the repository; a bare clone of the
//! project's Git repository supplies source snippets. No source code is
//! executed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use foundry_app::{DesignSpaceGateway, DesignSpaceGatewayError};
use foundry_domain::{MergeRequestDiff, Project};
use foundry_web::{DesignSpaceDetail, DesignSpaceLine, DesignSpaceOverview, DesignSpaceSource};
use sha2::{Digest, Sha256};

mod arche;

const SOURCE_CONTEXT: u32 = 18;
const SOURCE_MAX_LINES: u32 = 600;

/// Repository collection root plus the source-clone cache.
#[derive(Clone)]
pub struct DesignSpaceAccess {
    root: PathBuf,
    cache_root: Option<PathBuf>,
    git_lock: Arc<Mutex<()>>,
}

impl DesignSpaceAccess {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let cache_root = foundry_shared::default_config_path()
            .and_then(|path| path.parent().map(|dir| dir.join("source-cache")));
        Self {
            root: root.into(),
            cache_root,
            git_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn branches(&self, project: &Project) -> Result<Vec<String>, String> {
        let access = self.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || access.branches_blocking(&project))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn overview(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<DesignSpaceOverview, String> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.overview_blocking(&project, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn element(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceDetail, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.element_blocking(&project, &hash, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn section(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
        qname: &str,
    ) -> Result<String, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        let branch = branch.to_owned();
        let qname = qname.to_owned();
        tokio::task::spawn_blocking(move || {
            access.section_blocking(&project, &hash, &branch, &qname)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub async fn source(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceSource, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.source_blocking(&project, &hash, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    fn repo_path(&self, project: &Project) -> Result<PathBuf, String> {
        let name = repo_name(&project.design_space_url)
            .ok_or_else(|| "项目还没有分配设计空间".to_string())?;
        let path = self.root.join(&name);
        if !path.is_dir() {
            return Err(format!("设计空间仓库不存在: {name}"));
        }
        if !path.join(".arche").is_dir() {
            return Err(format!("设计空间仓库不是 Arche 仓库: {name}"));
        }
        Ok(path)
    }

    fn overview_blocking(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<DesignSpaceOverview, String> {
        let path = self.repo_path(project)?;
        arche::overview(&path, branch)
    }

    fn element_blocking(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceDetail, String> {
        let path = self.repo_path(project)?;
        arche::detail(&path, hash, branch)
    }

    fn section_blocking(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
        qname: &str,
    ) -> Result<String, String> {
        let path = self.repo_path(project)?;
        arche::section(&path, hash, branch, qname)
    }

    fn source_blocking(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceSource, String> {
        let path = self.repo_path(project)?;
        arche::source(self, &path, project, hash, branch)
    }

    fn ensure_source(&self, project: &Project, commit: &str) -> Result<PathBuf, String> {
        if project.git_repository.trim().is_empty() {
            return Err("项目没有配置 Git 仓库".to_string());
        }
        let cache_root = self
            .cache_root
            .as_ref()
            .ok_or_else(|| "无法确定源码缓存目录".to_string())?;
        fs::create_dir_all(cache_root).map_err(|error| error.to_string())?;
        let key = format!(
            "{}-{}",
            project.id,
            short_hash(project.git_repository.trim())
        );
        let dir = cache_root.join(key);
        let _guard = self.git_lock.lock().map_err(|error| error.to_string())?;

        if !dir.join("HEAD").is_file() {
            if dir.exists() {
                fs::remove_dir_all(&dir)
                    .map_err(|error| format!("清理不完整的源码缓存 {}: {error}", dir.display()))?;
            }
            let output = git(
                None,
                &[
                    "clone",
                    "--bare",
                    "--quiet",
                    "--",
                    project.git_repository.trim(),
                    &dir.to_string_lossy(),
                ],
            )?;
            if !output.status.success() {
                let _ = fs::remove_dir_all(&dir);
                return Err(git_error("无法克隆源码仓库", &output));
            }
        }

        if has_commit(&dir, commit) {
            return Ok(dir);
        }
        let output = git(Some(&dir), &["fetch", "--quiet", "origin"])?;
        if !output.status.success() {
            return Err(git_error("无法从源码仓库拉取缺失的 commit", &output));
        }
        if !has_commit(&dir, commit) {
            return Err(format!(
                "源码仓库中没有 commit {}；可能还没有推送该提交",
                short(commit)
            ));
        }
        Ok(dir)
    }

    fn branches_blocking(&self, project: &Project) -> Result<Vec<String>, String> {
        arche::branches(&self.repo_path(project)?)
    }

    fn branch_head_blocking(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, String> {
        arche::branch_head(&self.repo_path(project)?, branch)
    }

    fn is_ancestor_blocking(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, String> {
        arche::is_ancestor(&self.repo_path(project)?, ancestor, descendant)
    }

    fn diff_blocking(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<MergeRequestDiff, String> {
        arche::diff(&self.repo_path(project)?, from_commit, to_commit)
    }

    fn comment_count_blocking(&self, project: &Project, commit: &str) -> Result<usize, String> {
        arche::comment_count(&self.repo_path(project)?, commit)
    }

    fn content_digest_blocking(&self, project: &Project, commit: &str) -> Result<String, String> {
        arche::content_digest(&self.repo_path(project)?, commit)
    }

    fn fast_forward_blocking(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), String> {
        arche::fast_forward(
            &self.repo_path(project)?,
            target_branch,
            expected_target,
            source_commit,
        )
    }

    fn delete_branch_blocking(&self, project: &Project, branch: &str) -> Result<(), String> {
        if branch.is_empty()
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(format!("invalid branch: {branch}"));
        }
        arche::delete_branch(&self.repo_path(project)?, branch)
    }
}

#[async_trait::async_trait]
impl DesignSpaceGateway for DesignSpaceAccess {
    async fn branches(&self, project: &Project) -> Result<Vec<String>, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || access.branches_blocking(&project))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn branch_head(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.branch_head_blocking(&project, &branch))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn content_digest(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let commit = commit.to_owned();
        tokio::task::spawn_blocking(move || access.content_digest_blocking(&project, &commit))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn is_ancestor(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let ancestor = ancestor.to_owned();
        let descendant = descendant.to_owned();
        tokio::task::spawn_blocking(move || {
            access.is_ancestor_blocking(&project, &ancestor, &descendant)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
        .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn diff(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<MergeRequestDiff, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let from_commit = from_commit.to_owned();
        let to_commit = to_commit.to_owned();
        tokio::task::spawn_blocking(move || {
            access.diff_blocking(&project, &from_commit, &to_commit)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
        .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn comment_count(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let commit = commit.to_owned();
        tokio::task::spawn_blocking(move || access.comment_count_blocking(&project, &commit))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn fast_forward(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let target_branch = target_branch.to_owned();
        let expected_target = expected_target.to_owned();
        let source_commit = source_commit.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            access.fast_forward_blocking(&project, &target_branch, &expected_target, &source_commit)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        match result {
            Ok(()) => Ok(()),
            Err(message) if message.contains("已经前进") || message.contains("快进") => {
                Err(DesignSpaceGatewayError::Conflict(message))
            }
            Err(message) => Err(DesignSpaceGatewayError::Backend(message)),
        }
    }

    async fn delete_branch(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.delete_branch_blocking(&project, &branch))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }
}

fn language_for_path(path: &str) -> String {
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    match extension {
        "rs" => "rust",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "h" => "c",
        "py" => "python",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" => "javascript",
        "jsx" => "jsx",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "cs" => "csharp",
        "php" => "php",
        "swift" => "swift",
        "scala" => "scala",
        "m" | "mm" => "objc",
        _ => "",
    }
    .to_owned()
}

fn repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let path = if let Some(index) = url.find("://") {
        let rest = &url[index + 3..];
        rest.split_once('/').map(|(_, path)| path)?
    } else {
        url.trim_end_matches('/')
    };
    if path.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let name = percent_decode(last);
    valid_repo_name(&name).then_some(name)
}

fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    to_hex_hash(&hasher.finalize())[..12].to_string()
}

fn to_hex_hash(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn git(cwd: Option<&Path>, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.arg("-C").arg(cwd);
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
    command
        .output()
        .map_err(|error| format!("无法运行 git: {error}"))
}

fn git_error(message: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        message.to_string()
    } else {
        format!("{message}: {detail}")
    }
}

fn has_commit(dir: &Path, commit: &str) -> bool {
    let spec = format!("{commit}^{{commit}}");
    git(Some(dir), &["cat-file", "-e", &spec])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn read_blob(dir: &Path, commit: &str, path: &str) -> Result<String, String> {
    let spec = format!("{commit}:{path}");
    let output = git(Some(dir), &["cat-file", "blob", &spec])?;
    if !output.status.success() {
        return Err(git_error(&format!("无法读取 {} 的源码", path), &output));
    }
    if output.stdout.contains(&0) {
        return Err("该文件是二进制，无法显示源码".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn source_lines(
    text: &str,
    commit: &str,
    path: &str,
    start_line: u32,
    end_line: u32,
) -> Result<DesignSpaceSource, String> {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len() as u32;
    if total == 0 {
        return Err("源码文件为空".to_string());
    }
    let (highlight_start, highlight_end) = if end_line == 0 {
        (0, 0)
    } else {
        (
            start_line.max(1).min(total),
            end_line.max(start_line).min(total),
        )
    };
    let first = if highlight_start == 0 {
        1
    } else {
        highlight_start.saturating_sub(SOURCE_CONTEXT).max(1)
    };
    let mut last = if highlight_end == 0 {
        total
    } else {
        highlight_end.saturating_add(SOURCE_CONTEXT).min(total)
    };
    if last < first {
        last = first;
    }
    if last - first + 1 > SOURCE_MAX_LINES {
        last = first + SOURCE_MAX_LINES - 1;
    }

    let mut out = Vec::with_capacity((last - first + 1) as usize);
    for number in first..=last {
        let text = lines[(number - 1) as usize];
        out.push(DesignSpaceLine {
            number,
            text: text.to_owned(),
            in_element: highlight_start != 0
                && number >= highlight_start
                && number <= highlight_end,
        });
    }
    Ok(DesignSpaceSource {
        commit: commit.to_owned(),
        path: path.to_owned(),
        start_line: highlight_start,
        end_line: highlight_end,
        first_line: first,
        last_line: last,
        total_lines: total,
        lines: out,
    })
}

fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}
