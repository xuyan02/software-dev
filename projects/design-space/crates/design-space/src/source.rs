//! Source git repository access.
//!
//! `.ds/source` is a bare clone. `.ds/source-head` records the current ref as
//! `branch <name>` or `commit <hex>`. A linked worktree at the workspace root
//! exposes the checked-out source files to normal `git` commands.

use crate::model::{GitOid, Hash32};
use crate::workspace::ObjectFormat;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const GIT_NETWORK_ATTEMPTS: usize = 5;
const GIT_NETWORK_BACKOFF: Duration = Duration::from_secs(1);
static WORKTREE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEntry {
    pub path: String,
    pub oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct SourceRepo {
    root: PathBuf,
    url: String,
    format: ObjectFormat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChange {
    pub status: char,
    pub old_path: Option<String>,
    pub path: String,
}

impl SourceRepo {
    pub fn open(root: impl Into<PathBuf>, url: &str, format: ObjectFormat) -> Result<Self> {
        if url.trim().is_empty() {
            bail!("source URL is empty");
        }
        Ok(Self {
            root: root.into(),
            url: url.trim().to_owned(),
            format,
        })
    }

    pub fn exists(&self) -> bool {
        self.cache().join("HEAD").is_file()
    }

    pub fn clone_bare(&self) -> Result<()> {
        if self.exists() {
            return Ok(());
        }
        let cache = self.cache();
        if cache.exists() {
            fs::remove_dir_all(&cache)?;
        }
        if let Some(parent) = cache.parent() {
            fs::create_dir_all(parent)?;
        }
        let cache_text = cache.to_str().context("source cache path is not UTF-8")?;
        let mut command = Command::new("git");
        command.args(["clone", "--bare", "--quiet", &self.url, cache_text]);
        run_network_git(command, "clone", Some(&cache))?;
        let default = self.git_text(["symbolic-ref", "--short", "HEAD"])?;
        self.strip_local_heads()?;
        self.set_current(&format!("branch {}", default.trim()))?;
        Ok(())
    }

    pub fn fetch(&self) -> Result<()> {
        if !self.exists() {
            return self.clone_bare();
        }
        // `repo.toml` is the source of truth. If it changed, follow the new
        // URL instead of continuing to fetch from the old origin.
        let origin = self
            .git_text(["remote", "get-url", "origin"])
            .unwrap_or_default();
        if origin.trim() != self.url {
            self.git_status(["remote", "set-url", "origin", self.url.as_str()])?;
        }
        let mut command = Command::new("git");
        command.arg("-C").arg(self.cache()).args([
            "fetch",
            "--prune",
            "--tags",
            "origin",
            "+refs/heads/*:refs/remotes/origin/*",
        ]);
        run_network_git(command, "fetch", None)?;
        Ok(())
    }

    pub fn current_ref(&self) -> Result<String> {
        let path = self.head_path();
        match fs::read_to_string(&path) {
            Ok(text) => Ok(text.trim().to_owned()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let default = self.git_text(["symbolic-ref", "--short", "HEAD"])?;
                Ok(format!("branch {}", default.trim()))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn checkout(&self, git_ref: &str) -> Result<SourceRef> {
        let name = git_ref.trim();
        if name.is_empty() {
            bail!("empty git ref");
        }
        if self.worktree_exists() && self.worktree_dirty()? {
            bail!("source worktree has uncommitted changes; refusing to switch refs");
        }
        let commit = self.resolve_source(name)?;
        let branch = name.strip_prefix("origin/").unwrap_or(name);
        let branch_ref = format!("refs/remotes/origin/{branch}");
        let is_branch = self
            .git_status(["show-ref", "--verify", "--quiet", &branch_ref])
            .is_ok();
        if is_branch {
            self.set_current(&format!("branch {branch}"))?;
            self.checkout_worktree(&commit)?;
            Ok(SourceRef {
                label: format!("branch {branch}"),
            })
        } else {
            self.set_current(&format!("commit {commit}"))?;
            self.checkout_worktree(&commit)?;
            Ok(SourceRef {
                label: format!("commit {commit}"),
            })
        }
    }

    fn workspace_root(&self) -> PathBuf {
        self.root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    pub fn worktree_path(&self) -> PathBuf {
        self.workspace_root()
    }

    pub fn worktree_exists(&self) -> bool {
        self.workspace_root().join(".git").is_file()
    }

    /// Create the workspace-root linked worktree if it does not exist.
    pub fn ensure_worktree(&self) -> Result<()> {
        if self.worktree_exists() {
            let gitfile = self.workspace_root().join(".git");
            let text = fs::read_to_string(&gitfile)?;
            if !text.contains(".ds/source/worktrees/root") {
                bail!(
                    "workspace root has a different .git pointer; refusing to replace it: {}",
                    gitfile.display()
                );
            }
            self.ensure_exclude()?;
            return Ok(());
        }
        if !self.exists() {
            bail!("source repository is missing; run `ds source fetch`");
        }
        let commit = self.resolve_current()?;
        self.create_worktree(&commit)
    }

    pub fn checkout_worktree(&self, commit: &str) -> Result<()> {
        if !self.worktree_exists() {
            return self.create_worktree(commit);
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(self.workspace_root())
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
                "checkout",
                "--force",
                "--detach",
                commit,
            ])
            .output()
            .context("checkout source worktree")?;
        if !output.status.success() {
            bail!(
                "checkout source worktree failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        self.ensure_exclude()
    }

    fn create_worktree(&self, commit: &str) -> Result<()> {
        let root = self.workspace_root();
        fs::create_dir_all(&root)?;
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            if entry.file_name() == ".ds" {
                continue;
            }
            bail!(
                "workspace root is not empty; cannot create source worktree ({})",
                entry.path().display()
            );
        }
        let parent = root
            .parent()
            .context("workspace root has no parent")?
            .to_path_buf();
        let temp = parent.join(format!(
            ".ds-worktree-{}-{}",
            std::process::id(),
            WORKTREE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if temp.exists() {
            fs::remove_dir_all(&temp)?;
        }
        let output = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "submodule.recurse=false",
            ])
            .arg("--git-dir")
            .arg(self.cache())
            .args(["worktree", "add", "--detach", "--force"])
            .arg(&temp)
            .arg(commit)
            .output()
            .context("create source worktree")?;
        if !output.status.success() {
            let _ = fs::remove_dir_all(&temp);
            bail!(
                "create source worktree failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        if temp.join(".ds").exists() {
            let _ = fs::remove_dir_all(&temp);
            bail!(
                "source repository contains a .ds directory; cannot use the workspace root as its worktree"
            );
        }
        let gitfile = fs::read_to_string(temp.join(".git"))?;
        let admin = gitfile
            .trim()
            .strip_prefix("gitdir: ")
            .map(PathBuf::from)
            .context("source worktree has no gitdir pointer")?;
        for entry in fs::read_dir(&temp)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let target = root.join(entry.file_name());
            if target.exists() {
                let _ = fs::remove_dir_all(&temp);
                bail!(
                    "source file collides with workspace data: {}",
                    target.display()
                );
            }
            fs::rename(entry.path(), target)?;
        }
        let worktrees = self.cache().join("worktrees");
        let final_admin = worktrees.join("root");
        if final_admin.exists() {
            fs::remove_dir_all(&final_admin)?;
        }
        fs::rename(&admin, &final_admin)?;
        fs::write(root.join(".git"), "gitdir: .ds/source/worktrees/root\n")?;
        fs::write(final_admin.join("gitdir"), "../../../.git\n")?;
        let _ = fs::remove_dir_all(&temp);
        self.ensure_exclude()
    }

    fn ensure_exclude(&self) -> Result<()> {
        let info = self.cache().join("info");
        fs::create_dir_all(&info)?;
        let path = info.join("exclude");
        let mut text = fs::read_to_string(&path).unwrap_or_default();
        if !text.lines().any(|line| line.trim() == ".ds/") {
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(".ds/\n");
        }
        fs::write(path, text)?;
        Ok(())
    }

    pub fn resolve_current(&self) -> Result<String> {
        if let Some(head) = self.worktree_head()? {
            return Ok(head);
        }
        self.resolve_head()
    }

    /// Commit selected by `source-head`, independent of the worktree HEAD.
    pub fn resolve_head(&self) -> Result<String> {
        let current = self.current_ref()?;
        let target = current
            .strip_prefix("branch ")
            .or_else(|| current.strip_prefix("commit "))
            .with_context(|| format!("malformed source-head: {current}"))?;
        self.resolve_source(target)
    }

    fn worktree_dirty(&self) -> Result<bool> {
        if !self.worktree_exists() {
            return Ok(false);
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(self.workspace_root())
            .args(["status", "--porcelain"])
            .output()
            .context("read source worktree status")?;
        if !output.status.success() {
            bail!(
                "read source worktree status failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(!output.stdout.is_empty())
    }

    pub fn sync_worktree_to_head(&self) -> Result<()> {
        if !self.worktree_exists() {
            return self.ensure_worktree();
        }
        if self.worktree_dirty()? {
            bail!("source worktree has uncommitted changes; refusing to overwrite");
        }
        let commit = self.resolve_head()?;
        self.checkout_worktree(&commit)
    }

    fn worktree_head(&self) -> Result<Option<String>> {
        if !self.worktree_exists() {
            return Ok(None);
        }
        let output = Command::new("git")
            .arg("-C")
            .arg(self.workspace_root())
            .args(["rev-parse", "HEAD"])
            .output()
            .context("read source worktree HEAD")?;
        if !output.status.success() {
            bail!(
                "read source worktree HEAD failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Some(
            String::from_utf8(output.stdout)
                .context("source worktree HEAD is not UTF-8")?
                .trim()
                .to_owned(),
        ))
    }

    pub fn resolve_source(&self, git_ref: &str) -> Result<String> {
        let git_ref = git_ref.trim();
        if git_ref.is_empty() || git_ref.starts_with('-') || git_ref.contains('\0') {
            bail!("invalid git ref");
        }
        let mut specs = Vec::new();
        if git_ref.starts_with("refs/") {
            specs.push(git_ref.to_owned());
        } else {
            let branch = git_ref.strip_prefix("origin/").unwrap_or(git_ref);
            specs.push(format!("refs/remotes/origin/{branch}"));
            specs.push(format!("refs/heads/{branch}"));
            specs.push(format!("refs/tags/{git_ref}"));
            if looks_like_oid(git_ref) {
                specs.push(git_ref.to_owned());
            }
        }
        for spec in specs {
            let target = format!("{spec}^{{commit}}");
            if let Ok(commit) = self.git_text(["rev-parse", "--verify", "--quiet", &target]) {
                let commit = commit.trim();
                if looks_like_oid(commit) {
                    return Ok(commit.to_owned());
                }
            }
        }
        bail!("cannot resolve git ref: {git_ref}")
    }

    pub fn list_blobs(&self, commit: &str) -> Result<Vec<BlobEntry>> {
        let output = self.git_bytes(["ls-tree", "-r", "-z", commit])?;
        let mut entries = Vec::new();
        for chunk in output.split(|byte| *byte == 0) {
            if chunk.is_empty() {
                continue;
            }
            let Some(tab) = chunk.iter().position(|byte| *byte == b'\t') else {
                bail!("malformed ls-tree entry");
            };
            let meta = std::str::from_utf8(&chunk[..tab]).context("ls-tree header")?;
            let path = String::from_utf8(chunk[tab + 1..].to_vec()).context("non-UTF-8 path")?;
            let mut parts = meta.split(' ');
            let mode = parts.next().unwrap_or_default();
            let kind = parts.next().unwrap_or_default();
            let oid = parts.next().unwrap_or_default();
            if kind != "blob" || mode == "120000" || !looks_like_oid(oid) {
                continue;
            }
            entries.push(BlobEntry {
                path,
                oid: oid.to_owned(),
            });
        }
        Ok(entries)
    }

    pub fn read_path(&self, commit: &str, path: &str) -> Result<Vec<u8>> {
        if path.contains('\0') {
            bail!("source path contains NUL");
        }
        let spec = format!("{commit}:{path}");
        self.git_bytes(["cat-file", "blob", &spec])
    }

    pub fn log(&self, git_ref: &str, limit: usize) -> Result<Vec<String>> {
        let output = self.git_text([
            "log",
            &format!("-n{limit}"),
            "--format=%H%x00%an%x00%at%x00%s",
            git_ref,
        ])?;
        Ok(output
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.cache())
            .args(["merge-base", "--is-ancestor", ancestor, descendant])
            .output()
            .context("run git merge-base")?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => bail!(
                "git merge-base failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        }
    }

    pub fn changes(&self, base: &str, head: &str) -> Result<Vec<SourceChange>> {
        let output = self.git_bytes(["diff", "--name-status", "-M", base, head])?;
        let text = String::from_utf8(output).context("git diff output is not UTF-8")?;
        let mut changes = Vec::new();
        for line in text.lines().filter(|line| !line.is_empty()) {
            let mut fields = line.split('\t');
            let status = fields.next().context("missing diff status")?;
            let status = status.chars().next().context("empty diff status")?;
            match status {
                'R' => {
                    let old = fields.next().context("missing rename source")?;
                    let new = fields.next().context("missing rename target")?;
                    changes.push(SourceChange {
                        status: 'R',
                        old_path: Some(old.to_owned()),
                        path: new.to_owned(),
                    });
                }
                'C' => {
                    let _old = fields.next().context("missing copy source")?;
                    let new = fields.next().context("missing copy target")?;
                    changes.push(SourceChange {
                        status: 'A',
                        old_path: None,
                        path: new.to_owned(),
                    });
                }
                'A' | 'M' | 'D' | 'T' => {
                    let path = fields.next().context("missing changed path")?;
                    changes.push(SourceChange {
                        status,
                        old_path: None,
                        path: path.to_owned(),
                    });
                }
                _ => {}
            }
        }
        Ok(changes)
    }

    pub fn git_oid(&self, commit: &str) -> Result<GitOid> {
        let hex = commit.trim();
        let expected = match self.format {
            ObjectFormat::Sha1 => 40,
            ObjectFormat::Sha256 => 64,
        };
        if hex.len() != expected || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid {} commit: {commit}", self.format.as_str());
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let text = std::str::from_utf8(chunk)?;
            bytes[index] = u8::from_str_radix(text, 16)?;
        }
        Ok(GitOid(bytes))
    }

    pub fn current_git_oid(&self) -> Result<GitOid> {
        let commit = self.resolve_current()?;
        self.git_oid(&commit)
    }

    pub fn oid_hex(&self, oid: GitOid) -> String {
        let length = match self.format {
            ObjectFormat::Sha1 => 20,
            ObjectFormat::Sha256 => 32,
        };
        let mut out = String::with_capacity(length * 2);
        for byte in &oid.0[..length] {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    }

    fn strip_local_heads(&self) -> Result<()> {
        let output = self.git_text(["for-each-ref", "--format=%(refname:short)", "refs/heads/"])?;
        for branch in output.lines().filter(|line| !line.is_empty()) {
            let remote = format!("refs/remotes/origin/{branch}");
            let local = format!("refs/heads/{branch}");
            self.git_status(["update-ref", &remote, &local])?;
            self.git_status(["update-ref", "-d", &local])?;
        }
        Ok(())
    }

    fn set_current(&self, text: &str) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        let temp = self.head_path().with_extension("tmp");
        fs::write(&temp, format!("{text}\n"))?;
        fs::rename(temp, self.head_path())?;
        Ok(())
    }

    fn head_path(&self) -> PathBuf {
        self.root.join("source-head")
    }

    fn cache(&self) -> PathBuf {
        self.root.join("source")
    }

    fn git_bytes<const N: usize>(&self, args: [&str; N]) -> Result<Vec<u8>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.cache())
            .args(args)
            .output()
            .context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(output.stdout)
    }

    fn git_text<const N: usize>(&self, args: [&str; N]) -> Result<String> {
        let bytes = self.git_bytes(args)?;
        String::from_utf8(bytes).context("git output is not UTF-8")
    }

    fn git_status<const N: usize>(&self, args: [&str; N]) -> Result<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(self.cache())
            .args(args)
            .output()
            .context("run git")?;
        if !output.status.success() {
            bail!(
                "git failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }
}

fn looks_like_oid(text: &str) -> bool {
    (text.len() == 40 || text.len() == 64) && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Run a network git command, retrying transient TLS/connection failures.
///
/// `cleanup` is removed before a retry so a partial clone does not make the
/// next attempt fail with "destination path already exists".
fn run_network_git(mut command: Command, action: &str, cleanup: Option<&Path>) -> Result<Output> {
    let mut delay = GIT_NETWORK_BACKOFF;
    for attempt in 1..=GIT_NETWORK_ATTEMPTS {
        let output = command.output().context("run git")?;
        if output.status.success() {
            return Ok(output);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if attempt == GIT_NETWORK_ATTEMPTS || !is_transient_git_error(&stderr) {
            bail!("git {action} failed: {}", stderr.trim());
        }
        eprintln!(
            "warning: git {action} failed (attempt {attempt}/{GIT_NETWORK_ATTEMPTS}): {}",
            stderr.trim()
        );
        if let Some(path) = cleanup {
            let _ = fs::remove_dir_all(path);
        }
        std::thread::sleep(delay);
        delay = delay.saturating_mul(2);
    }
    unreachable!("git retry loop always returns")
}

fn is_transient_git_error(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    [
        "gnutls recv error",
        "tls connection was non-properly terminated",
        "gnutls_handshake",
        "ssl_error_syscall",
        "connection reset",
        "connection timed out",
        "connection refused",
        "could not resolve host",
        "temporary failure in name resolution",
        "the remote end hung up unexpectedly",
        "early eof",
        "rpc failed",
        "http/2",
    ]
    .iter()
    .any(|pattern| stderr.contains(pattern))
}

/// Compute a GitOid from a raw SHA-1/SHA-256 hash value and object format.
pub fn git_oid_from_hash(hash: Hash32, format: ObjectFormat) -> GitOid {
    match format {
        ObjectFormat::Sha1 => {
            let mut bytes = [0u8; 32];
            bytes[..20].copy_from_slice(&hash.0[..20]);
            GitOid(bytes)
        }
        ObjectFormat::Sha256 => GitOid(hash.0),
    }
}

/// Resolve a source path and commit against a bare clone when available.
pub fn read_source_bytes(
    root: &Path,
    url: &str,
    format: ObjectFormat,
    commit: Option<&str>,
    path: &str,
) -> Result<Vec<u8>> {
    let repo = SourceRepo::open(root, url, format)?;
    let commit = match commit {
        Some(commit) => commit.to_owned(),
        None => repo.resolve_current()?,
    };
    repo.read_path(&commit, path)
}

#[cfg(test)]
mod tests {
    use super::is_transient_git_error;

    #[test]
    fn recognizes_transient_git_network_errors() {
        assert!(is_transient_git_error(
            "fatal: unable to access 'https://github.com/x/y.git/': GnuTLS recv error (-110): The TLS connection was non-properly terminated."
        ));
        assert!(is_transient_git_error(
            "fatal: the remote end hung up unexpectedly"
        ));
        assert!(is_transient_git_error("error: RPC failed; curl 56"));
    }

    #[test]
    fn does_not_retry_permanent_git_errors() {
        assert!(!is_transient_git_error(
            "fatal: Authentication failed for 'https://github.com/x/y.git/'"
        ));
        assert!(!is_transient_git_error("fatal: repository 'x/y' not found"));
        assert!(!is_transient_git_error("fatal: not a git repository"));
    }
}
