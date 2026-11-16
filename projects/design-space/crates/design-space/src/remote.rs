//! Design-space remotes.
//!
//! A remote is a directory with the same object/doc/symbol/commit/pack layout
//! and `refs/heads/*`. It can be accessed directly on the filesystem, through
//! the HTTP API served by `ds-server`, or over SSH by running `ds-server
//! --stdio` on the remote host.

use crate::format;
use crate::model::{CommitHash, Hash32};
use crate::pack;
use crate::workspace::{State, Workspace};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const FILE_ROOTS: [&str; 5] = ["objects", "docs", "symbols", "commits", "packs"];
pub(crate) const MAX_TRANSFER: usize = 256 * 1024 * 1024;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum Remote {
    Local(LocalRemote),
    Http(HttpRemote),
    Ssh(SshRemote),
}

#[derive(Debug, Clone)]
pub struct LocalRemote {
    root: PathBuf,
}

#[derive(Debug)]
pub struct HttpRemote {
    base: String,
    client: Client,
    token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SshRemote {
    target: String,
    port: Option<u16>,
    root: String,
}

impl Remote {
    pub fn open(url: &str, token_file: Option<&Path>) -> Result<Self> {
        let url = url.trim();
        if let Some(path) = url.strip_prefix("file://") {
            return Ok(Self::Local(LocalRemote {
                root: PathBuf::from(path),
            }));
        }
        if url.starts_with("http://") || url.starts_with("https://") {
            let token = token_file.map(read_token_file).transpose()?;
            return Ok(Self::Http(HttpRemote::new(url, token)?));
        }
        if url.starts_with("ssh://") {
            return Ok(Self::Ssh(SshRemote::new(url)?));
        }
        if url.contains("://") {
            bail!("unsupported remote scheme: {url}");
        }
        Ok(Self::Local(LocalRemote {
            root: PathBuf::from(url),
        }))
    }

    pub fn display(&self) -> String {
        match self {
            Self::Local(local) => local.root.display().to_string(),
            Self::Http(http) => http.base.clone(),
            Self::Ssh(ssh) => ssh.url(),
        }
    }

    pub fn push(&self, workspace: &Workspace, branch: &str) -> Result<CommitHash> {
        match self {
            Self::Local(local) => local.push(workspace, branch),
            Self::Http(http) => http.push(workspace, branch),
            Self::Ssh(ssh) => ssh.push(workspace, branch),
        }
    }

    /// Fetch every remote branch and return `(branch, commit)` pairs.
    pub fn fetch(&self, workspace: &Workspace) -> Result<Vec<(String, CommitHash)>> {
        match self {
            Self::Local(local) => local.fetch(workspace),
            Self::Http(http) => http.fetch(workspace),
            Self::Ssh(ssh) => ssh.fetch(workspace),
        }
    }

    /// Fetch then fast-forward one branch. Refuses dirty index/worktree.
    pub fn pull(&self, workspace: &Workspace, branch: &str) -> Result<CommitHash> {
        self.fetch(workspace)?;
        let remote_commit = workspace
            .read_remote_ref(branch)?
            .with_context(|| format!("remote branch not found: {branch}"))?;
        if let Some(local) = workspace.read_ref(branch)? {
            if local == remote_commit {
                return Ok(local);
            }
            if !is_ancestor(workspace, local, remote_commit)? {
                bail!("pull is not a fast-forward for branch {branch}");
            }
            ensure_clean(workspace, local)?;
        }
        let commit = workspace.get_commit(remote_commit)?;
        workspace.write_ref(branch, remote_commit)?;
        let state = State {
            base_commit: remote_commit,
            root_view: commit.view,
        };
        workspace.write_index(state)?;
        workspace.write_worktree(state)?;
        Ok(remote_commit)
    }
}

impl LocalRemote {
    fn ensure(&self) -> Result<()> {
        for dir in ["objects", "docs", "symbols", "commits", "refs/heads"] {
            fs::create_dir_all(self.root.join(dir))?;
        }
        Ok(())
    }

    fn push(&self, workspace: &Workspace, branch: &str) -> Result<CommitHash> {
        self.ensure()?;
        let commit = workspace
            .read_ref(branch)?
            .with_context(|| format!("branch {branch} has no commit"))?;
        copy_layout(workspace.store().root(), &self.root)?;
        write_ref(&self.root, branch, commit)?;
        Ok(commit)
    }

    fn fetch(&self, workspace: &Workspace) -> Result<Vec<(String, CommitHash)>> {
        if !self.root.exists() {
            bail!("remote does not exist: {}", self.root.display());
        }
        copy_layout(&self.root, workspace.store().root())?;
        let refs = read_refs(&self.root)?;
        write_remote_refs(workspace, &refs)?;
        Ok(refs)
    }
}

impl HttpRemote {
    fn new(url: &str, token: Option<String>) -> Result<Self> {
        Ok(Self {
            base: url.trim_end_matches('/').to_owned(),
            client: Client::builder()
                .build()
                .context("create HTTP client failed")?,
            token,
        })
    }

    fn push(&self, workspace: &Workspace, branch: &str) -> Result<CommitHash> {
        let commit = workspace
            .read_ref(branch)?
            .with_context(|| format!("branch {branch} has no commit"))?;
        for (relative, path) in store_files(workspace.store().root())? {
            let bytes = fs::read(&path)?;
            self.put_bytes(&format!("/files/{relative}"), &bytes)?;
        }
        self.put_bytes(
            "/refs",
            format!("{branch}\t{}\n", commit.to_hex()).as_bytes(),
        )?;
        Ok(commit)
    }

    fn fetch(&self, workspace: &Workspace) -> Result<Vec<(String, CommitHash)>> {
        let refs = parse_refs(&self.get_text("/refs")?)?;
        let files = self.get_text("/files")?;
        let root = workspace.store().root();
        for relative in files.lines().filter(|line| !line.is_empty()) {
            if !valid_relative(Path::new(relative)) {
                bail!("invalid remote file path: {relative}");
            }
            let target = root.join(relative);
            if target.is_file() {
                continue;
            }
            let bytes = self.get_bytes(&format!("/files/{relative}"))?;
            publish(root, relative, &bytes)?;
        }
        write_remote_refs(workspace, &refs)?;
        Ok(refs)
    }

    fn get_text(&self, path: &str) -> Result<String> {
        let bytes = self.get_bytes(path)?;
        String::from_utf8(bytes).context("remote response is not UTF-8")
    }

    fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let mut request = self.client.get(format!("{}{}", self.base, path));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .with_context(|| format!("GET {path}"))?
            .error_for_status()
            .with_context(|| format!("GET {path}"))?;
        let mut reader = response.take(MAX_TRANSFER as u64 + 1);
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .with_context(|| format!("read GET {path} response"))?;
        if bytes.len() > MAX_TRANSFER {
            bail!("remote response is too large: {path}");
        }
        Ok(bytes)
    }

    fn put_bytes(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut request = self
            .client
            .put(format!("{}{}", self.base, path))
            .body(bytes.to_vec());
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        request
            .send()
            .with_context(|| format!("PUT {path}"))?
            .error_for_status()
            .with_context(|| format!("PUT {path}"))?;
        Ok(())
    }
}

impl SshRemote {
    fn new(url: &str) -> Result<Self> {
        if url.contains('\0') {
            bail!("SSH URL contains NUL");
        }
        let rest = url.strip_prefix("ssh://").expect("checked by caller");
        let (authority, path) = rest
            .split_once('/')
            .with_context(|| format!("SSH URL has no absolute path: {url}"))?;
        if authority.is_empty() {
            bail!("SSH URL has no host: {url}");
        }
        if path.is_empty() {
            bail!("SSH URL has no path: {url}");
        }
        let root = format!("/{path}");
        let (user, host_port) = match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user), host),
            None => (None, authority),
        };
        if let Some(user) = user
            && !valid_ssh_part(user)
        {
            bail!("invalid SSH user: {user}");
        }
        let (host, port) = split_host_port(host_port)?;
        if !valid_ssh_part(host) {
            bail!("invalid SSH host: {host}");
        }
        let ssh_host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let target = match user {
            Some(user) => format!("{user}@{ssh_host}"),
            None => ssh_host,
        };
        Ok(Self { target, port, root })
    }

    fn url(&self) -> String {
        let port = self.port.map(|port| format!(":{port}")).unwrap_or_default();
        format!("ssh://{}{port}{}", self.target, self.root)
    }

    fn push(&self, workspace: &Workspace, branch: &str) -> Result<CommitHash> {
        let commit = workspace
            .read_ref(branch)?
            .with_context(|| format!("branch {branch} has no commit"))?;
        let mut session = SshSession::open(self)?;
        for (relative, path) in store_files(workspace.store().root())? {
            let bytes = fs::read(&path)?;
            session.request(
                &format!("PUT {} {}", relative.len(), bytes.len()),
                &[relative.as_bytes(), &bytes],
            )?;
        }
        let body = format!("{branch}\t{}\n", commit.to_hex());
        session.request(&format!("PUTREF {}", body.len()), &[body.as_bytes()])?;
        Ok(commit)
    }

    fn fetch(&self, workspace: &Workspace) -> Result<Vec<(String, CommitHash)>> {
        let mut session = SshSession::open(self)?;
        let refs = parse_refs(&String::from_utf8(session.request("REFS", &[])?)?)?;
        let files = String::from_utf8(session.request("FILES", &[])?)?;
        let root = workspace.store().root();
        for relative in files.lines().filter(|line| !line.is_empty()) {
            if !valid_relative(Path::new(relative)) {
                bail!("invalid remote file path: {relative}");
            }
            let target = root.join(relative);
            if target.is_file() {
                continue;
            }
            let bytes =
                session.request(&format!("GET {}", relative.len()), &[relative.as_bytes()])?;
            publish(root, relative, &bytes)?;
        }
        write_remote_refs(workspace, &refs)?;
        Ok(refs)
    }
}

struct SshSession {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl SshSession {
    fn open(remote: &SshRemote) -> Result<Self> {
        let mut command = Command::new("ssh");
        command.arg("-T");
        if let Some(port) = remote.port {
            command.args(["-p", &port.to_string()]);
        }
        command.arg(&remote.target);
        command.arg(format!(
            "ds-server --stdio --root {}",
            shell_quote(&remote.root)
        ));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        let mut child = command.spawn().context("spawn ssh failed")?;
        let stdin = child.stdin.take().context("ssh stdin unavailable")?;
        let stdout = child.stdout.take().context("ssh stdout unavailable")?;
        Ok(Self {
            child,
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, header: &str, blobs: &[&[u8]]) -> Result<Vec<u8>> {
        let stdin = self.stdin.as_mut().context("SSH session is closed")?;
        stdin.write_all(header.as_bytes())?;
        stdin.write_all(b"\n")?;
        for blob in blobs {
            stdin.write_all(blob)?;
        }
        stdin.flush()?;

        let mut line = String::new();
        if self.stdout.read_line(&mut line)? == 0 {
            bail!("SSH server closed the connection");
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let (status, length) = line
            .split_once(' ')
            .with_context(|| format!("malformed SSH response: {line}"))?;
        let length: usize = length
            .parse()
            .with_context(|| format!("invalid SSH response length: {line}"))?;
        if length > MAX_TRANSFER {
            bail!("SSH response is too large");
        }
        let mut data = vec![0u8; length];
        self.stdout.read_exact(&mut data)?;
        match status {
            "OK" => Ok(data),
            "ERR" => bail!("{}", String::from_utf8_lossy(&data)),
            _ => bail!("malformed SSH response: {line}"),
        }
    }
}

impl Drop for SshSession {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn is_ancestor(
    workspace: &Workspace,
    ancestor: CommitHash,
    descendant: CommitHash,
) -> Result<bool> {
    let mut current = descendant;
    while !current.is_zero() {
        if current == ancestor {
            return Ok(true);
        }
        current = workspace.get_commit(current)?.parent;
    }
    Ok(false)
}

fn ensure_clean(workspace: &Workspace, head: CommitHash) -> Result<()> {
    let view = workspace.get_commit(head)?.view;
    if let Some(index) = workspace.read_index()?
        && index.root_view != view
    {
        bail!("index has changes; commit or reset before pulling");
    }
    if let Some(worktree) = workspace.read_worktree()?
        && worktree.root_view != view
    {
        bail!("worktree has changes; commit or reset before pulling");
    }
    Ok(())
}

fn write_remote_refs(workspace: &Workspace, refs: &[(String, CommitHash)]) -> Result<()> {
    for (branch, commit) in refs {
        workspace
            .get_commit(*commit)
            .with_context(|| format!("remote ref {branch} points at a missing commit"))?;
    }
    for (branch, commit) in refs {
        workspace.write_remote_ref(branch, *commit)?;
    }
    Ok(())
}

pub(crate) fn parse_refs(text: &str) -> Result<Vec<(String, CommitHash)>> {
    let mut refs = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (branch, hash) = line
            .split_once('\t')
            .with_context(|| format!("malformed remote ref: {line}"))?;
        if !valid_branch(branch) {
            bail!("invalid remote branch: {branch}");
        }
        let hash = parse_hash(hash).with_context(|| format!("invalid remote ref hash: {hash}"))?;
        if hash.is_zero() {
            bail!("remote ref is zero");
        }
        refs.insert(branch.to_owned(), CommitHash(hash));
    }
    Ok(refs.into_iter().collect())
}

fn read_refs(root: &Path) -> Result<Vec<(String, CommitHash)>> {
    let dir = root.join("refs/heads");
    let mut files = Vec::new();
    collect_files(&dir, &mut files)?;
    let mut refs = Vec::new();
    for path in files {
        let branch = path
            .strip_prefix(&dir)?
            .to_string_lossy()
            .replace('\\', "/");
        if !valid_branch(&branch) {
            bail!("invalid remote branch: {branch}");
        }
        let hash = fs::read_to_string(&path)?;
        let hash =
            parse_hash(hash.trim()).with_context(|| format!("invalid remote ref for {branch}"))?;
        if hash.is_zero() {
            bail!("remote ref is zero: {branch}");
        }
        refs.push((branch, CommitHash(hash)));
    }
    refs.sort();
    Ok(refs)
}

fn copy_layout(source: &Path, target: &Path) -> Result<()> {
    for dir in FILE_ROOTS {
        copy_tree(&source.join(dir), &target.join(dir))?;
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    if !source.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            fs::create_dir_all(&target_path)?;
            copy_tree(&source_path, &target_path)?;
        } else if source_path.is_file() {
            if let Some(parent) = target_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if !target_path.exists() {
                fs::copy(&source_path, &target_path)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn write_ref(root: &Path, branch: &str, commit: CommitHash) -> Result<()> {
    if !valid_branch(branch) {
        bail!("invalid branch name: {branch}");
    }
    let path = root.join("refs/heads").join(branch);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = root.join("tmp").join(format!(
        "ref-{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(root.join("tmp"))?;
    fs::write(&temp, format!("{}\n", commit.to_hex()))?;
    fs::rename(temp, path)?;
    Ok(())
}

fn store_files(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for dir in FILE_ROOTS {
        collect_files(&root.join(dir), &mut files)?;
    }
    let mut out = Vec::with_capacity(files.len());
    for path in files {
        let relative = path
            .strip_prefix(root)?
            .to_string_lossy()
            .replace('\\', "/");
        if !valid_relative(Path::new(&relative)) {
            bail!("invalid local file path: {relative}");
        }
        out.push((relative, path));
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

pub(crate) fn publish(root: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    verify_upload(Path::new(relative), bytes)?;
    if bytes.len() > MAX_TRANSFER {
        bail!("remote file is too large: {relative}");
    }
    let target = root.join(relative);
    if target.is_file() {
        return Ok(());
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = root.join("tmp");
    fs::create_dir_all(&tmp)?;
    let temp = tmp.join(format!(
        "fetch-{}-{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&temp, bytes)?;
    match fs::rename(&temp, &target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error.into())
        }
    }
}

pub(crate) fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

/// Validate a path received from a remote before writing it under `.ds`.
pub(crate) fn valid_relative(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    if components.is_empty()
        || !components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return false;
    }
    let Some(Component::Normal(root)) = components.first() else {
        return false;
    };
    let Some(root) = root.to_str() else {
        return false;
    };
    if !FILE_ROOTS.contains(&root) {
        return false;
    }
    if root == "packs" {
        if components.len() != 2 {
            return false;
        }
        let Some(Component::Normal(name)) = components.get(1) else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return false;
        };
        let Some(stem) = name.strip_suffix(".pack") else {
            return false;
        };
        return stem
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            && stem.len() == 64;
    }
    if components.len() != 4 {
        return false;
    }
    let Some(Component::Normal(aa)) = components.get(1) else {
        return false;
    };
    let Some(Component::Normal(bb)) = components.get(2) else {
        return false;
    };
    let Some(Component::Normal(name)) = components.get(3) else {
        return false;
    };
    let (Some(aa), Some(bb), Some(name)) = (aa.to_str(), bb.to_str(), name.to_str()) else {
        return false;
    };
    if name.len() != 64
        || !name
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        || aa != &name[..2]
        || bb != &name[2..4]
    {
        return false;
    }
    true
}

pub(crate) fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains('\\')
        && !branch
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        && branch
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(crate) fn parse_hash(text: &str) -> Option<Hash32> {
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
        bytes[index] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(Hash32(bytes))
}

/// Read a bearer token from a file. The file contains one token; surrounding
/// whitespace is ignored.
pub fn read_token_file(path: &Path) -> Result<String> {
    let text =
        fs::read_to_string(path).with_context(|| format!("read token file: {}", path.display()))?;
    let token = text.trim();
    if token.is_empty() {
        bail!("token file is empty: {}", path.display());
    }
    if token.len() > 4096 {
        bail!("token is too long: {}", path.display());
    }
    if token
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("token contains whitespace or control characters");
    }
    Ok(token.to_owned())
}

/// Check that an uploaded file is stored under its own content hash.
pub(crate) fn verify_upload(relative: &Path, bytes: &[u8]) -> Result<()> {
    if !valid_relative(relative) {
        bail!("invalid file path: {}", relative.display());
    }
    let components: Vec<_> = relative.components().collect();
    let Some(Component::Normal(root)) = components.first() else {
        unreachable!("validated above");
    };
    let root = root.to_str().expect("validated above");
    if root == "packs" {
        let Some(Component::Normal(name)) = components.get(1) else {
            unreachable!("validated above");
        };
        let name = name.to_str().expect("validated above");
        let stem = name.strip_suffix(".pack").expect("validated above");
        let expected = parse_hash(stem).expect("validated above");
        let actual = pack::validate_pack(bytes).context("invalid pack upload")?;
        if actual != expected {
            bail!("pack upload does not match its file name");
        }
        return Ok(());
    }
    let Some(Component::Normal(name)) = components.get(3) else {
        unreachable!("validated above");
    };
    let name = name.to_str().expect("validated above");
    let hash = parse_hash(name).expect("validated above");
    match root {
        "objects" => {
            format::decode_object_checked(bytes, crate::model::ObjectHash(hash))
                .context("invalid object upload")?;
        }
        "commits" => {
            format::decode_commit_checked(bytes, CommitHash(hash))
                .context("invalid commit upload")?;
        }
        "docs" => {
            if format::hash_bytes(bytes) != hash {
                bail!("doc upload does not match its hash");
            }
            if std::str::from_utf8(bytes).is_err() {
                bail!("doc upload is not UTF-8");
            }
        }
        "symbols" => {
            if format::hash_bytes(bytes) != hash {
                bail!("symbol upload does not match its hash");
            }
            if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
                bail!("invalid symbol upload");
            }
        }
        _ => unreachable!("validated above"),
    }
    Ok(())
}

fn split_host_port(authority: &str) -> Result<(&str, Option<u16>)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, rest) = rest
            .split_once(']')
            .with_context(|| format!("invalid SSH address: {authority}"))?;
        if rest.is_empty() {
            return Ok((host, None));
        }
        let port = rest
            .strip_prefix(':')
            .with_context(|| format!("invalid SSH address: {authority}"))?;
        return Ok((host, Some(parse_port(port)?)));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            Ok((host, Some(parse_port(port)?)))
        }
        Some(_) => bail!("invalid SSH address: {authority}"),
        None => Ok((authority, None)),
    }
}

fn parse_port(text: &str) -> Result<u16> {
    let port: u16 = text
        .parse()
        .with_context(|| format!("invalid SSH port: {text}"))?;
    if port == 0 {
        bail!("invalid SSH port: 0");
    }
    Ok(port)
}

fn valid_ssh_part(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with('-')
        && !text
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        && !text.contains('\\')
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_canonical_stored_paths() {
        let hex = "ab".repeat(32);
        let path = format!("objects/{}/{}/{hex}", &hex[..2], &hex[2..4]);
        assert!(valid_relative(Path::new(&path)));
        assert!(valid_relative(Path::new(&format!("packs/{hex}.pack"))));
        assert!(!valid_relative(Path::new(&hex)));
        assert!(!valid_relative(Path::new("../objects/aa/bb/hash")));
        assert!(!valid_relative(Path::new(&format!(
            "packs/{}/{}/{hex}",
            &hex[..2],
            &hex[2..4]
        ))));
        assert!(!valid_relative(Path::new(&format!("objects/00/00/{hex}"))));
        assert!(!valid_relative(Path::new(&format!(
            "objects/{}/{}/{}",
            &hex[..2],
            &hex[2..4],
            hex.to_uppercase()
        ))));
    }

    #[test]
    fn parses_ssh_urls() {
        let remote = SshRemote::new("ssh://user@example.com:2222/srv/ds").unwrap();
        assert_eq!(remote.target, "user@example.com");
        assert_eq!(remote.port, Some(2222));
        assert_eq!(remote.root, "/srv/ds");

        let remote = SshRemote::new("ssh://[::1]/srv/ds").unwrap();
        assert_eq!(remote.target, "[::1]");
        assert_eq!(remote.port, None);

        assert!(SshRemote::new("ssh://host").is_err());
        assert!(SshRemote::new("ssh://host/").is_err());
        assert!(SshRemote::new("ssh://-oProxyCommand=x/srv").is_err());
    }

    #[test]
    fn parses_and_validates_remote_refs() {
        let hash = "ab".repeat(32);
        let refs = parse_refs(&format!("main\t{hash}\nfeature/x\t{hash}\n")).unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].0, "feature/x");
        assert!(parse_refs("main\tbad\n").is_err());
        assert!(parse_refs(&format!("--bad\t{hash}\n")).is_err());
        assert!(parse_refs("\tbad\n").is_err());
    }

    #[test]
    fn reads_token_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "secret\n").unwrap();
        assert_eq!(read_token_file(&path).unwrap(), "secret");

        std::fs::write(&path, "\n\t").unwrap();
        assert!(read_token_file(&path).is_err());
        std::fs::write(&path, "two words").unwrap();
        assert!(read_token_file(&path).is_err());
        assert!(read_token_file(&dir.path().join("missing")).is_err());
    }
}
