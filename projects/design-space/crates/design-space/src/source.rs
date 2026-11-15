//! Source repository access.
//!
//! The source URL is fixed in `.design/config`. `init` clones one bare
//! repository into `.design/source`; later fetches are explicit
//! (`ds source fetch`), never implicit in `create`.
//!
//! The tool tracks its own current source ref in `.design/source-head`
//! (`branch <name>` or `commit <sha>`), so switching branches or pinning a
//! commit never depends on a work tree. It reads blobs through `git cat-file`
//! and never checks out files, so filters and smudge rules cannot change the
//! bytes the design space describes.
//!
//! This local implementation runs `git` directly. Running it inside an
//! isolated capsule is a later step; it never executes repository code.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

pub struct SourceRepo {
    url: String,
    root: PathBuf,
    cache: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEntry {
    pub path: String,
    pub oid: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchEntry {
    pub name: String,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitEntry {
    pub commit: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
}

/// What the source is currently pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Branch(String),
    Commit(String),
}

impl SourceRef {
    pub fn label(&self) -> String {
        match self {
            Self::Branch(name) => format!("branch:{name}"),
            Self::Commit(commit) => format!("commit:{commit}"),
        }
    }
}

impl SourceRepo {
    pub fn open(root: &Path, url: &str) -> Result<Self> {
        if url.trim().is_empty() {
            return Err(Error::Config("source URL is empty".into()));
        }
        Ok(Self {
            url: url.trim().to_owned(),
            root: root.to_path_buf(),
            // One design space has exactly one immutable source, so a fixed
            // path is enough; the URL itself lives in `config`.
            cache: root.join("source"),
        })
    }

    pub fn exists(&self) -> bool {
        self.cache.join("HEAD").is_file()
    }

    /// Clone the source if it is missing. `init` calls this.
    pub fn clone_bare(&self) -> Result<()> {
        if self.exists() {
            return Ok(());
        }
        if self.cache.exists() {
            // A previous init failed halfway through the clone.
            fs::remove_dir_all(&self.cache)?;
        }
        if let Some(parent) = self.cache.parent() {
            fs::create_dir_all(parent)?;
        }
        let cache = self
            .cache
            .to_str()
            .ok_or_else(|| Error::Config("source cache path is not UTF-8".into()))?;
        run_git(&["clone", "--bare", "--quiet", self.url.as_str(), cache])?;
        // The source has no local branches: turn the clone's refs/heads into
        // refs/remotes/origin and delete the local ones.
        self.strip_local_heads(true)?;
        Ok(())
    }

    /// Clone if missing, otherwise fetch. Kept for library users; the CLI
    /// fetches only through `ds source fetch`.
    pub fn ensure(&self) -> Result<()> {
        if self.exists() {
            self.fetch()
        } else {
            self.clone_bare()
        }
    }

    /// Fetch remote branches and tags. Local branches are never created.
    pub fn fetch(&self) -> Result<()> {
        self.git(&[
            "fetch",
            "--prune",
            "--tags",
            "origin",
            "+refs/heads/*:refs/remotes/origin/*",
        ])?;
        self.strip_local_heads(false)?;
        Ok(())
    }

    /// Name of the source's default branch, taken from the bare clone's HEAD.
    pub fn default_branch(&self) -> Result<String> {
        let output = self.git_output(&["symbolic-ref", "--short", "-q", "HEAD"])?;
        if !output.status.success() {
            return Err(Error::Config(
                "source HEAD is detached; cannot determine the default branch".into(),
            ));
        }
        let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if name.is_empty() {
            return Err(Error::Config("source has no default branch".into()));
        }
        Ok(name)
    }

    /// Current source branch or commit, as recorded in `.design/source-head`.
    pub fn current(&self) -> Result<SourceRef> {
        if let Ok(text) = fs::read_to_string(self.head_path()) {
            let text = text.trim();
            if let Some(name) = text.strip_prefix("branch ")
                && !name.is_empty()
            {
                return Ok(SourceRef::Branch(name.to_owned()));
            }
            if let Some(commit) = text.strip_prefix("commit ")
                && !commit.is_empty()
            {
                return Ok(SourceRef::Commit(commit.to_owned()));
            }
            return Err(Error::Config(format!(
                "malformed .design/source-head: {text}"
            )));
        }
        self.derive_current()
    }

    pub fn set_current(&self, target: &SourceRef) -> Result<()> {
        let text = match target {
            SourceRef::Branch(name) => format!("branch {name}\n"),
            SourceRef::Commit(commit) => format!("commit {commit}\n"),
        };
        fs::create_dir_all(&self.root)?;
        let temp = self.head_path().with_extension("tmp");
        fs::write(&temp, text)?;
        fs::rename(temp, self.head_path())?;
        Ok(())
    }

    /// Switch the source to a remote branch, or pin it to a tag/commit.
    ///
    /// A branch stays symbolic (`branch <name>`); anything else is resolved to
    /// a commit and stored detached.
    pub fn checkout(&self, git_ref: &str) -> Result<SourceRef> {
        let name = git_ref.trim();
        if name.is_empty() {
            return Err(Error::Config("empty git ref".into()));
        }
        if let Some(branch) = remote_branch_name(name)
            && self.remote_branch_exists(&branch)?
        {
            let target = SourceRef::Branch(branch);
            self.set_current(&target)?;
            return Ok(target);
        }
        let commit = self.resolve_source(name)?;
        let target = SourceRef::Commit(commit);
        self.set_current(&target)?;
        Ok(target)
    }

    pub fn resolve_current(&self) -> Result<String> {
        match self.current()? {
            SourceRef::Branch(name) => self.resolve_source(&name),
            SourceRef::Commit(commit) => self.resolve_source(&commit),
        }
    }

    /// Resolve a remote branch, tag or commit. Local branches do not exist.
    pub fn resolve_source(&self, git_ref: &str) -> Result<String> {
        let git_ref = git_ref.trim();
        if git_ref.is_empty() {
            return Err(Error::Config("empty git ref".into()));
        }
        if git_ref.starts_with('-') || git_ref.contains('\0') {
            return Err(Error::Config(format!("invalid git ref: {git_ref}")));
        }

        let mut specs = Vec::new();
        if git_ref.starts_with("refs/remotes/origin/") || git_ref.starts_with("refs/tags/") {
            specs.push(git_ref.to_owned());
        } else if !git_ref.starts_with("refs/") {
            if let Some(rest) = git_ref.strip_prefix("origin/") {
                specs.push(format!("refs/remotes/origin/{rest}"));
            }
            specs.push(format!("refs/remotes/origin/{git_ref}"));
            specs.push(format!("refs/tags/{git_ref}"));
            if looks_like_sha(git_ref) {
                specs.push(git_ref.to_owned());
            }
        }

        for spec in specs {
            let arg = format!("{spec}^{{commit}}");
            let output = self.git_output(&["rev-parse", "--verify", "--end-of-options", &arg])?;
            if output.status.success() {
                let commit = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if is_oid(&commit) {
                    return Ok(commit);
                }
            }
        }
        Err(Error::Config(format!(
            "{git_ref} is not an origin branch, tag or commit; run `ds source fetch` first"
        )))
    }

    /// Resolve any ref the local git understands, including remote branches.
    pub fn resolve(&self, git_ref: &str) -> Result<String> {
        let git_ref = git_ref.trim();
        if git_ref.is_empty() {
            return Err(Error::Config("empty git ref".into()));
        }
        if git_ref.starts_with('-') {
            return Err(Error::Config(format!("invalid git ref: {git_ref}")));
        }
        let spec = format!("{git_ref}^{{commit}}");
        let output = self.git(&["rev-parse", "--verify", "--end-of-options", &spec])?;
        let commit = String::from_utf8(output)
            .map_err(|error| Error::Config(format!("commit is not UTF-8: {error}")))?
            .trim()
            .to_owned();
        if !is_oid(&commit) {
            return Err(Error::Config(format!("not a commit SHA: {commit}")));
        }
        Ok(commit)
    }

    /// Remote branches known to the local clone, as `origin/<name>`.
    pub fn branches(&self) -> Result<Vec<BranchEntry>> {
        let output = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)",
            "refs/remotes/origin",
        ])?;
        let mut entries = parse_ref_entries(&output)?;
        entries.retain(|entry| entry.name != "origin/HEAD");
        Ok(entries)
    }

    /// Recent commits, newest first.
    pub fn log(&self, git_ref: &str, limit: usize) -> Result<Vec<CommitEntry>> {
        let limit = limit.max(1);
        let count = format!("-n{limit}");
        let output = self.git(&[
            "log",
            &count,
            "--format=%H%x00%an%x00%at%x00%s",
            git_ref,
            "--",
        ])?;
        let mut entries = Vec::new();
        for line in output.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&[u8]> = line.split(|byte| *byte == 0).collect();
            if fields.len() != 4 {
                return Err(Error::Config("malformed git log entry".into()));
            }
            let text = |bytes: &[u8]| {
                String::from_utf8(bytes.to_vec())
                    .map_err(|error| Error::Config(format!("git log field: {error}")))
            };
            let commit = text(fields[0])?;
            let author = text(fields[1])?;
            let timestamp = text(fields[2])?
                .parse()
                .map_err(|error| Error::Config(format!("git log timestamp: {error}")))?;
            entries.push(CommitEntry {
                commit,
                author,
                timestamp,
                subject: text(fields[3])?,
            });
        }
        Ok(entries)
    }

    /// List regular blobs in a commit. Symlinks and submodules are skipped.
    pub fn list_blobs(&self, commit: &str) -> Result<Vec<BlobEntry>> {
        let output = self.git(&["ls-tree", "-r", "-z", commit])?;
        let mut entries = Vec::new();
        for chunk in output.split(|byte| *byte == 0) {
            if chunk.is_empty() {
                continue;
            }
            let Some(tab) = chunk.iter().position(|byte| *byte == b'\t') else {
                return Err(Error::Config("malformed ls-tree entry".into()));
            };
            let meta = std::str::from_utf8(&chunk[..tab])
                .map_err(|error| Error::Config(format!("ls-tree header: {error}")))?;
            let path = String::from_utf8(chunk[tab + 1..].to_vec())
                .map_err(|error| Error::Config(format!("non-UTF-8 path: {error}")))?;

            let mut parts = meta.split(' ');
            let mode = parts.next().unwrap_or_default();
            let kind = parts.next().unwrap_or_default();
            let oid = parts.next().unwrap_or_default();
            if kind != "blob" || mode == "120000" || !is_oid(oid) {
                continue;
            }
            entries.push(BlobEntry {
                path,
                oid: oid.to_owned(),
            });
        }
        Ok(entries)
    }

    pub fn blob_size(&self, oid: &str) -> Result<u64> {
        let output = self.git(&["cat-file", "-s", oid])?;
        let text = String::from_utf8(output)
            .map_err(|error| Error::Config(format!("blob size is not UTF-8: {error}")))?;
        text.trim()
            .parse()
            .map_err(|error| Error::Config(format!("blob size: {error}")))
    }

    pub fn read_blob(&self, oid: &str) -> Result<Vec<u8>> {
        if !is_oid(oid) {
            return Err(Error::Config(format!("invalid blob oid: {oid}")));
        }
        self.git(&["cat-file", "blob", oid])
    }

    /// Read one file's bytes at a commit without a work tree.
    pub fn read_path(&self, commit: &str, path: &str) -> Result<Vec<u8>> {
        if !is_oid(commit) {
            return Err(Error::Config(format!("invalid commit oid: {commit}")));
        }
        if path.contains('\0') {
            return Err(Error::Config("path contains NUL".into()));
        }
        let spec = format!("{commit}:{path}");
        self.git(&["cat-file", "blob", &spec])
    }

    /// True when `ancestor` is reachable from `descendant`.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool> {
        if !is_oid(ancestor) {
            return Err(Error::Config(format!("invalid commit oid: {ancestor}")));
        }
        if !is_oid(descendant) {
            return Err(Error::Config(format!("invalid commit oid: {descendant}")));
        }
        let output = self.git_output(&["merge-base", "--is-ancestor", ancestor, descendant])?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(Error::Config(format!(
                "git merge-base: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))),
        }
    }

    fn head_path(&self) -> PathBuf {
        self.root.join("source-head")
    }

    fn derive_current(&self) -> Result<SourceRef> {
        let output = self.git_output(&["symbolic-ref", "--short", "-q", "HEAD"])?;
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !name.is_empty() {
                return Ok(SourceRef::Branch(name));
            }
        }
        Ok(SourceRef::Commit(self.resolve("HEAD")?))
    }

    fn remote_branch_exists(&self, name: &str) -> Result<bool> {
        if name.is_empty() || name.starts_with('-') || name.contains('\0') {
            return Ok(false);
        }
        let spec = format!("refs/remotes/origin/{name}^{{commit}}");
        let output = self.git_output(&["rev-parse", "--verify", "--quiet", &spec])?;
        Ok(output.status.success())
    }

    /// The source never keeps local branches. After a clone, optionally copy
    /// `refs/heads/*` to `refs/remotes/origin/*` first, then delete them.
    fn strip_local_heads(&self, seed_remote: bool) -> Result<()> {
        let output = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)%00%(objectname)",
            "refs/heads",
        ])?;
        let entries = parse_ref_entries(&output)?;
        if seed_remote {
            for entry in &entries {
                let reference = format!("refs/remotes/origin/{}", entry.name);
                self.git(&["update-ref", &reference, &entry.commit])?;
            }
        }
        for entry in &entries {
            let reference = format!("refs/heads/{}", entry.name);
            self.git(&["update-ref", "-d", &reference])?;
        }
        Ok(())
    }

    fn git(&self, args: &[&str]) -> Result<Vec<u8>> {
        let output = self.git_output(args)?;
        if !output.status.success() {
            return Err(Error::Config(format!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(output.stdout)
    }

    fn git_output(&self, args: &[&str]) -> Result<std::process::Output> {
        let cache = self
            .cache
            .to_str()
            .ok_or_else(|| Error::Config("source cache path is not UTF-8".into()))?;
        let mut full = vec!["-C", cache];
        full.extend_from_slice(args);
        Command::new("git")
            .args(&full)
            .output()
            .map_err(|error| Error::Config(format!("failed to run git: {error}")))
    }
}

fn run_git(args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(args)
        .output()
        .map_err(|error| Error::Config(format!("failed to run git: {error}")))?;
    if !output.status.success() {
        return Err(Error::Config(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

fn is_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn looks_like_sha(value: &str) -> bool {
    (4..=64).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn parse_ref_entries(output: &[u8]) -> Result<Vec<BranchEntry>> {
    let mut entries = Vec::new();
    for line in output.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Some(separator) = line.iter().position(|byte| *byte == 0) else {
            return Err(Error::Config("malformed for-each-ref entry".into()));
        };
        let name = String::from_utf8(line[..separator].to_vec())
            .map_err(|error| Error::Config(format!("branch name: {error}")))?;
        let commit = String::from_utf8(line[separator + 1..].to_vec())
            .map_err(|error| Error::Config(format!("branch commit: {error}")))?;
        entries.push(BranchEntry { name, commit });
    }
    Ok(entries)
}

/// `origin/main` / `refs/remotes/origin/main` / `main` mean the same branch.
fn remote_branch_name(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix("refs/remotes/origin/") {
        Some(rest.to_owned())
    } else if let Some(rest) = name.strip_prefix("origin/") {
        Some(rest.to_owned())
    } else if name.starts_with("refs/") || name.starts_with('-') || name.contains('\0') {
        None
    } else {
        Some(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    }

    fn commit_all(dir: &Path, message: &str) {
        git(dir, &["add", "."]);
        git(
            dir,
            &[
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                message,
            ],
        );
    }

    fn init_repo(dir: &Path) {
        fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        fs::write(dir.join("a.py"), "class Foo:\n    pass\n").unwrap();
        fs::write(dir.join("README.md"), "hello\n").unwrap();
        commit_all(dir, "init");
    }

    #[test]
    fn reads_blobs_from_a_local_repository() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        init_repo(&repo_dir);

        let source =
            SourceRepo::open(&temp.path().join(".ds"), repo_dir.to_str().unwrap()).unwrap();
        source.clone_bare().unwrap();
        let default = source.default_branch().unwrap();
        let commit = source.resolve_source(&default).unwrap();
        assert!(is_oid(&commit));

        let entries = source.list_blobs(&commit).unwrap();
        let paths: Vec<_> = entries.iter().map(|entry| entry.path.as_str()).collect();
        assert!(paths.contains(&"a.py"));
        assert!(paths.contains(&"README.md"));

        let py = entries.iter().find(|entry| entry.path == "a.py").unwrap();
        let bytes = source.read_blob(&py.oid).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), "class Foo:\n    pass\n");
        assert!(source.blob_size(&py.oid).unwrap() > 0);
        assert_eq!(
            String::from_utf8(source.read_path(&commit, "a.py").unwrap()).unwrap(),
            "class Foo:\n    pass\n"
        );

        fs::write(repo_dir.join("a.py"), "class Foo:\n    value = 1\n").unwrap();
        commit_all(&repo_dir, "second");

        source.fetch().unwrap();
        let updated = source.resolve_source(&default).unwrap();
        assert_ne!(updated, commit);
        assert!(source.is_ancestor(&commit, &updated).unwrap());
        assert!(!source.is_ancestor(&updated, &commit).unwrap());
        let entries = source.list_blobs(&updated).unwrap();
        let py = entries.iter().find(|entry| entry.path == "a.py").unwrap();
        assert_eq!(
            String::from_utf8(source.read_blob(&py.oid).unwrap()).unwrap(),
            "class Foo:\n    value = 1\n"
        );
    }

    #[test]
    fn switches_between_branches_and_commits() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        init_repo(&repo_dir);

        let root = temp.path().join(".ds");
        let source = SourceRepo::open(&root, repo_dir.to_str().unwrap()).unwrap();
        source.clone_bare().unwrap();

        let default = source.default_branch().unwrap();
        source
            .set_current(&SourceRef::Branch(default.clone()))
            .unwrap();
        assert_eq!(
            source.current().unwrap(),
            SourceRef::Branch(default.clone())
        );
        let first = source.resolve_current().unwrap();

        fs::write(repo_dir.join("a.py"), "class Foo:\n    value = 1\n").unwrap();
        commit_all(&repo_dir, "second");
        let second = String::from_utf8(
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(&repo_dir)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned();
        // The clone is stale until an explicit fetch.
        assert_ne!(first, second);
        assert_eq!(source.resolve_current().unwrap(), first);

        source.fetch().unwrap();
        assert_eq!(source.resolve_source(&default).unwrap(), second);
        assert_eq!(source.resolve_current().unwrap(), second);

        let checked_out = source.checkout(&first).unwrap();
        assert_eq!(checked_out, SourceRef::Commit(first.clone()));
        assert_eq!(source.resolve_current().unwrap(), first);

        let checked_out = source.checkout(&default).unwrap();
        assert_eq!(checked_out, SourceRef::Branch(default.clone()));
        assert_eq!(source.resolve_current().unwrap(), second);
    }

    #[test]
    fn source_refs_are_remote_only() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        init_repo(&repo_dir);

        let source =
            SourceRepo::open(&temp.path().join(".ds"), repo_dir.to_str().unwrap()).unwrap();
        source.clone_bare().unwrap();
        let default = source.default_branch().unwrap();
        assert!(source.resolve_source(&default).is_ok());
        assert!(source.resolve_source(&format!("origin/{default}")).is_ok());
        assert!(source.resolve_source("HEAD").is_err());
        assert!(
            source
                .resolve_source(&format!("refs/heads/{default}"))
                .is_err()
        );
        assert!(source.resolve_source("origin/does-not-exist").is_err());
    }

    #[test]
    fn clone_and_fetch_keep_no_local_branches() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        init_repo(&repo_dir);

        let source =
            SourceRepo::open(&temp.path().join(".ds"), repo_dir.to_str().unwrap()).unwrap();
        source.clone_bare().unwrap();
        assert!(
            source
                .git(&["for-each-ref", "refs/heads"])
                .unwrap()
                .is_empty()
        );
        source.fetch().unwrap();
        assert!(
            source
                .git(&["for-each-ref", "refs/heads"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn lists_branches_and_log() {
        let temp = tempfile::tempdir().unwrap();
        let repo_dir = temp.path().join("repo");
        init_repo(&repo_dir);

        let source =
            SourceRepo::open(&temp.path().join(".ds"), repo_dir.to_str().unwrap()).unwrap();
        source.clone_bare().unwrap();
        let default = source.default_branch().unwrap();

        let branches = source.branches().unwrap();
        assert!(
            branches
                .iter()
                .any(|entry| entry.name == format!("origin/{default}"))
        );
        assert!(
            branches
                .iter()
                .all(|entry| entry.name.starts_with("origin/"))
        );

        fs::write(repo_dir.join("a.py"), "class Foo:\n    value = 1\n").unwrap();
        commit_all(&repo_dir, "second");
        source.fetch().unwrap();

        let head = source.resolve_source(&default).unwrap();
        let log = source.log(&head, 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "second");
        assert_eq!(log[1].subject, "init");
        assert!(is_oid(&log[0].commit));
    }
}
