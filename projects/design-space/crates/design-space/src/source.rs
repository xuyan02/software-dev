//! Read-only access to the source git repository.
//!
//! `.ds/source` is a bare clone. `.ds/source-head` records the current ref as
//! `branch <name>` or `commit <hex>`. Source commands never checkout a worktree
//! and never execute repository code.

use crate::model::{GitOid, Hash32};
use crate::workspace::ObjectFormat;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
        let output = Command::new("git")
            .args(["clone", "--bare", "--quiet", &self.url, cache_text])
            .output()
            .context("run git clone")?;
        if !output.status.success() {
            bail!(
                "git clone failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let default = self.git_text(["symbolic-ref", "--short", "HEAD"])?;
        self.strip_local_heads()?;
        self.set_current(&format!("branch {}", default.trim()))?;
        Ok(())
    }

    pub fn fetch(&self) -> Result<()> {
        if !self.exists() {
            return self.clone_bare();
        }
        self.git_status([
            "fetch",
            "--prune",
            "--tags",
            "origin",
            "+refs/heads/*:refs/remotes/origin/*",
        ])?;
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
        let commit = self.resolve_source(name)?;
        let branch = name.strip_prefix("origin/").unwrap_or(name);
        let branch_ref = format!("refs/remotes/origin/{branch}");
        let is_branch = self
            .git_status(["show-ref", "--verify", "--quiet", &branch_ref])
            .is_ok();
        if is_branch {
            self.set_current(&format!("branch {branch}"))?;
            Ok(SourceRef {
                label: format!("branch {branch}"),
            })
        } else {
            self.set_current(&format!("commit {commit}"))?;
            Ok(SourceRef {
                label: format!("commit {commit}"),
            })
        }
    }

    pub fn resolve_current(&self) -> Result<String> {
        let current = self.current_ref()?;
        let target = current
            .strip_prefix("branch ")
            .or_else(|| current.strip_prefix("commit "))
            .with_context(|| format!("malformed source-head: {current}"))?;
        self.resolve_source(target)
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
