//! Remote storage service.
//!
//! The service exposes a content-addressed directory over HTTP or over a
//! length-prefixed stdio protocol used by the SSH transport. Both expose the
//! same operations:
//!
//! - list refs;
//! - list immutable files;
//! - read one immutable file;
//! - write one immutable file;
//! - update refs.
//!
//! Uploads are verified against their content hash before they are published.
//! HTTP can require a shared bearer token; SSH relies on SSH authentication.

use crate::remote::{
    FILE_ROOTS, MAX_TRANSFER, collect_files, parse_hash, parse_refs, publish, read_token_file,
    valid_branch, valid_relative, write_ref,
};
use anyhow::{Context, Result, bail};
use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path as AxumPath, Request, State};
use axum::http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_PATH: usize = 4096;

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
        let Some(expected) = self.token_hash else {
            return true;
        };
        let Some(value) = headers.get(AUTHORIZATION) else {
            return false;
        };
        let Ok(value) = value.to_str() else {
            return false;
        };
        let Some((scheme, token)) = value.split_once(' ') else {
            return false;
        };
        scheme.eq_ignore_ascii_case("Bearer") && constant_time_eq(&expected, &hash_token(token))
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
pub fn serve_http(server: RemoteServer, listen: SocketAddr) -> Result<()> {
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
        axum::serve(listener, app(Arc::new(server)))
            .await
            .context("HTTP server failed")?;
        Ok(())
    })
}

fn app(server: Arc<RemoteServer>) -> Router {
    Router::new()
        .route("/v1/refs", get(get_refs).put(put_refs))
        .route("/v1/files", get(list_files))
        .route("/v1/files/{*path}", get(get_file).put(put_file))
        .layer(DefaultBodyLimit::max(MAX_TRANSFER))
        .layer(middleware::from_fn_with_state(server.clone(), require_auth))
        .with_state(server)
}

async fn require_auth(
    State(server): State<Arc<RemoteServer>>,
    request: Request,
    next: Next,
) -> Response {
    if server.authorized(request.headers()) {
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

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (left, right)| difference | (left ^ right))
        == 0
}

type ApiError = (StatusCode, String);
type ApiResult = std::result::Result<Response, ApiError>;

async fn get_refs(State(server): State<Arc<RemoteServer>>) -> ApiResult {
    Ok(server.refs().map_err(bad_request)?.into_response())
}

async fn put_refs(State(server): State<Arc<RemoteServer>>, body: Bytes) -> ApiResult {
    server.write_refs(&body).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn list_files(State(server): State<Arc<RemoteServer>>) -> ApiResult {
    Ok(server.files().map_err(bad_request)?.into_response())
}

async fn get_file(
    State(server): State<Arc<RemoteServer>>,
    AxumPath(path): AxumPath<String>,
) -> ApiResult {
    match server.read(&path).map_err(bad_request)? {
        Some(bytes) => Ok(bytes.into_response()),
        None => Err((StatusCode::NOT_FOUND, "file not found".to_owned())),
    }
}

async fn put_file(
    State(server): State<Arc<RemoteServer>>,
    AxumPath(path): AxumPath<String>,
    body: Bytes,
) -> ApiResult {
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
        Commit, CommitHash, DesignObject, DirectoryPayload, DocHash, Kind, ObjectHash,
        ObjectPayload, ParentRef,
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
                design_doc: DocHash::ZERO,
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
