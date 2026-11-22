//! Workspace metadata for the format.
//!
//! This layer sits above [`crate::store::Store`] and owns:
//!
//! - `.ds/repo.toml` (repository source metadata);
//! - `.ds/config` (workspace-local settings such as the remote URL);
//! - `.ds/refs/heads/*`;
//! - `.ds/HEAD`;
//! - `.ds/index` and `.ds/worktree`.
//!
//! It deliberately does not implement the conceptual-element CLI.

use crate::model::{Commit, CommitHash, DesignObject, DocHash, Hash32, ObjectHash, SymbolHash};
use crate::store::{Store, StoreError};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const STATE_VERSION: u8 = 1;
const INDEX_MAGIC: &[u8; 4] = b"IDX1";
const WORKTREE_MAGIC: &[u8; 4] = b"WRK1";

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("config: {0}")]
    Config(String),
    #[error("invalid ref: {0}")]
    InvalidRef(String),
    #[error("invalid state file: {0}")]
    InvalidState(String),
}

pub type Result<T> = std::result::Result<T, WorkspaceError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObjectFormat {
    Sha1,
    Sha256,
}

impl ObjectFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "sha1",
            Self::Sha256 => "sha256",
        }
    }
}

/// Repository metadata file. It lives at the remote repository root and is
/// fetched into `.ds/repo.toml` for every workspace.
pub const REPO_CONFIG_FILE: &str = "repo.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    /// Legacy location for source metadata. New workspaces keep it only in
    /// `repo.toml`; `Workspace::open` migrates old configs automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<SourceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SourceConfig {
    pub(crate) url: String,
    pub(crate) object_format: ObjectFormat,
}

/// Repository-level metadata stored as `repo.toml` on the remote. It is not a
/// design object and is not versioned; it records how to interpret the design
/// space and where its read-only source repository lives.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepoConfig {
    pub(crate) source: SourceConfig,
}

impl RepoConfig {
    pub(crate) fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(|error| WorkspaceError::Config(error.to_string()))
    }

    pub(crate) fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).map_err(|error| WorkspaceError::Config(error.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    meta: PathBuf,
    store: Store,
    config: Config,
    repo: RepoConfig,
}

impl Workspace {
    /// Initialize `.ds` metadata under `meta_root` and write `repo.toml`.
    pub fn init(
        meta_root: impl Into<PathBuf>,
        source_url: impl Into<String>,
        object_format: ObjectFormat,
    ) -> Result<Self> {
        let repo = RepoConfig {
            source: SourceConfig {
                url: source_url.into(),
                object_format,
            },
        };
        Self::create(meta_root.into(), repo, true)
    }

    /// Create the store and config for a clone before `repo.toml` is fetched.
    /// The temporary in-memory source is replaced by [`Self::install_repo_config`].
    pub fn bootstrap(meta_root: impl Into<PathBuf>) -> Result<Self> {
        let repo = RepoConfig {
            source: SourceConfig {
                url: String::new(),
                object_format: ObjectFormat::Sha1,
            },
        };
        Self::create(meta_root.into(), repo, false)
    }

    fn create(meta: PathBuf, repo: RepoConfig, write_repo: bool) -> Result<Self> {
        fs::create_dir_all(&meta)?;
        let store = Store::open(&meta)?;
        for dir in ["refs/heads", "refs/remotes"] {
            fs::create_dir_all(meta.join(dir))?;
        }
        let config = Config {
            source: None,
            remote: None,
        };
        write_atomic(&meta, Path::new("config"), toml_bytes(&config)?.as_slice())?;
        write_atomic(&meta, Path::new("HEAD"), b"ref: refs/heads/main\n")?;
        if write_repo {
            write_repo_metadata(&meta, &repo)?;
        }
        Ok(Self {
            meta,
            store,
            config,
            repo,
        })
    }

    pub fn open(meta_root: impl Into<PathBuf>) -> Result<Self> {
        let meta = meta_root.into();
        let store = Store::open(&meta)?;
        let bytes = fs::read(meta.join("config"))?;
        let text = String::from_utf8(bytes)
            .map_err(|_| WorkspaceError::Config("config is not UTF-8".into()))?;
        let mut config: Config =
            toml::from_str(&text).map_err(|error| WorkspaceError::Config(error.to_string()))?;
        let repo = match read_repo_metadata(&meta)? {
            Some(repo) => repo,
            None => {
                let Some(source) = config.source.take() else {
                    return Err(WorkspaceError::Config(format!(
                        "workspace is missing {REPO_CONFIG_FILE}"
                    )));
                };
                let repo = RepoConfig { source };
                write_repo_metadata(&meta, &repo)?;
                write_atomic(&meta, Path::new("config"), toml_bytes(&config)?.as_slice())?;
                repo
            }
        };
        Ok(Self {
            meta,
            store,
            config,
            repo,
        })
    }

    pub fn meta_root(&self) -> &Path {
        &self.meta
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn source_url(&self) -> &str {
        &self.repo.source.url
    }

    pub fn object_format(&self) -> ObjectFormat {
        self.repo.source.object_format
    }

    pub fn remote_url(&self) -> Option<&str> {
        self.config.remote.as_deref()
    }

    /// Install repository metadata after a clone fetched `repo.toml`.
    pub fn install_repo_config(
        &mut self,
        url: impl Into<String>,
        object_format: ObjectFormat,
    ) -> Result<()> {
        self.repo = RepoConfig {
            source: SourceConfig {
                url: url.into(),
                object_format,
            },
        };
        write_repo_metadata(&self.meta, &self.repo)
    }

    pub fn set_remote_url(&mut self, url: impl Into<String>) -> Result<()> {
        self.config.remote = Some(url.into());
        self.write_config()
    }

    fn write_config(&self) -> Result<()> {
        write_atomic(
            &self.meta,
            Path::new("config"),
            toml_bytes(&self.config)?.as_slice(),
        )?;
        Ok(())
    }

    pub fn put_object(&self, object: &DesignObject) -> Result<ObjectHash> {
        Ok(self.store.put_object(object)?)
    }

    pub fn get_object(&self, hash: ObjectHash) -> Result<DesignObject> {
        Ok(self.store.get_object(hash)?)
    }

    pub fn put_doc(&self, text: &str) -> Result<DocHash> {
        Ok(self.store.put_doc(text)?)
    }

    pub fn get_doc(&self, hash: DocHash) -> Result<String> {
        Ok(self.store.get_doc(hash)?)
    }

    pub fn put_symbol(&self, text: &str) -> Result<SymbolHash> {
        Ok(self.store.put_symbol(text)?)
    }

    pub fn get_symbol(&self, hash: SymbolHash) -> Result<String> {
        Ok(self.store.get_symbol(hash)?)
    }

    pub fn put_commit(&self, commit: &Commit) -> Result<CommitHash> {
        Ok(self.store.put_commit(commit)?)
    }

    pub fn get_commit(&self, hash: CommitHash) -> Result<Commit> {
        Ok(self.store.get_commit(hash)?)
    }

    pub fn read_head(&self) -> Result<String> {
        let text = fs::read_to_string(self.meta.join("HEAD"))?;
        Ok(text.trim().to_owned())
    }

    pub fn write_head(&self, target: &str) -> Result<()> {
        Ok(write_atomic(
            &self.meta,
            Path::new("HEAD"),
            target.as_bytes(),
        )?)
    }

    pub fn read_ref(&self, branch: &str) -> Result<Option<CommitHash>> {
        validate_branch(branch)?;
        let path = self.meta.join("refs/heads").join(branch);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let hash = parse_hash(text)
            .ok_or_else(|| WorkspaceError::InvalidRef(format!("{branch}: {text}")))?;
        if hash.is_zero() {
            return Err(WorkspaceError::InvalidRef(format!("{branch}: zero commit")));
        }
        Ok(Some(CommitHash(hash)))
    }

    pub fn write_ref(&self, branch: &str, commit: CommitHash) -> Result<()> {
        validate_branch(branch)?;
        if commit.is_zero() {
            return Err(WorkspaceError::InvalidRef(format!("{branch}: zero commit")));
        }
        let path = Path::new("refs/heads").join(branch);
        Ok(write_atomic(
            &self.meta,
            &path,
            format!("{}\n", commit.to_hex()).as_bytes(),
        )?)
    }

    pub fn read_remote_ref(&self, branch: &str) -> Result<Option<CommitHash>> {
        validate_branch(branch)?;
        let path = self.meta.join("refs/remotes/origin").join(branch);
        read_ref_file(&path, branch)
    }

    pub fn write_remote_ref(&self, branch: &str, commit: CommitHash) -> Result<()> {
        validate_branch(branch)?;
        if commit.is_zero() {
            return Err(WorkspaceError::InvalidRef(format!("{branch}: zero commit")));
        }
        let path = Path::new("refs/remotes/origin").join(branch);
        Ok(write_atomic(
            &self.meta,
            &path,
            format!("{}\n", commit.to_hex()).as_bytes(),
        )?)
    }

    pub fn read_index(&self) -> Result<Option<State>> {
        read_state(&self.meta, "index", INDEX_MAGIC)
    }

    pub fn write_index(&self, state: State) -> Result<()> {
        Ok(write_atomic(
            &self.meta,
            Path::new("index"),
            &encode_state(INDEX_MAGIC, state),
        )?)
    }

    pub fn read_worktree(&self) -> Result<Option<State>> {
        read_state(&self.meta, "worktree", WORKTREE_MAGIC)
    }

    pub fn write_worktree(&self, state: State) -> Result<()> {
        Ok(write_atomic(
            &self.meta,
            Path::new("worktree"),
            &encode_state(WORKTREE_MAGIC, state),
        )?)
    }
}

/// Mutable index/worktree state. Zero hashes mean "absent".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct State {
    pub base_commit: CommitHash,
    pub root_view: ObjectHash,
}

fn toml_bytes(config: &Config) -> Result<Vec<u8>> {
    let text = toml::to_string_pretty(config)
        .map_err(|error| WorkspaceError::Config(error.to_string()))?;
    Ok(text.into_bytes())
}

fn read_repo_metadata(meta: &Path) -> Result<Option<RepoConfig>> {
    let bytes = match fs::read(meta.join(REPO_CONFIG_FILE)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let text = String::from_utf8(bytes)
        .map_err(|_| WorkspaceError::Config(format!("{REPO_CONFIG_FILE} is not UTF-8")))?;
    Ok(Some(RepoConfig::from_toml(&text)?))
}

fn write_repo_metadata(meta: &Path, repo: &RepoConfig) -> Result<()> {
    let text = repo.to_toml()?;
    Ok(write_atomic(
        meta,
        Path::new(REPO_CONFIG_FILE),
        text.as_bytes(),
    )?)
}

fn encode_state(magic: &[u8; 4], state: State) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 64);
    out.extend_from_slice(magic);
    out.push(STATE_VERSION);
    out.extend_from_slice(&[0, 0, 0]);
    out.extend_from_slice(&state.base_commit.0.0);
    out.extend_from_slice(&state.root_view.0.0);
    out
}

fn read_state(meta: &Path, name: &str, magic: &[u8; 4]) -> Result<Option<State>> {
    let path = meta.join(name);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if bytes.len() != 72 {
        return Err(WorkspaceError::InvalidState(format!(
            "{name}: expected 72 bytes, got {}",
            bytes.len()
        )));
    }
    if &bytes[0..4] != magic {
        return Err(WorkspaceError::InvalidState(format!("{name}: bad magic")));
    }
    if bytes[4] != STATE_VERSION {
        return Err(WorkspaceError::InvalidState(format!(
            "{name}: unsupported version {}",
            bytes[4]
        )));
    }
    if bytes[5..8] != [0, 0, 0] {
        return Err(WorkspaceError::InvalidState(format!(
            "{name}: reserved bytes must be zero"
        )));
    }
    let mut base = [0u8; 32];
    base.copy_from_slice(&bytes[8..40]);
    let mut root = [0u8; 32];
    root.copy_from_slice(&bytes[40..72]);
    Ok(Some(State {
        base_commit: CommitHash::from_bytes(base),
        root_view: ObjectHash::from_bytes(root),
    }))
}

fn read_ref_file(path: &Path, branch: &str) -> Result<Option<CommitHash>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let hash =
        parse_hash(text).ok_or_else(|| WorkspaceError::InvalidRef(format!("{branch}: {text}")))?;
    if hash.is_zero() {
        return Err(WorkspaceError::InvalidRef(format!("{branch}: zero commit")));
    }
    Ok(Some(CommitHash(hash)))
}

fn validate_branch(branch: &str) -> Result<()> {
    if branch.is_empty()
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.contains('\\')
        || branch.as_bytes().contains(&0)
    {
        return Err(WorkspaceError::InvalidRef(branch.to_owned()));
    }
    for component in Path::new(branch).components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err(WorkspaceError::InvalidRef(branch.to_owned()));
        }
    }
    Ok(())
}

fn parse_hash(text: &str) -> Option<Hash32> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, byte) in out.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&text[start..start + 2], 16).ok()?;
    }
    Some(Hash32(out))
}

fn write_atomic(root: &Path, relative: &Path, bytes: &[u8]) -> io::Result<()> {
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let nonce = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = root.join("tmp").join(format!(
        "{}-{}-{}.tmp",
        relative
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        std::process::id(),
        nonce
    ));
    fs::create_dir_all(root.join("tmp"))?;
    {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temp, &target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempdir().unwrap();
        let ws = Workspace::init(
            dir.path().join(".ds"),
            "https://example.com/repo.git",
            ObjectFormat::Sha1,
        )
        .unwrap();
        (dir, ws)
    }

    #[test]
    fn initializes_and_reopens_config() {
        let (_dir, ws) = workspace();
        assert_eq!(ws.source_url(), "https://example.com/repo.git");
        assert_eq!(ws.object_format(), ObjectFormat::Sha1);
        assert_eq!(ws.read_head().unwrap(), "ref: refs/heads/main");
        let config = fs::read_to_string(ws.meta_root().join("config")).unwrap();
        assert!(!config.contains("source"));
        let repo = fs::read_to_string(ws.meta_root().join(REPO_CONFIG_FILE)).unwrap();
        assert!(repo.contains("https://example.com/repo.git"));
        let reopened = Workspace::open(ws.meta_root()).unwrap();
        assert_eq!(reopened.source_url(), ws.source_url());
        assert_eq!(reopened.object_format(), ws.object_format());
    }

    #[test]
    fn migrates_legacy_config_source_into_repo_toml() {
        let dir = tempdir().unwrap();
        let meta = dir.path().join(".ds");
        fs::create_dir_all(&meta).unwrap();
        fs::write(
            meta.join("config"),
            "remote = \"file:///remote\"\n\n[source]\nurl = \"https://legacy.example/repo.git\"\nobject_format = \"sha256\"\n",
        )
        .unwrap();

        let ws = Workspace::open(&meta).unwrap();
        assert_eq!(ws.source_url(), "https://legacy.example/repo.git");
        assert_eq!(ws.object_format(), ObjectFormat::Sha256);
        assert!(meta.join(REPO_CONFIG_FILE).is_file());
        let config = fs::read_to_string(meta.join("config")).unwrap();
        assert!(!config.contains("[source]"));
        assert!(config.contains("file:///remote"));
    }

    #[test]
    fn persists_remote() {
        let (_dir, mut ws) = workspace();
        ws.set_remote_url("https://example.com/ds").unwrap();
        let reopened = Workspace::open(ws.meta_root()).unwrap();
        assert_eq!(reopened.remote_url(), Some("https://example.com/ds"));
    }

    #[test]
    fn writes_and_reads_refs() {
        let (_dir, ws) = workspace();
        let hash = CommitHash::from_bytes([7; 32]);
        ws.write_ref("main", hash).unwrap();
        assert_eq!(ws.read_ref("main").unwrap(), Some(hash));
        assert_eq!(ws.read_ref("missing").unwrap(), None);
    }

    #[test]
    fn roundtrips_index_and_worktree() {
        let (_dir, ws) = workspace();
        let state = State {
            base_commit: CommitHash::from_bytes([1; 32]),
            root_view: ObjectHash::from_bytes([2; 32]),
        };
        ws.write_index(state).unwrap();
        ws.write_worktree(state).unwrap();
        assert_eq!(ws.read_index().unwrap(), Some(state));
        assert_eq!(ws.read_worktree().unwrap(), Some(state));
    }

    #[test]
    fn rejects_bad_ref_and_state() {
        let (_dir, ws) = workspace();
        assert!(matches!(
            ws.write_ref("main", CommitHash::ZERO),
            Err(WorkspaceError::InvalidRef(_))
        ));
        std::fs::write(ws.meta_root().join("index"), b"bad").unwrap();
        assert!(matches!(
            ws.read_index(),
            Err(WorkspaceError::InvalidState(_))
        ));
    }

    #[test]
    fn rejects_path_traversal_branches() {
        let (_dir, ws) = workspace();
        let hash = CommitHash::from_bytes([7; 32]);
        for branch in ["", "/abs", "../x", "a/../../b", "a\\b"] {
            assert!(matches!(
                ws.write_ref(branch, hash),
                Err(WorkspaceError::InvalidRef(_))
            ));
        }
        assert!(ws.read_ref("feature/ok").is_ok());
    }
}
