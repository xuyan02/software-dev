//! Remote storage service.
//!
//! The service exposes a repository collection over HTTP or a single
//! repository over the length-prefixed stdio protocol used by SSH. Each
//! repository is a directory with the same object/doc/symbol/commit/pack
//! layout and `refs/heads/*`.
//!
//! HTTP operations:
//!
//! - list repositories;
//! - list refs;
//! - list immutable files;
//! - read one immutable file;
//! - write one immutable file;
//! - update refs.
//!
//! Uploads are verified against their content hash before they are published.
//! HTTP can require a shared bearer token; SSH relies on SSH authentication.

use crate::catalog::Catalog;
use crate::model::{
    Commit, CommitHash, DesignObject, DesignStructurePayload, DirectoryPayload,
    DocumentDirectoryPayload, DocumentPayload, GitOid, Hash32, Kind, NamespacePayload,
    ObjectPayload, ParentRef, SymbolHash,
};
use crate::remote::{
    FILE_ROOTS, MAX_TRANSFER, collect_files, parse_hash, parse_refs, publish, read_token_file,
    valid_branch, valid_relative, write_ref,
};
use anyhow::{Context, Result, bail};
use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Extension, Path as AxumPath, Request, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PATH: usize = 4096;

#[derive(Debug, Clone)]
pub struct RepoCollection {
    root: PathBuf,
    /// Token given to task agents. It may write temporary branches and files,
    /// but it may not move a protected branch.
    agent_token_hash: Option<[u8; 32]>,
    /// Control-plane token. It may move any branch and create repositories.
    admin_token_hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRole {
    Agent,
    Admin,
}

impl AuthRole {
    pub fn is_admin(self) -> bool {
        self == Self::Admin
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedRepo {
    pub name: String,
    pub branch: String,
    pub commit: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateRepoError {
    #[error("invalid repository name: {0}")]
    InvalidName(String),
    #[error("invalid branch: {0}")]
    InvalidBranch(String),
    #[error("repository already exists: {0}")]
    Exists(String),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl RepoCollection {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            root,
            agent_token_hash: None,
            admin_token_hash: None,
        })
    }

    /// Create a repository with an empty initial View on `branch`.
    ///
    /// This is the repository-creation interface used by Foundry. The server
    /// does not create a repository implicitly on push.
    pub fn create(
        &self,
        name: &str,
        branch: &str,
    ) -> std::result::Result<CreatedRepo, CreateRepoError> {
        if !valid_repo_name(name) {
            return Err(CreateRepoError::InvalidName(name.to_owned()));
        }
        if !valid_branch(branch) {
            return Err(CreateRepoError::InvalidBranch(branch.to_owned()));
        }
        let path = self.root.join(name);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(CreateRepoError::Exists(name.to_owned()));
            }
            Err(error) => return Err(CreateRepoError::Other(error.into())),
        }
        match init_repo(&path, branch) {
            Ok(commit) => Ok(CreatedRepo {
                name: name.to_owned(),
                branch: branch.to_owned(),
                commit: commit.to_hex(),
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(&path);
                Err(CreateRepoError::Other(error))
            }
        }
    }

    pub fn with_token_file(mut self, path: &Path) -> Result<Self> {
        let token = read_token_file(path)?;
        self.agent_token_hash = Some(hash_token(&token));
        Ok(self)
    }

    pub fn with_admin_token_file(mut self, path: &Path) -> Result<Self> {
        let token = read_token_file(path)?;
        self.admin_token_hash = Some(hash_token(&token));
        Ok(self)
    }

    pub fn auth_role(&self, headers: &HeaderMap) -> Option<AuthRole> {
        if self.agent_token_hash.is_none() && self.admin_token_hash.is_none() {
            return Some(AuthRole::Admin);
        }
        let token = bearer_token(headers)?;
        let token = hash_token(&token);
        if self
            .admin_token_hash
            .is_some_and(|expected| constant_time_eq(&expected, &token))
        {
            return Some(AuthRole::Admin);
        }
        if self
            .agent_token_hash
            .is_some_and(|expected| constant_time_eq(&expected, &token))
        {
            return Some(AuthRole::Agent);
        }
        None
    }

    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        self.auth_role(headers).is_some()
    }

    pub fn repos(&self) -> Result<Vec<String>> {
        let mut names = Vec::new();
        if !self.root.is_dir() {
            return Ok(names);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_repo_name(&name) {
                continue;
            }
            if entry.path().join("objects").is_dir() || entry.path().join("refs").is_dir() {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn server(&self, name: &str) -> Result<Option<RemoteServer>> {
        if !valid_repo_name(name) {
            bail!("invalid repository name: {name}");
        }
        let root = self.root.join(name);
        if !root.is_dir() {
            return Ok(None);
        }
        Ok(Some(RemoteServer::open(root)?))
    }
}

fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn init_repo(root: &Path, branch: &str) -> Result<CommitHash> {
    let store = crate::store::Store::open(root)?;
    let files = store.put_symbol("files")?;
    let namespaces = store.put_symbol("namespaces")?;
    let documents = store.put_symbol("documents")?;
    let mut catalog = Catalog::new(GitOid::ZERO);
    catalog.insert(
        files,
        DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: files,
                parent: ParentRef::View,
                source: None,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    catalog.insert(
        namespaces,
        DesignObject::new(
            Kind::Namespace,
            ObjectPayload::Namespace(NamespacePayload {
                qualified_name: namespaces,
                parent: ParentRef::View,
                source: None,
                content_hash: Hash32::ZERO,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    catalog.insert(
        documents,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: documents,
                parent: ParentRef::View,
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;

    // Preset design documents and the logical structure root. Creating them
    // with the repository means design workflows never start from a missing
    // technical design.
    let design_dir = store.put_symbol("documents/design")?;
    catalog.insert(
        design_dir,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: design_dir,
                parent: ParentRef::Object(documents),
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;
    for (qname, title) in [
        ("documents/design/technical", "技术设计"),
        ("documents/design/directory", "目录设计"),
        ("documents/design/namespace", "命名空间设计"),
    ] {
        let document = store.put_symbol(qname)?;
        catalog.insert(
            document,
            DesignObject::new(
                Kind::Document,
                ObjectPayload::Document(DocumentPayload {
                    qualified_name: document,
                    parent: design_dir,
                    title: title.to_owned(),
                    sections: vec![],
                    children: vec![],
                    comments: vec![],
                }),
            ),
        )?;
    }
    let proposals = store.put_symbol("documents/design/proposals")?;
    catalog.insert(
        proposals,
        DesignObject::new(
            Kind::DocumentDirectory,
            ObjectPayload::DocumentDirectory(DocumentDirectoryPayload {
                qualified_name: proposals,
                parent: ParentRef::Object(design_dir),
                children: vec![],
                comments: vec![],
            }),
        ),
    )?;
    let design = store.put_symbol("design")?;
    catalog.insert(
        design,
        DesignObject::new(
            Kind::DesignStructure,
            ObjectPayload::DesignStructure(DesignStructurePayload {
                qualified_name: design,
                parent: ParentRef::View,
                title: "设计结构".to_owned(),
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        ),
    )?;
    let view = catalog.rebuild(&store)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let commit = Commit {
        view,
        parent: CommitHash::ZERO,
        message: "initialize".to_owned(),
        author: "ds-server".to_owned(),
        timestamp,
    };
    let hash = store.put_commit(&commit)?;
    write_ref(root, branch, hash)?;
    Ok(hash)
}

#[derive(Debug, Clone)]
pub struct RemoteServer {
    root: PathBuf,
    token_hash: Option<[u8; 32]>,
}

impl RemoteServer {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for dir in [
            "objects",
            "commits",
            "docs",
            "symbols",
            "packs",
            "refs/heads",
            "tmp",
        ] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self {
            root,
            token_hash: None,
        })
    }

    /// Require `Authorization: Bearer <token>` for every HTTP request.
    pub fn with_token_file(mut self, path: &Path) -> Result<Self> {
        let token = read_token_file(path)?;
        self.token_hash = Some(hash_token(&token));
        Ok(self)
    }

    pub fn authorized(&self, headers: &HeaderMap) -> bool {
        authorized(headers, self.token_hash)
    }

    pub fn refs(&self) -> Result<String> {
        let dir = self.root.join("refs/heads");
        let mut files = Vec::new();
        collect_files(&dir, &mut files)?;
        files.sort();
        let mut out = String::new();
        for path in files {
            let branch = path
                .strip_prefix(&dir)?
                .to_string_lossy()
                .replace('\\', "/");
            if !valid_branch(&branch) {
                bail!("invalid branch stored on remote: {branch}");
            }
            let text = fs::read_to_string(&path)?;
            let hash = parse_hash(text.trim())
                .with_context(|| format!("invalid ref stored on remote: {branch}"))?;
            if hash.is_zero() {
                bail!("zero ref stored on remote: {branch}");
            }
            out.push_str(&branch);
            out.push('\t');
            out.push_str(&hash.to_hex());
            out.push('\n');
        }
        Ok(out)
    }

    pub fn files(&self) -> Result<String> {
        let mut files = Vec::new();
        for dir in FILE_ROOTS {
            collect_files(&self.root.join(dir), &mut files)?;
        }
        let mut relative = Vec::with_capacity(files.len());
        for path in files {
            let relative_path = path
                .strip_prefix(&self.root)?
                .to_string_lossy()
                .replace('\\', "/");
            if !valid_relative(Path::new(&relative_path)) {
                bail!("invalid file stored on remote: {relative_path}");
            }
            relative.push(relative_path);
        }
        relative.sort();
        let mut out = String::new();
        for relative in relative {
            out.push_str(&relative);
            out.push('\n');
        }
        Ok(out)
    }

    pub fn read(&self, relative: &str) -> Result<Option<Vec<u8>>> {
        let path = self.resolve(relative)?;
        if !path.is_file() {
            return Ok(None);
        }
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_TRANSFER as u64 {
            bail!("remote file is too large: {relative}");
        }
        Ok(Some(fs::read(path)?))
    }

    pub fn write(&self, relative: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() > MAX_TRANSFER {
            bail!("upload is too large: {relative}");
        }
        let target = self.resolve(relative)?;
        if target.is_file() {
            if fs::read(&target)? != bytes {
                bail!("remote content mismatch at {relative}");
            }
            return Ok(());
        }
        publish(&self.root, relative, bytes)
    }

    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        if !valid_branch(branch) {
            bail!("invalid branch name: {branch}");
        }
        let path = self.root.join("refs/heads").join(branch);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn write_refs(&self, body: &[u8]) -> Result<()> {
        let body = std::str::from_utf8(body).context("ref update is not UTF-8")?;
        let refs = parse_refs(body)?;
        let store = crate::store::Store::open(&self.root)?;
        for (branch, commit) in &refs {
            store
                .get_commit(*commit)
                .with_context(|| format!("ref {branch} points at a missing commit"))?;
        }
        for (branch, commit) in &refs {
            write_ref(&self.root, branch, *commit)?;
        }
        Ok(())
    }

    fn resolve(&self, relative: &str) -> Result<PathBuf> {
        if relative.contains('\0') || !valid_relative(Path::new(relative)) {
            bail!("invalid relative path: {relative}");
        }
        Ok(self.root.join(relative))
    }
}

/// Serve the remote over HTTP.
/// Serve the repository collection over HTTP.
pub fn serve_http(collection: RepoCollection, listen: SocketAddr) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("create async runtime failed")?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(listen)
            .await
            .with_context(|| format!("bind {listen} failed"))?;
        let address = listener.local_addr()?;
        eprintln!("ds-server listening on http://{address}");
        axum::serve(listener, app(Arc::new(collection)))
            .await
            .context("HTTP server failed")?;
        Ok(())
    })
}

fn app(collection: Arc<RepoCollection>) -> Router {
    Router::new()
        .route("/repos", get(list_repos).post(create_repo))
        .route("/{repo}/refs", get(get_refs).put(put_refs))
        .route("/{repo}/refs/{*branch}", axum::routing::delete(delete_ref))
        .route("/{repo}/files", get(list_files))
        .route("/{repo}/files/{*path}", get(get_file).put(put_file))
        .layer(DefaultBodyLimit::max(MAX_TRANSFER))
        .layer(middleware::from_fn_with_state(
            collection.clone(),
            require_auth,
        ))
        .with_state(collection)
}

async fn require_auth(
    State(collection): State<Arc<RepoCollection>>,
    mut request: Request,
    next: Next,
) -> Response {
    if let Some(role) = collection.auth_role(request.headers()) {
        request.extensions_mut().insert(role);
        return next.run(request).await;
    }
    let mut response = (StatusCode::UNAUTHORIZED, "unauthorized\n").into_response();
    response.headers_mut().insert(
        WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"ds\""),
    );
    response
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    Some(token.trim().to_owned())
}

fn protected_branch(branch: &str) -> bool {
    matches!(branch, "source" | "main" | "master")
}

fn authorized(headers: &HeaderMap, expected: Option<[u8; 32]>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(token) = bearer_token(headers) else {
        return false;
    };
    constant_time_eq(&expected, &hash_token(&token))
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

type ApiError = (StatusCode, String);
type ApiResult = std::result::Result<Response, ApiError>;

async fn list_repos(State(collection): State<Arc<RepoCollection>>) -> ApiResult {
    let mut out = String::new();
    for repo in collection.repos().map_err(bad_request)? {
        out.push_str(&repo);
        out.push('\n');
    }
    Ok(out.into_response())
}

#[derive(Debug, Deserialize)]
struct CreateRepoRequest {
    name: String,
    #[serde(default)]
    branch: Option<String>,
}

async fn create_repo(
    State(collection): State<Arc<RepoCollection>>,
    Extension(role): Extension<AuthRole>,
    Json(body): Json<CreateRepoRequest>,
) -> ApiResult {
    if !role.is_admin() {
        return Ok((StatusCode::FORBIDDEN, "admin token required\n").into_response());
    }
    let branch = body.branch.as_deref().unwrap_or("main");
    let created = collection
        .create(&body.name, branch)
        .map_err(|error| match error {
            CreateRepoError::Exists(name) => (
                StatusCode::CONFLICT,
                format!("repository already exists: {name}"),
            ),
            CreateRepoError::InvalidName(name) => (
                StatusCode::BAD_REQUEST,
                format!("invalid repository name: {name}"),
            ),
            CreateRepoError::InvalidBranch(branch) => {
                (StatusCode::BAD_REQUEST, format!("invalid branch: {branch}"))
            }
            CreateRepoError::Other(error) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{error:#}"))
            }
        })?;
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

fn open_repo(
    collection: &RepoCollection,
    repo: &str,
) -> std::result::Result<RemoteServer, ApiError> {
    collection
        .server(repo)
        .map_err(bad_request)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("repository not found: {repo}"),
            )
        })
}

async fn get_refs(
    State(collection): State<Arc<RepoCollection>>,
    AxumPath(repo): AxumPath<String>,
) -> ApiResult {
    let server = open_repo(&collection, &repo)?;
    Ok(server.refs().map_err(bad_request)?.into_response())
}

async fn put_refs(
    State(collection): State<Arc<RepoCollection>>,
    Extension(role): Extension<AuthRole>,
    AxumPath(repo): AxumPath<String>,
    body: Bytes,
) -> ApiResult {
    if role == AuthRole::Agent {
        let text = std::str::from_utf8(&body)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
        let refs =
            parse_refs(text).map_err(|error| (StatusCode::BAD_REQUEST, format!("{error:#}")))?;
        if refs.iter().any(|(branch, _)| protected_branch(branch)) {
            return Ok((
                StatusCode::FORBIDDEN,
                "agent token may not move a protected branch\n",
            )
                .into_response());
        }
    }
    let server = open_repo(&collection, &repo)?;
    server.write_refs(&body).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn delete_ref(
    State(collection): State<Arc<RepoCollection>>,
    Extension(role): Extension<AuthRole>,
    AxumPath((repo, branch)): AxumPath<(String, String)>,
) -> ApiResult {
    if role == AuthRole::Agent && protected_branch(&branch) {
        return Ok((
            StatusCode::FORBIDDEN,
            "agent token may not delete a protected branch\n",
        )
            .into_response());
    }
    let server = open_repo(&collection, &repo)?;
    server.delete_branch(&branch).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn list_files(
    State(collection): State<Arc<RepoCollection>>,
    AxumPath(repo): AxumPath<String>,
) -> ApiResult {
    let server = open_repo(&collection, &repo)?;
    Ok(server.files().map_err(bad_request)?.into_response())
}

async fn get_file(
    State(collection): State<Arc<RepoCollection>>,
    AxumPath((repo, path)): AxumPath<(String, String)>,
) -> ApiResult {
    let server = open_repo(&collection, &repo)?;
    match server.read(&path).map_err(bad_request)? {
        Some(bytes) => Ok(bytes.into_response()),
        None => Err((StatusCode::NOT_FOUND, "file not found".to_owned())),
    }
}

async fn put_file(
    State(collection): State<Arc<RepoCollection>>,
    AxumPath((repo, path)): AxumPath<(String, String)>,
    body: Bytes,
) -> ApiResult {
    let server = open_repo(&collection, &repo)?;
    server.write(&path, &body).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn bad_request(error: anyhow::Error) -> ApiError {
    (StatusCode::BAD_REQUEST, format!("{error:#}"))
}

/// Serve the remote over a length-prefixed stdin/stdout protocol.
pub fn serve_stdio(server: &RemoteServer) -> Result<()> {
    let stdin = io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        match handle(server, &mut input, line) {
            Ok(bytes) => {
                writeln!(output, "OK {}", bytes.len())?;
                output.write_all(&bytes)?;
            }
            Err(error) => {
                let message = format!("{error:#}");
                writeln!(output, "ERR {}", message.len())?;
                output.write_all(message.as_bytes())?;
            }
        }
        output.flush()?;
    }
}

fn handle<R: BufRead>(server: &RemoteServer, input: &mut R, line: &str) -> Result<Vec<u8>> {
    let mut parts = line.split(' ');
    let operation = parts.next().unwrap_or_default();
    match operation {
        "REFS" => {
            no_more(parts)?;
            Ok(server.refs()?.into_bytes())
        }
        "FILES" => {
            no_more(parts)?;
            Ok(server.files()?.into_bytes())
        }
        "GET" => {
            let length = length(parts.next(), MAX_PATH)?;
            no_more(parts)?;
            let relative = read_text(input, length)?;
            server
                .read(&relative)?
                .with_context(|| format!("file not found: {relative}"))
        }
        "PUT" => {
            let path_length = length(parts.next(), MAX_PATH)?;
            let data_length = length(parts.next(), MAX_TRANSFER)?;
            no_more(parts)?;
            let relative = read_text(input, path_length)?;
            let bytes = read_bytes(input, data_length)?;
            server.write(&relative, &bytes)?;
            Ok(Vec::new())
        }
        "PUTREF" => {
            let length = length(parts.next(), MAX_TRANSFER)?;
            no_more(parts)?;
            let body = read_bytes(input, length)?;
            server.write_refs(&body)?;
            Ok(Vec::new())
        }
        _ => bail!("unknown request: {operation}"),
    }
}

fn length(text: Option<&str>, max: usize) -> Result<usize> {
    let text = text.context("missing length")?;
    let value: usize = text
        .parse()
        .with_context(|| format!("invalid length: {text}"))?;
    if value > max {
        bail!("length {value} exceeds limit {max}");
    }
    Ok(value)
}

fn no_more<'a>(mut parts: impl Iterator<Item = &'a str>) -> Result<()> {
    if parts.next().is_some() {
        bail!("trailing request fields");
    }
    Ok(())
}

fn read_text(input: &mut impl Read, length: usize) -> Result<String> {
    let bytes = read_bytes(input, length)?;
    String::from_utf8(bytes).context("path is not UTF-8")
}

fn read_bytes(input: &mut impl Read, length: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0u8; length];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{self, commit_hash, encode_commit, encode_object, object_hash};
    use crate::model::{
        Commit, CommitHash, DesignObject, DirectoryPayload, Kind, ObjectHash, ObjectPayload,
        ParentRef,
    };
    use std::io::Cursor;
    use tempfile::tempdir;

    fn object_record(name: &str) -> (ObjectHash, Vec<u8>) {
        let object = DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: format::symbol_hash(name),
                parent: ParentRef::View,
                source: None,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        );
        let hash = object_hash(&object).unwrap();
        let bytes = encode_object(&object).unwrap();
        (hash, bytes)
    }

    fn commit_record(view: ObjectHash) -> (CommitHash, Vec<u8>) {
        let commit = Commit {
            view,
            parent: CommitHash::ZERO,
            message: "test".into(),
            author: "agent".into(),
            timestamp: 1,
        };
        (commit_hash(&commit), encode_commit(&commit))
    }

    fn record_path(kind: &str, hash: crate::model::Hash32) -> String {
        let hex = hash.to_hex();
        format!("{kind}/{}/{}/{}", &hex[..2], &hex[2..4], hex)
    }

    #[test]
    fn publishes_lists_and_reads_verified_files() {
        let dir = tempdir().unwrap();
        let server = RemoteServer::open(dir.path().join("remote")).unwrap();
        let (object_hash, object_bytes) = object_record("files");
        let (commit_hash, commit_bytes) = commit_record(object_hash);
        let object_path = record_path("objects", object_hash.0);
        let commit_path = record_path("commits", commit_hash.0);

        server.write(&object_path, &object_bytes).unwrap();
        server.write(&commit_path, &commit_bytes).unwrap();
        server
            .write_refs(format!("main\t{}\n", commit_hash.to_hex()).as_bytes())
            .unwrap();

        assert_eq!(
            server.files().unwrap(),
            format!("{commit_path}\n{object_path}\n")
        );
        assert_eq!(
            server.refs().unwrap(),
            format!("main\t{}\n", commit_hash.to_hex())
        );
        assert_eq!(server.read(&object_path).unwrap().unwrap(), object_bytes);
        assert!(
            server
                .read(&record_path("objects", crate::model::Hash32([8; 32])))
                .unwrap()
                .is_none()
        );

        server.write(&object_path, &object_bytes).unwrap();
    }

    #[test]
    fn rejects_tampered_and_escaping_uploads() {
        let dir = tempdir().unwrap();
        let server = RemoteServer::open(dir.path().join("remote")).unwrap();
        let (object_hash, object_bytes) = object_record("files");
        let wrong = crate::model::Hash32([7; 32]);
        assert!(
            server
                .write(&record_path("objects", wrong), &object_bytes)
                .is_err()
        );
        assert!(server.write("../escape", b"x").is_err());
        assert!(!dir.path().join("escape").exists());
        assert!(
            server
                .write(
                    &format!("objects/aa/bb/{}", object_hash.to_hex()),
                    &object_bytes
                )
                .is_err()
        );
    }

    #[test]
    fn refuses_refs_to_missing_commits() {
        let dir = tempdir().unwrap();
        let server = RemoteServer::open(dir.path().join("remote")).unwrap();
        let missing = CommitHash::from_bytes([9; 32]);
        assert!(
            server
                .write_refs(format!("main\t{}\n", missing.to_hex()).as_bytes())
                .is_err()
        );
        assert!(server.refs().unwrap().is_empty());
    }

    #[test]
    fn collection_lists_and_opens_repositories() {
        let dir = tempdir().unwrap();
        let collection = RepoCollection::open(dir.path().join("repos")).unwrap();
        RemoteServer::open(dir.path().join("repos/alpha")).unwrap();
        RemoteServer::open(dir.path().join("repos/beta")).unwrap();
        assert_eq!(collection.repos().unwrap(), vec!["alpha", "beta"]);
        assert!(collection.server("alpha").unwrap().is_some());
        assert!(collection.server("missing").unwrap().is_none());
        assert!(collection.server("../escape").is_err());
        assert!(collection.server(".hidden").is_err());
    }

    #[test]
    fn creates_repositories_with_an_initial_branch() {
        let dir = tempdir().unwrap();
        let collection = RepoCollection::open(dir.path().join("repos")).unwrap();
        let created = collection.create("alpha", "main").unwrap();
        assert_eq!(created.name, "alpha");
        assert_eq!(created.branch, "main");
        assert_eq!(created.commit.len(), 64);
        assert_eq!(collection.repos().unwrap(), vec!["alpha"]);
        let server = collection.server("alpha").unwrap().unwrap();
        assert_eq!(
            server.refs().unwrap(),
            format!("main\t{}\n", created.commit)
        );
        assert!(matches!(
            collection.create("alpha", "main"),
            Err(CreateRepoError::Exists(_))
        ));
        assert!(matches!(
            collection.create("../bad", "main"),
            Err(CreateRepoError::InvalidName(_))
        ));
        assert!(matches!(
            collection.create("beta", "bad branch"),
            Err(CreateRepoError::InvalidBranch(_))
        ));
    }

    #[test]
    fn separates_agent_and_admin_tokens() {
        let dir = tempdir().unwrap();
        let agent_token = dir.path().join("agent-token");
        let admin_token = dir.path().join("admin-token");
        std::fs::write(&agent_token, "agent-secret\n").unwrap();
        std::fs::write(&admin_token, "admin-secret\n").unwrap();
        let collection = RepoCollection::open(dir.path().join("repos"))
            .unwrap()
            .with_token_file(&agent_token)
            .unwrap()
            .with_admin_token_file(&admin_token)
            .unwrap();

        let mut headers = HeaderMap::new();
        assert_eq!(collection.auth_role(&headers), None);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer agent-secret"),
        );
        assert_eq!(collection.auth_role(&headers), Some(AuthRole::Agent));
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_static("Bearer admin-secret"),
        );
        assert_eq!(collection.auth_role(&headers), Some(AuthRole::Admin));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        assert_eq!(collection.auth_role(&headers), None);
    }

    #[test]
    fn only_the_default_branches_are_protected() {
        assert!(protected_branch("source"));
        assert!(protected_branch("main"));
        assert!(protected_branch("master"));
        assert!(!protected_branch("feature"));
        assert!(!protected_branch("design-change/task"));
    }

    #[test]
    fn requires_bearer_token_when_configured() {
        let dir = tempdir().unwrap();
        let token = dir.path().join("token");
        std::fs::write(&token, "secret\n").unwrap();
        let server = RemoteServer::open(dir.path().join("remote"))
            .unwrap()
            .with_token_file(&token)
            .unwrap();

        let mut headers = HeaderMap::new();
        assert!(!server.authorized(&headers));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer wrong"));
        assert!(!server.authorized(&headers));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Basic secret"));
        assert!(!server.authorized(&headers));
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer secret"));
        assert!(server.authorized(&headers));

        let open = RemoteServer::open(dir.path().join("open")).unwrap();
        assert!(open.authorized(&HeaderMap::new()));
    }

    #[test]
    fn stdio_protocol_roundtrip() {
        let dir = tempdir().unwrap();
        let server = RemoteServer::open(dir.path().join("remote")).unwrap();
        let (object_hash, object_bytes) = object_record("files");
        let (commit_hash, commit_bytes) = commit_record(object_hash);
        let object_path = record_path("objects", object_hash.0);
        let commit_path = record_path("commits", commit_hash.0);

        let mut input = Cursor::new([object_path.as_bytes(), &object_bytes].concat());
        handle(
            &server,
            &mut input,
            &format!("PUT {} {}", object_path.len(), object_bytes.len()),
        )
        .unwrap();

        let mut input = Cursor::new([commit_path.as_bytes(), &commit_bytes].concat());
        handle(
            &server,
            &mut input,
            &format!("PUT {} {}", commit_path.len(), commit_bytes.len()),
        )
        .unwrap();

        let body = format!("main\t{}\n", commit_hash.to_hex());
        let mut input = Cursor::new(body.as_bytes().to_vec());
        handle(&server, &mut input, &format!("PUTREF {}", body.len())).unwrap();

        let files = handle(&server, &mut Cursor::new(Vec::new()), "FILES").unwrap();
        assert_eq!(
            String::from_utf8(files).unwrap(),
            format!("{commit_path}\n{object_path}\n")
        );

        let refs = handle(&server, &mut Cursor::new(Vec::new()), "REFS").unwrap();
        assert_eq!(
            String::from_utf8(refs).unwrap(),
            format!("main\t{}\n", commit_hash.to_hex())
        );

        let mut input = Cursor::new(object_path.as_bytes().to_vec());
        let bytes = handle(&server, &mut input, &format!("GET {}", object_path.len())).unwrap();
        assert_eq!(bytes, object_bytes);

        assert!(handle(&server, &mut Cursor::new(Vec::new()), "NOPE").is_err());
    }
}
