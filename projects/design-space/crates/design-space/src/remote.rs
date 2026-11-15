//! Remote sync: server storage, pack transfer, HTTP and SSH transports.
//!
//! The protocol is small and content-addressed:
//!
//! - `refs`     → branch → version hash;
//! - `fetch`    → (want, have) → a pack of the missing closure;
//! - `pack`     → raw pack bytes, verified and unpacked into the store;
//! - `push`     → { ref, old, new } → ok / conflict / missing.
//!
//! HTTP exposes these as `/v1/...`. SSH runs the same messages over
//! `ds serve --stdio`, which is meant to be started by `sshd` (or by
//! `ssh host ds serve --stdio --repo <dir>`); the client speaks the framed
//! protocol through the system `ssh` command.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hash::{Hash, from_hex, is_zero, to_hex};
use crate::node::Node;
use crate::pack::{self, Pack};
use crate::space::{self, load_version};
use crate::store::Store;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefsResponse {
    pub refs: BTreeMap<String, String>,
    /// Source repository clients should clone; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchRequest {
    pub want: String,
    pub have: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub old: Option<String>,
    pub new: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushResult {
    /// `ok`, `conflict` or `missing`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

impl PushResult {
    pub fn ok() -> Self {
        Self {
            status: "ok".into(),
            current: None,
            missing: Vec::new(),
        }
    }

    pub fn conflict(current: Option<String>) -> Self {
        Self {
            status: "conflict".into(),
            current,
            missing: Vec::new(),
        }
    }

    pub fn missing(missing: Vec<String>) -> Self {
        Self {
            status: "missing".into(),
            current: None,
            missing,
        }
    }
}

#[derive(Debug, Default)]
pub struct PushSummary {
    pub up_to_date: bool,
    pub version: String,
    pub nodes: usize,
}

#[derive(Debug, Default)]
pub struct FetchSummary {
    pub up_to_date: bool,
    pub version: String,
    pub nodes: usize,
}

/// Server-side repository: loose nodes, refs and archived packs.
pub struct ServerRepo {
    root: PathBuf,
    store: Store,
    source: Option<String>,
}

impl ServerRepo {
    pub fn open(root: &Path) -> Result<Self> {
        Self::open_with_source(root, None)
    }

    /// Server repository that advertises its source repository in `refs`.
    pub fn open_with_source(root: &Path, source: Option<String>) -> Result<Self> {
        for dir in ["objects", "packs", "refs/heads", "tmp"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
            store: Store::new(root),
            source,
        })
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn refs(&self) -> Result<BTreeMap<String, String>> {
        let heads = self.root.join("refs").join("heads");
        let mut refs = BTreeMap::new();
        let Ok(entries) = fs::read_dir(&heads) else {
            return Ok(refs);
        };
        for entry in entries {
            let entry = entry?;
            if !entry.path().is_file() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if let Some(version) = self.read_ref_hex(&name)? {
                refs.insert(name, version);
            }
        }
        Ok(refs)
    }

    pub fn read_ref_hex(&self, name: &str) -> Result<Option<String>> {
        Ok(space::read_ref_optional(&self.root, name)?.map(|hash| to_hex(&hash)))
    }

    /// Verify a pack, unpack it into the object store, archive the pack.
    pub fn receive_pack(&self, bytes: &[u8]) -> Result<String> {
        let pack = Pack::from_bytes(bytes.to_vec())?;
        for hash in pack.hashes() {
            let node = pack.get(hash)?;
            self.store.put(&node)?;
        }
        let checksum = to_hex(pack.checksum());
        let path = self.root.join("packs").join(format!("{checksum}.pack"));
        let temp = self.root.join("tmp").join(format!("{checksum}.pack.tmp"));
        fs::write(&temp, bytes)?;
        fs::rename(temp, path)?;
        Ok(checksum)
    }

    pub fn fetch_pack(&self, want: &str, have: Option<&str>) -> Result<Vec<u8>> {
        let want_hash = from_hex(want)?;
        let want_version = load_version(&self.store, &want_hash)?;
        let old_root = match have {
            Some(have) => {
                let have = from_hex(have)?;
                if self.store.contains(&have) {
                    Some(load_version(&self.store, &have)?.root)
                } else {
                    None
                }
            }
            None => None,
        };

        let mut nodes = collect_diff(&self.store, old_root, want_version.root)?;
        nodes.extend(version_chain(&self.store, &want_hash)?);
        let nodes = load_nodes(&self.store, &nodes)?;
        Ok(pack::write(&nodes)?.bytes)
    }

    pub fn push(&self, request: &PushRequest) -> Result<PushResult> {
        let current = self.read_ref_hex(&request.ref_name)?;
        if current != request.old {
            return Ok(PushResult::conflict(current));
        }

        let new_hash = from_hex(&request.new)?;
        if !self.store.contains(&new_hash) {
            return Ok(PushResult::missing(vec![request.new.clone()]));
        }
        let new_version = load_version(&self.store, &new_hash)?;

        let old_root = match &request.old {
            Some(old) => {
                let old = from_hex(old)?;
                if !self.store.contains(&old) {
                    return Ok(PushResult::conflict(current));
                }
                if !is_version_ancestor(&self.store, &old, &new_hash)? {
                    return Ok(PushResult::conflict(current));
                }
                Some(load_version(&self.store, &old)?.root)
            }
            None => None,
        };

        let mut nodes = collect_diff(&self.store, old_root, new_version.root)?;
        nodes.extend(version_chain(&self.store, &new_hash)?);
        let missing: Vec<String> = nodes
            .iter()
            .filter(|hash| !self.store.contains(hash))
            .map(|hash| to_hex(hash))
            .collect();
        if !missing.is_empty() {
            return Ok(PushResult::missing(missing));
        }

        space::write_ref(&self.root, &request.ref_name, &new_hash)?;
        Ok(PushResult::ok())
    }
}

/// Everything reachable from a version root. Version parents are not included:
/// a snapshot only needs its tree.
pub fn collect_all(store: &Store, root: &Hash) -> Result<BTreeSet<Hash>> {
    let mut out = BTreeSet::new();
    let mut stack = vec![*root];
    while let Some(hash) = stack.pop() {
        if !out.insert(hash) {
            continue;
        }
        match store.get(&hash)? {
            Node::Index(node) => stack.extend(node.entries.iter().map(|entry| entry.hash)),
            Node::Object(object) => {
                if !is_zero(&object.design_hash) {
                    stack.push(object.design_hash);
                }
            }
            Node::Version(version) => stack.push(version.root),
            Node::Design(_) => {}
        }
    }
    Ok(out)
}

/// Hashes reachable from `new_root` but not already present under `old_root`.
pub fn collect_diff(
    store: &Store,
    old_root: Option<Hash>,
    new_root: Hash,
) -> Result<BTreeSet<Hash>> {
    let mut out = BTreeSet::new();
    visit_diff(store, old_root, new_root, &mut out)?;
    Ok(out)
}

fn visit_diff(store: &Store, old: Option<Hash>, new: Hash, out: &mut BTreeSet<Hash>) -> Result<()> {
    if old == Some(new) {
        return Ok(());
    }
    match store.get(&new)? {
        Node::Index(index) => {
            let old_index = match old {
                Some(hash) => match store.get(&hash)? {
                    Node::Index(old) if old.leaf == index.leaf => Some(old),
                    _ => None,
                },
                None => None,
            };
            let Some(old_index) = old_index else {
                collect_all_into(store, &new, out)?;
                return Ok(());
            };
            out.insert(new);
            let old_entries: BTreeMap<&[u8], Hash> = old_index
                .entries
                .iter()
                .map(|entry| (entry.key.as_slice(), entry.hash))
                .collect();
            for entry in &index.entries {
                match old_entries.get(entry.key.as_slice()) {
                    Some(hash) if *hash == entry.hash => {}
                    other => visit_diff(store, other.copied(), entry.hash, out)?,
                }
            }
        }
        Node::Object(object) => {
            out.insert(new);
            if !is_zero(&object.design_hash) {
                out.insert(object.design_hash);
            }
        }
        Node::Design(_) => {
            out.insert(new);
        }
        Node::Version(version) => {
            out.insert(new);
            visit_diff(store, old, version.root, out)?;
        }
    }
    Ok(())
}

fn collect_all_into(store: &Store, root: &Hash, out: &mut BTreeSet<Hash>) -> Result<()> {
    for hash in collect_all(store, root)? {
        out.insert(hash);
    }
    Ok(())
}

pub fn is_version_ancestor(store: &Store, ancestor: &Hash, descendant: &Hash) -> Result<bool> {
    let mut current = *descendant;
    for _ in 0..1_000_000 {
        if &current == ancestor {
            return Ok(true);
        }
        let version = load_version(store, &current)?;
        if is_zero(&version.parent) {
            return Ok(false);
        }
        current = version.parent;
    }
    Err(Error::Config(
        "version parent chain is too deep or cyclic".into(),
    ))
}

/// Version nodes from `version` back to the root commit. Their trees are not
/// included; this is only enough for history (`ds log`).
pub fn version_chain(store: &Store, version: &Hash) -> Result<BTreeSet<Hash>> {
    let mut out = BTreeSet::new();
    let mut current = *version;
    loop {
        out.insert(current);
        let node = load_version(store, &current)?;
        if is_zero(&node.parent) {
            break;
        }
        current = node.parent;
    }
    Ok(out)
}

pub fn verify_closure(store: &Store, version: &Hash) -> Result<()> {
    let node = load_version(store, version)?;
    collect_all(store, &node.root)?;
    Ok(())
}

fn load_nodes(store: &Store, hashes: &BTreeSet<Hash>) -> Result<Vec<Node>> {
    let mut nodes = Vec::with_capacity(hashes.len());
    for hash in hashes {
        nodes.push(store.get(hash)?);
    }
    Ok(nodes)
}

// ---------------------------------------------------------------------------
// Remote-tracking refs

pub fn read_remote_ref(dir: &Path, remote: &str, branch: &str) -> Result<Option<Hash>> {
    let path = remote_ref_path(dir, remote, branch)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(from_hex(text.trim())?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

pub fn write_remote_ref(dir: &Path, remote: &str, branch: &str, version: &Hash) -> Result<()> {
    let path = remote_ref_path(dir, remote, branch)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = dir
        .join("tmp")
        .join(format!("remote-{remote}-{}", branch.replace('/', "_")));
    fs::create_dir_all(dir.join("tmp"))?;
    fs::write(&temp, format!("{}\n", to_hex(version)))?;
    fs::rename(temp, path)?;
    Ok(())
}

fn remote_ref_path(dir: &Path, remote: &str, branch: &str) -> Result<PathBuf> {
    crate::branch::validate_name(branch)?;
    if remote.is_empty()
        || !remote
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::Config(format!("invalid remote name {remote}")));
    }
    Ok(dir.join("refs").join("remotes").join(remote).join(branch))
}

// ---------------------------------------------------------------------------
// Client transport

pub trait Transport {
    fn refs(&mut self) -> Result<BTreeMap<String, String>>;
    /// Branch map plus the advertised source repository, when the remote has
    /// one. Implementations that can return both in one round trip override
    /// this; the default falls back to `refs` without a source.
    fn refs_info(&mut self) -> Result<RefsResponse> {
        Ok(RefsResponse {
            refs: self.refs()?,
            source: None,
        })
    }
    fn fetch(&mut self, want: &str, have: Option<&str>) -> Result<Vec<u8>>;
    fn send_pack(&mut self, bytes: &[u8]) -> Result<String>;
    fn push(&mut self, request: &PushRequest) -> Result<PushResult>;
}

pub fn open_transport(url: &str) -> Result<Box<dyn Transport>> {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(Box::new(HttpTransport::new(url)?));
    }
    if let Some(rest) = url.strip_prefix("ds://") {
        // A ds:// address pointing at this machine can be served directly
        // when the same user already owns the design-space root. This is what
        // lets `ds clone ds://localhost/<id>` work without an sshd.
        if let Some(local) = local_transport(rest)? {
            return Ok(Box::new(local));
        }
        return Ok(Box::new(SshTransport::start(rest)?));
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        return Ok(Box::new(SshTransport::start(rest)?));
    }
    Err(Error::Config(format!(
        "unsupported remote URL {url}; use http(s):// or ssh://"
    )))
}

/// In-process transport over a local design-space repository.
struct LocalTransport {
    repo: ServerRepo,
}

fn local_transport(rest: &str) -> Result<Option<LocalTransport>> {
    let Some(root) = local_root() else {
        return Ok(None);
    };
    local_transport_at(&root, rest)
}

fn local_transport_at(root: &Path, rest: &str) -> Result<Option<LocalTransport>> {
    let Some(id) = loopback_project_id(rest) else {
        return Ok(None);
    };
    let Ok(project) = crate::project::find_by_id(root, &id) else {
        return Ok(None);
    };
    let repo = ServerRepo::open_with_source(&project.dir, project.meta.source)?;
    Ok(Some(LocalTransport { repo }))
}

fn local_root() -> Option<PathBuf> {
    match std::env::var("DESIGN_SPACE_ROOT") {
        Ok(root) if !root.trim().is_empty() => return Some(PathBuf::from(root)),
        _ => {}
    }
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.trim().is_empty())
        .map(|home| PathBuf::from(home).join(".foundry").join("design-spaces"))
}

/// Extract the project id from a loopback `ds://` address, e.g.
/// `user@localhost:2222/<id>` or `127.0.0.1/<id>`.
fn loopback_project_id(rest: &str) -> Option<String> {
    let (authority, path) = rest.split_once('/')?;
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    if host != "localhost" && host != "127.0.0.1" {
        return None;
    }
    let id = path.trim_matches('/');
    if id.is_empty() || id.contains('/') {
        return None;
    }
    Some(id.to_owned())
}

impl Transport for LocalTransport {
    fn refs(&mut self) -> Result<BTreeMap<String, String>> {
        self.repo.refs()
    }

    fn refs_info(&mut self) -> Result<RefsResponse> {
        Ok(RefsResponse {
            refs: self.repo.refs()?,
            source: self.repo.source().map(str::to_owned),
        })
    }

    fn fetch(&mut self, want: &str, have: Option<&str>) -> Result<Vec<u8>> {
        self.repo.fetch_pack(want, have)
    }

    fn send_pack(&mut self, bytes: &[u8]) -> Result<String> {
        self.repo.receive_pack(bytes)
    }

    fn push(&mut self, request: &PushRequest) -> Result<PushResult> {
        self.repo.push(request)
    }
}

fn auth_header() -> Option<String> {
    std::env::var("DS_TOKEN")
        .ok()
        .filter(|token| !token.is_empty())
}

struct HttpTransport {
    base: String,
    agent: ureq::Agent,
    token: Option<String>,
}

impl HttpTransport {
    fn new(url: &str) -> Result<Self> {
        Ok(Self {
            base: url.trim_end_matches('/').to_owned(),
            agent: ureq::AgentBuilder::new().build(),
            token: auth_header(),
        })
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        let mut request = self.agent.request(method, &format!("{}{path}", self.base));
        if let Some(token) = &self.token {
            request = request.set("Authorization", &format!("Bearer {token}"));
        }
        request
    }

    fn status_error(code: u16, body: &str) -> Error {
        Error::Config(format!("remote error {code}: {}", body.trim()))
    }
}

impl Transport for HttpTransport {
    fn refs(&mut self) -> Result<BTreeMap<String, String>> {
        Ok(self.refs_info()?.refs)
    }

    fn refs_info(&mut self) -> Result<RefsResponse> {
        match self.request("GET", "/v1/refs").call() {
            Ok(response) => Ok(response.into_json::<RefsResponse>()?),
            Err(ureq::Error::Status(code, response)) => {
                Err(Self::status_error(code, &response.into_string()?))
            }
            Err(error) => Err(Error::Config(format!("http: {error}"))),
        }
    }

    fn fetch(&mut self, want: &str, have: Option<&str>) -> Result<Vec<u8>> {
        let request = FetchRequest {
            want: want.to_owned(),
            have: have.map(str::to_owned),
        };
        match self.request("POST", "/v1/fetch").send_json(&request) {
            Ok(response) => {
                let mut bytes = Vec::new();
                response.into_reader().read_to_end(&mut bytes)?;
                Ok(bytes)
            }
            Err(ureq::Error::Status(code, response)) => {
                Err(Self::status_error(code, &response.into_string()?))
            }
            Err(error) => Err(Error::Config(format!("http: {error}"))),
        }
    }

    fn send_pack(&mut self, bytes: &[u8]) -> Result<String> {
        #[derive(Deserialize)]
        struct Sent {
            checksum: String,
        }
        match self
            .request("POST", "/v1/packs")
            .set("Content-Type", "application/octet-stream")
            .send_bytes(bytes)
        {
            Ok(response) => Ok(response.into_json::<Sent>()?.checksum),
            Err(ureq::Error::Status(code, response)) => {
                Err(Self::status_error(code, &response.into_string()?))
            }
            Err(error) => Err(Error::Config(format!("http: {error}"))),
        }
    }

    fn push(&mut self, request: &PushRequest) -> Result<PushResult> {
        match self.request("POST", "/v1/push").send_json(request) {
            Ok(response) => Ok(response.into_json::<PushResult>()?),
            Err(ureq::Error::Status(code, response)) => {
                let body = response.into_string()?;
                match serde_json::from_str::<PushResult>(&body) {
                    Ok(result) if code == 409 || code == 422 => Ok(result),
                    _ => Err(Self::status_error(code, &body)),
                }
            }
            Err(error) => Err(Error::Config(format!("http: {error}"))),
        }
    }
}

const OP_REFS: u8 = 1;
const OP_FETCH: u8 = 2;
const OP_PACK: u8 = 3;
const OP_PUSH: u8 = 4;

struct SshTransport {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl SshTransport {
    fn start(url: &str) -> Result<Self> {
        let (authority, port, path) = parse_destination(url)?;
        let mut command = Command::new("ssh");
        if let Some(port) = port {
            command.args(["-p", &port]);
        }
        command.arg(authority);
        command.arg(build_remote_command(&path));
        command.stdin(Stdio::piped()).stdout(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|error| Error::Config(format!("failed to run ssh: {error}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Config("ssh stdin unavailable".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Config("ssh stdout unavailable".into()))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn request(&mut self, op: u8, payload: &[u8]) -> Result<Vec<u8>> {
        self.stdin.write_all(&[op])?;
        self.stdin
            .write_all(&(payload.len() as u32).to_le_bytes())?;
        self.stdin.write_all(payload)?;
        self.stdin.flush()?;

        let status = read_u8(&mut self.stdout)?;
        let length = read_u32(&mut self.stdout)? as usize;
        let mut response = vec![0u8; length];
        self.stdout.read_exact(&mut response)?;
        match status {
            0 => Ok(response),
            1 => {
                let message = String::from_utf8_lossy(&response).trim().to_owned();
                Err(Error::Config(message))
            }
            other => Err(Error::Config(format!("ssh protocol error {other}"))),
        }
    }
}

impl Drop for SshTransport {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Transport for SshTransport {
    fn refs(&mut self) -> Result<BTreeMap<String, String>> {
        Ok(self.refs_info()?.refs)
    }

    fn refs_info(&mut self) -> Result<RefsResponse> {
        let response = self.request(OP_REFS, &[])?;
        Ok(serde_json::from_slice::<RefsResponse>(&response)?)
    }

    fn fetch(&mut self, want: &str, have: Option<&str>) -> Result<Vec<u8>> {
        let request = FetchRequest {
            want: want.to_owned(),
            have: have.map(str::to_owned),
        };
        self.request(OP_FETCH, &serde_json::to_vec(&request)?)
    }

    fn send_pack(&mut self, bytes: &[u8]) -> Result<String> {
        #[derive(Deserialize)]
        struct Sent {
            checksum: String,
        }
        let response = self.request(OP_PACK, bytes)?;
        Ok(serde_json::from_slice::<Sent>(&response)?.checksum)
    }

    fn push(&mut self, request: &PushRequest) -> Result<PushResult> {
        let response = self.request(OP_PUSH, &serde_json::to_vec(request)?)?;
        Ok(serde_json::from_slice::<PushResult>(&response)?)
    }
}

/// Split `user@host[:port]/abs/path` into ssh authority, optional port and a
/// percent-decoded repository path.
fn parse_destination(url: &str) -> Result<(String, Option<String>, String)> {
    let (authority, path) = url
        .split_once('/')
        .ok_or_else(|| Error::Config("ds/ssh URL needs a repository path".into()))?;
    if path.is_empty() {
        return Err(Error::Config("ds/ssh URL needs a repository path".into()));
    }
    let (user_host, port) = split_authority(authority)?;
    Ok((user_host, port, format!("/{}", percent_decode(path)?)))
}

fn percent_decode(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(Error::Config("truncated percent escape in URL".into()));
            }
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3])
                .map_err(|error| Error::Config(error.to_string()))?;
            out.push(
                u8::from_str_radix(hex, 16)
                    .map_err(|_| Error::Config(format!("bad percent escape %{hex}")))?,
            );
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|error| Error::Config(error.to_string()))
}

fn split_authority(authority: &str) -> Result<(String, Option<String>)> {
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (host, Some(port.to_owned()))
        }
        _ => (authority, None),
    };
    if host.is_empty() || host.starts_with('-') {
        return Err(Error::Config(format!("invalid ssh authority {authority}")));
    }
    Ok((host.to_owned(), port))
}

fn build_remote_command(path: &str) -> String {
    let command =
        std::env::var("DS_REMOTE_COMMAND").unwrap_or_else(|_| "ds-server stdio".to_owned());
    let trimmed = path.trim_start_matches('/');
    let quoted = trimmed.replace('\'', "'\\''");
    if trimmed.contains('/') {
        // Legacy address carrying an absolute repository path.
        format!("{command} --repo '/{quoted}'")
    } else {
        format!("{command} --project '{quoted}'")
    }
}

// ---------------------------------------------------------------------------
// Client operations

pub fn push(
    transport: &mut dyn Transport,
    store: &Store,
    dir: &Path,
    branch: &str,
) -> Result<PushSummary> {
    let local = space::read_ref_optional(dir, branch)?
        .ok_or_else(|| Error::Config(format!("branch {branch} has no committed version")))?;
    let local_hex = to_hex(&local);

    let refs = transport.refs()?;
    let remote = refs.get(branch).cloned();
    if remote.as_deref() == Some(local_hex.as_str()) {
        return Ok(PushSummary {
            up_to_date: true,
            version: local_hex,
            nodes: 0,
        });
    }

    let old_hash = match &remote {
        Some(hex) => Some(from_hex(hex)?),
        None => None,
    };
    if let Some(old) = old_hash {
        if !store.contains(&old) {
            return Err(Error::Config(
                "remote version is not present locally; run `ds fetch` first".into(),
            ));
        }
        if !is_version_ancestor(store, &old, &local)? {
            return Err(Error::Config(format!(
                "push is not a fast-forward: {} is not an ancestor of {local_hex}",
                to_hex(&old)
            )));
        }
    }

    let old_root = match old_hash {
        Some(old) => Some(load_version(store, &old)?.root),
        None => None,
    };
    let new_version = load_version(store, &local)?;
    let mut nodes = collect_diff(store, old_root, new_version.root)?;
    nodes.extend(version_chain(store, &local)?);
    let count = nodes.len();
    send_nodes(transport, store, &nodes)?;

    let mut result = transport.push(&PushRequest {
        ref_name: branch.to_owned(),
        old: remote.clone(),
        new: local_hex.clone(),
    })?;
    if result.status == "missing" {
        let extra: BTreeSet<Hash> = result
            .missing
            .iter()
            .map(|hex| from_hex(hex))
            .collect::<Result<_>>()?;
        if !extra.is_empty() {
            send_nodes(transport, store, &extra)?;
            result = transport.push(&PushRequest {
                ref_name: branch.to_owned(),
                old: remote.clone(),
                new: local_hex.clone(),
            })?;
        }
    }
    match result.status.as_str() {
        "ok" => {
            write_remote_ref(dir, "origin", branch, &local)?;
            Ok(PushSummary {
                up_to_date: false,
                version: local_hex,
                nodes: count,
            })
        }
        "conflict" => Err(Error::Config(format!(
            "remote ref moved to {}",
            result.current.unwrap_or_else(|| "(none)".into())
        ))),
        "missing" => Err(Error::Config(format!(
            "remote still misses {} node(s)",
            result.missing.len()
        ))),
        other => Err(Error::Config(format!("unexpected push result {other}"))),
    }
}

fn send_nodes(transport: &mut dyn Transport, store: &Store, nodes: &BTreeSet<Hash>) -> Result<()> {
    let nodes = load_nodes(store, nodes)?;
    let packed = pack::write(&nodes)?;
    transport.send_pack(&packed.bytes)?;
    Ok(())
}

pub fn fetch(
    transport: &mut dyn Transport,
    store: &Store,
    dir: &Path,
    branch: &str,
    pull: bool,
) -> Result<FetchSummary> {
    let refs = transport.refs()?;
    let want = refs
        .get(branch)
        .cloned()
        .ok_or_else(|| Error::Config(format!("remote branch {branch} does not exist")))?;
    let want_hash = from_hex(&want)?;

    let local = space::read_ref_optional(dir, branch)?;
    if local == Some(want_hash) {
        write_remote_ref(dir, "origin", branch, &want_hash)?;
        return Ok(FetchSummary {
            up_to_date: true,
            version: want,
            nodes: 0,
        });
    }
    let have = local.map(|hash| to_hex(&hash));
    let bytes = transport.fetch(&want, have.as_deref())?;
    let pack = Pack::from_bytes(bytes)?;
    let count = pack.len();
    for hash in pack.hashes() {
        let node = pack.get(hash)?;
        store.put(&node)?;
    }
    verify_closure(store, &want_hash)?;
    write_remote_ref(dir, "origin", branch, &want_hash)?;

    if pull {
        if let Some(local) = local
            && !is_version_ancestor(store, &local, &want_hash)?
        {
            return Err(Error::Config(format!(
                "pull is not a fast-forward: remote {want} does not descend from {}",
                to_hex(&local)
            )));
        }
        space::write_ref(dir, branch, &want_hash)?;
    }

    Ok(FetchSummary {
        up_to_date: false,
        version: want,
        nodes: count,
    })
}

// ---------------------------------------------------------------------------
// HTTP server

#[derive(Clone)]
struct AppState {
    server: Arc<ServerRepo>,
    token: Option<String>,
}

type ApiResult = std::result::Result<axum::response::Response, (axum::http::StatusCode, String)>;

fn check_auth(headers: &axum::http::HeaderMap, token: &Option<String>) -> bool {
    let Some(expected) = token else {
        return true;
    };
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected)
}

fn unauthorized() -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::UNAUTHORIZED, "unauthorized".into())
}

fn bad(error: impl std::fmt::Display) -> (axum::http::StatusCode, String) {
    (axum::http::StatusCode::BAD_REQUEST, error.to_string())
}

async fn http_refs(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: axum::http::HeaderMap,
) -> ApiResult {
    use axum::response::IntoResponse;
    if !check_auth(&headers, &state.token) {
        return Err(unauthorized());
    }
    let refs = state.server.refs().map_err(bad)?;
    let source = state.server.source().map(str::to_owned);
    Ok(axum::Json(RefsResponse { refs, source }).into_response())
}

async fn http_fetch(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<FetchRequest>,
) -> ApiResult {
    use axum::response::IntoResponse;
    if !check_auth(&headers, &state.token) {
        return Err(unauthorized());
    }
    let bytes = state
        .server
        .fetch_pack(&request.want, request.have.as_deref())
        .map_err(bad)?;
    Ok((
        axum::http::StatusCode::OK,
        [("content-type", "application/octet-stream")],
        bytes,
    )
        .into_response())
}

async fn http_pack(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult {
    use axum::response::IntoResponse;
    if !check_auth(&headers, &state.token) {
        return Err(unauthorized());
    }
    let checksum = state.server.receive_pack(&body).map_err(bad)?;
    Ok(axum::Json(serde_json::json!({ "checksum": checksum })).into_response())
}

async fn http_push(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: axum::http::HeaderMap,
    axum::Json(request): axum::Json<PushRequest>,
) -> ApiResult {
    use axum::response::IntoResponse;
    if !check_auth(&headers, &state.token) {
        return Err(unauthorized());
    }
    let result = state.server.push(&request).map_err(bad)?;
    let status = match result.status.as_str() {
        "ok" => axum::http::StatusCode::OK,
        "conflict" => axum::http::StatusCode::CONFLICT,
        _ => axum::http::StatusCode::UNPROCESSABLE_ENTITY,
    };
    Ok((status, axum::Json(result)).into_response())
}

pub fn serve_http(repo: &Path, listen: &str) -> Result<()> {
    use axum::Router;
    use axum::extract::DefaultBodyLimit;
    use axum::routing::{get, post};

    let state = AppState {
        server: Arc::new(ServerRepo::open(repo)?),
        token: auth_header(),
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let app = Router::new()
            .route("/v1/refs", get(http_refs))
            .route("/v1/fetch", post(http_fetch))
            .route("/v1/packs", post(http_pack))
            .route("/v1/push", post(http_push))
            .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .map_err(Error::Io)?;
        axum::serve(listener, app).await.map_err(Error::Io)
    })
}

// ---------------------------------------------------------------------------
// SSH / stdio server

/// Serve one project by its `ds://` id. The URL never carries a filesystem
/// path; the server resolves it under its root.
pub fn serve_project_stdio(root: &Path, project_id: &str) -> Result<()> {
    let project = crate::project::find_by_id(root, project_id)?;
    serve_stdio_with_source(&project.dir, project.meta.source)
}

pub fn serve_stdio(repo: &Path) -> Result<()> {
    serve_stdio_with_source(repo, None)
}

pub fn serve_stdio_with_source(repo: &Path, source: Option<String>) -> Result<()> {
    let server = ServerRepo::open_with_source(repo, source)?;
    let stdin = std::io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut output = std::io::stdout();
    while let Some(op) = read_u8_opt(&mut input)? {
        let length = read_u32(&mut input)? as usize;
        let mut payload = vec![0u8; length];
        input.read_exact(&mut payload)?;

        match dispatch(&server, op, &payload) {
            Ok(response) => {
                output.write_all(&[0])?;
                output.write_all(&(response.len() as u32).to_le_bytes())?;
                output.write_all(&response)?;
            }
            Err(error) => {
                let message = error.to_string();
                output.write_all(&[1])?;
                output.write_all(&(message.len() as u32).to_le_bytes())?;
                output.write_all(message.as_bytes())?;
            }
        }
        output.flush()?;
    }
    Ok(())
}

fn dispatch(server: &ServerRepo, op: u8, payload: &[u8]) -> Result<Vec<u8>> {
    match op {
        OP_REFS => Ok(serde_json::to_vec(&RefsResponse {
            refs: server.refs()?,
            source: server.source().map(str::to_owned),
        })?),
        OP_FETCH => {
            let request: FetchRequest = serde_json::from_slice(payload)?;
            server.fetch_pack(&request.want, request.have.as_deref())
        }
        OP_PACK => {
            #[derive(Serialize)]
            struct Sent {
                checksum: String,
            }
            Ok(serde_json::to_vec(&Sent {
                checksum: server.receive_pack(payload)?,
            })?)
        }
        OP_PUSH => {
            let request: PushRequest = serde_json::from_slice(payload)?;
            Ok(serde_json::to_vec(&server.push(&request)?)?)
        }
        other => Err(Error::Config(format!("unknown opcode {other}"))),
    }
}

fn read_u8<R: Read>(reader: &mut R) -> Result<u8> {
    let mut byte = [0u8; 1];
    reader.read_exact(&mut byte)?;
    Ok(byte[0])
}

fn read_u8_opt<R: Read>(reader: &mut R) -> Result<Option<u8>> {
    let mut byte = [0u8; 1];
    match reader.read_exact(&mut byte) {
        Ok(()) => Ok(Some(byte[0])),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32> {
    let mut bytes = [0u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

/// Kept for tests that need a framed client without spawning ssh.
pub fn frame(op: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + payload.len());
    out.push(op);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;

    fn element(name: &str, text: &str) -> Element {
        Element {
            name: name.into(),
            ty: crate::types::CPP_CLASS,
            path: "a.cpp".into(),
            commit: "00".repeat(20),
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
            end_line: 1,
            own_text: text.into(),
            parent: None,
            children: vec![],
            design: Some(format!("design {text}")),
            content_hash: None,
        }
    }

    fn commit_elements(dir: &Path, _base: Option<Hash>, elements: Vec<Element>) -> (Store, Hash) {
        let store = Store::new(dir);
        let options = space::CommitOptions {
            message: "test".into(),
            author: "test".into(),
            source_commit: [0u8; 32],
            pipeline: "test".into(),
            ref_name: "main".into(),
        };
        let result = space::commit(&store, dir, &elements, &options).unwrap();
        (store, result.version)
    }

    fn temp_repo() -> (tempfile::TempDir, ServerRepo) {
        let temp = tempfile::tempdir().unwrap();
        let server = ServerRepo::open(&temp.path().join("server")).unwrap();
        (temp, server)
    }

    #[test]
    fn parses_ssh_destinations() {
        assert_eq!(
            parse_destination("ds@host:2222/srv/ds/demo").unwrap(),
            ("ds@host".into(), Some("2222".into()), "/srv/ds/demo".into())
        );
        assert_eq!(
            parse_destination("host/a%20b").unwrap(),
            ("host".into(), None, "/a b".into())
        );
        assert!(parse_destination("host").is_err());
    }

    #[test]
    fn remote_command_passes_project_id_not_a_path() {
        // A single-segment path is a project id resolved under the server root.
        assert!(
            build_remote_command("/01a1db53c0ffee42").ends_with("--project '01a1db53c0ffee42'")
        );
        // Multi-segment paths remain supported as legacy absolute repos.
        assert!(
            build_remote_command("/srv/design-spaces/demo")
                .ends_with("--repo '/srv/design-spaces/demo'")
        );
    }

    #[test]
    fn loopback_addresses_use_the_local_repository() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        let project = crate::project::create(
            &root,
            "Demo",
            Some("git@example.com:demo.git"),
            "localhost",
            Some("u"),
            None,
        )
        .unwrap();
        let id = &project.meta.id;
        let mut transport = local_transport_at(&root, &format!("u@localhost/{id}"))
            .unwrap()
            .unwrap();
        let info = transport.refs_info().unwrap();
        assert_eq!(info.source.as_deref(), Some("git@example.com:demo.git"));
        assert!(info.refs.is_empty());
        assert!(
            local_transport_at(&root, &format!("u@remotehost/{id}"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn refs_advertise_the_recorded_source() {
        let temp = tempfile::tempdir().unwrap();
        let repo = ServerRepo::open_with_source(
            &temp.path().join("repo"),
            Some("git@example.com:demo.git".into()),
        )
        .unwrap();
        let bytes = dispatch(&repo, OP_REFS, &[]).unwrap();
        let response: RefsResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(response.source.as_deref(), Some("git@example.com:demo.git"));
    }

    #[test]
    fn diff_only_collects_changed_nodes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("client");
        fs::create_dir_all(&dir).unwrap();
        let (store, first) = commit_elements(
            &dir,
            None,
            vec![element("acme::Foo", "v1"), element("acme::Bar", "same")],
        );
        let first_version = load_version(&store, &first).unwrap();
        let view = space::view_root(&store, &first_version, b"namespaces")
            .unwrap()
            .unwrap();
        let bar = space::find_in_view(&store, &view, b"acme::Bar")
            .unwrap()
            .unwrap();

        let (store, second) = commit_elements(
            &dir,
            Some(first),
            vec![element("acme::Foo", "v2"), element("acme::Bar", "same")],
        );
        let second_version = load_version(&store, &second).unwrap();

        let diff = collect_diff(&store, Some(first_version.root), second_version.root).unwrap();
        let all = collect_all(&store, &second_version.root).unwrap();
        assert!(!diff.is_empty());
        assert!(!diff.contains(&bar));
        assert!(diff.iter().all(|hash| all.contains(hash)));
        assert!(!diff.contains(&first_version.root));
    }

    #[test]
    fn server_push_and_fetch_round_trip() {
        let (temp, server) = temp_repo();
        let dir = temp.path().join("client");
        fs::create_dir_all(&dir).unwrap();
        let (store, version) = commit_elements(&dir, None, vec![element("acme::Foo", "v1")]);
        let version_node = load_version(&store, &version).unwrap();
        let mut nodes = collect_all(&store, &version_node.root).unwrap();
        nodes.insert(version);
        let pack = pack::write(&load_nodes(&store, &nodes).unwrap()).unwrap();
        server.receive_pack(&pack.bytes).unwrap();

        let mut request = PushRequest {
            ref_name: "main".into(),
            old: None,
            new: to_hex(&version),
        };
        assert_eq!(server.push(&request).unwrap().status, "ok");
        let bytes = server.fetch_pack(&to_hex(&version), None).unwrap();
        assert!(!bytes.is_empty());

        // Transfer into a fresh client store.
        let fresh = temp.path().join("fresh");
        fs::create_dir_all(&fresh).unwrap();
        let fresh_store = Store::new(&fresh);
        let received = Pack::from_bytes(bytes).unwrap();
        for hash in received.hashes() {
            fresh_store.put(&received.get(hash).unwrap()).unwrap();
        }
        verify_closure(&fresh_store, &version).unwrap();

        // A conflicting push must be rejected.
        request.old = Some("00".repeat(32));
        assert_eq!(server.push(&request).unwrap().status, "conflict");
    }
}
