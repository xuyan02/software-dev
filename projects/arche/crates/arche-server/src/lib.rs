//! Arche HTTP and stdio server.
//!
//! The server is a library so Foundry can host it in-process. The `arche`
//! binary exposes the same code as `arche serve`.
//!
//! - HTTP: every repository under `root` is reachable at `/<repo>/...`.
//! - Stdio: one repository, used by the SSH transport.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, Result, bail};
use arche_core::pack::{PackKind, PackRecord, decode_bundle};
use arche_core::protocol::{record_files, valid_record_path};
use arche_core::{CommitHash, Repository};

#[derive(Debug, Clone)]
pub struct ServerOptions {
    /// Directory containing Arche repositories.
    pub root: PathBuf,
    /// HTTP listen address.
    pub listen: String,
    /// Optional bearer token for every HTTP request.
    pub token: Option<String>,
    /// Token allowed to move protected branches.
    pub admin_token: Option<String>,
    /// Branch names that only the admin token may update.
    pub protected: Vec<String>,
}

impl ServerOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            listen: "127.0.0.1:7700".to_string(),
            token: None,
            admin_token: None,
            protected: vec!["main".to_owned()],
        }
    }

    pub fn listen(mut self, listen: impl Into<String>) -> Self {
        self.listen = listen.into();
        self
    }

    pub fn token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    pub fn admin_token(mut self, token: Option<String>) -> Self {
        self.admin_token = token;
        self
    }

    pub fn protected(mut self, branches: Vec<String>) -> Self {
        self.protected = branches;
        self
    }
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

/// Serve every repository under `options.root` over HTTP. Blocks forever.
pub fn serve(options: &ServerOptions) -> Result<()> {
    fs::create_dir_all(&options.root)?;
    let listener =
        TcpListener::bind(&options.listen).with_context(|| format!("bind {}", options.listen))?;
    serve_with_listener(options, listener)
}

/// Serve on an already-bound listener, so tests and embedders can choose the
/// address before starting the loop.
pub fn serve_with_listener(options: &ServerOptions, listener: TcpListener) -> Result<()> {
    fs::create_dir_all(&options.root)?;
    eprintln!("arche listening on {}", listener.local_addr()?);
    for stream in listener.incoming() {
        let stream = stream?;
        let root = options.root.clone();
        let token = options.token.clone();
        let admin_token = options.admin_token.clone();
        let protected = options.protected.clone();
        thread::spawn(move || {
            if let Err(error) = handle(
                stream,
                &root,
                token.as_deref(),
                admin_token.as_deref(),
                &protected,
            ) {
                eprintln!("{error:#}");
            }
        });
    }
    Ok(())
}

/// Serve one repository over stdin/stdout. Blocks until EOF.
pub fn serve_stdio(root: impl AsRef<Path>) -> Result<()> {
    run_stdio(root.as_ref())
}

fn handle(
    mut stream: TcpStream,
    root: &Path,
    token: Option<&str>,
    admin_token: Option<&str>,
    protected: &[String],
) -> Result<()> {
    let request = read_request(&mut stream)?;
    let authorization = request.authorization.as_deref();
    let admin = admin_token.is_some_and(|token| authorization == Some(&format!("Bearer {token}")));
    if let Some(token) = token
        && authorization != Some(&format!("Bearer {token}"))
        && !admin
    {
        write_response(&mut stream, "401 Unauthorized", b"unauthorized")?;
        return Ok(());
    }
    let response = route(root, &request, admin, protected);
    match response {
        Ok((status, body)) => write_response(&mut stream, status, &body),
        Err(error) => write_response(
            &mut stream,
            "500 Internal Server Error",
            format!("{error:#}").as_bytes(),
        ),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            bail!("connection closed");
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_header_end(&buffer) {
            let headers = String::from_utf8_lossy(&buffer[..end]).to_string();
            let mut lines = headers.lines();
            let first = lines.next().context("missing request line")?;
            let mut first_parts = first.split_whitespace();
            let method = first_parts.next().context("missing method")?.to_owned();
            let path = first_parts.next().context("missing path")?.to_owned();
            let mut content_length = 0usize;
            let mut authorization = None;
            for line in lines {
                if let Some((name, value)) = line.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().unwrap_or(0);
                    } else if name.eq_ignore_ascii_case("authorization") {
                        authorization = Some(value.trim().to_owned());
                    }
                }
            }
            let body_start = end + 4;
            while buffer.len() < body_start + content_length {
                let read = stream.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..read]);
            }
            let body = buffer[body_start..body_start + content_length].to_vec();
            return Ok(Request {
                method,
                path,
                authorization,
                body,
            });
        }
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn route(
    root: &Path,
    request: &Request,
    admin: bool,
    protected: &[String],
) -> Result<(&'static str, Vec<u8>)> {
    let path = request.path.split('?').next().unwrap_or(&request.path);
    let path = path.trim_start_matches('/');
    let (repo, rest) = path.split_once('/').unwrap_or((path, ""));
    if repo.is_empty() || repo.contains("..") {
        bail!("invalid repository");
    }
    match (request.method.as_str(), repo, rest) {
        ("GET", "repos", "") => {
            let mut names = Vec::new();
            for entry in fs::read_dir(root)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() && entry.path().join(".arche").is_dir() {
                    names.push(entry.file_name().to_string_lossy().to_string());
                }
            }
            names.sort();
            Ok(("200 OK", names.join("\n").into_bytes()))
        }
        ("POST", "repos", "") => {
            let query = request.path.split_once('?').map(|(_, q)| q).unwrap_or("");
            let mut name = None;
            let mut source = None;
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    match key {
                        "name" => name = Some(value.to_owned()),
                        "source" => source = Some(value.to_owned()),
                        _ => {}
                    }
                }
            }
            let name = name.context("missing name")?;
            if name.contains('/') || name.contains("..") {
                bail!("invalid repository name");
            }
            Repository::init_with_source(root.join(&name), source.as_deref())?;
            Ok(("201 Created", Vec::new()))
        }
        (method, _, _) => {
            let repo_path = root.join(repo);
            let repo = Repository::open(&repo_path)?;
            match (method, rest) {
                ("GET", "refs") => {
                    let mut body = String::new();
                    for branch in repo.branch_list()? {
                        let path = repo.dir().join("refs/heads").join(&branch);
                        let hash = fs::read_to_string(path)?;
                        body.push_str(&format!("{branch}\t{}\n", hash.trim()));
                    }
                    Ok(("200 OK", body.into_bytes()))
                }
                ("PUT", "refs") => {
                    if let Some(id) = push_id(&request.path)
                        && let Err(error) = repo.publish_incoming(id)
                    {
                        let _ = repo.discard_incoming(id);
                        return Err(error);
                    }
                    let mut updates = Vec::new();
                    for line in String::from_utf8(request.body.clone())?.lines() {
                        let fields = line.split('\t').collect::<Vec<_>>();
                        if fields.len() < 3 {
                            continue;
                        }
                        let branch = fields[0];
                        if !admin && protected.iter().any(|name| name == branch) {
                            return Ok((
                                "403 Forbidden",
                                b"protected branch requires admin token".to_vec(),
                            ));
                        }
                        let expected = CommitHash::from_hex(fields[1])?;
                        let new = CommitHash::from_hex(fields[2])?;
                        let force = fields.get(3).is_some_and(|value| *value == "1");
                        updates.push((
                            format!("refs/heads/{branch}"),
                            (!expected.is_zero()).then_some(expected),
                            new,
                            force,
                        ));
                    }
                    if let Err(error) = repo.update_refs_transaction(&updates) {
                        return Ok(("409 Conflict", format!("{error:#}").into_bytes()));
                    }
                    Ok(("200 OK", Vec::new()))
                }
                ("POST", "push") => {
                    let bundle = decode_bundle(&request.body)?;
                    let id = format!(
                        "{}-{}",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)?
                            .as_nanos()
                    );
                    write_bundle_records(&repo, &id, &bundle.records)?;
                    repo.publish_incoming(&id)?;
                    let mut updates = Vec::new();
                    for command in &bundle.refs {
                        if !admin && protected.iter().any(|branch| branch == &command.branch) {
                            return Ok((
                                "403 Forbidden",
                                b"protected branch requires admin token".to_vec(),
                            ));
                        }
                        updates.push((
                            format!("refs/heads/{}", command.branch),
                            (!command.expected.is_zero()).then_some(command.expected),
                            command.new,
                            command.force,
                        ));
                    }
                    if let Err(error) = repo.update_refs_transaction(&updates) {
                        return Ok(("409 Conflict", format!("{error:#}").into_bytes()));
                    }
                    Ok(("200 OK", Vec::new()))
                }
                ("GET", "files") => {
                    let mut body = String::new();
                    for relative in record_files(repo.dir())? {
                        body.push_str(&relative);
                        body.push('\n');
                    }
                    Ok(("200 OK", body.into_bytes()))
                }
                (method, relative) if relative.starts_with("files/") => {
                    let relative = relative.trim_start_matches("files/");
                    if relative.contains("..")
                        || !(relative.starts_with("objects/")
                            || relative.starts_with("symbols/")
                            || relative.starts_with("commits/")
                            || relative.starts_with("packs/"))
                    {
                        bail!("invalid record path");
                    }
                    let path = repo.dir().join(relative);
                    match method {
                        "GET" => Ok(("200 OK", fs::read(path)?)),
                        "PUT" => {
                            if let Some(id) = push_id(&request.path) {
                                let path = repo.incoming_path(id)?.join(relative);
                                if let Some(parent) = path.parent() {
                                    fs::create_dir_all(parent)?;
                                }
                                fs::write(path, &request.body)?;
                            } else {
                                repo.write_record(relative, &request.body)?;
                            }
                            Ok(("200 OK", Vec::new()))
                        }
                        _ => Ok(("405 Method Not Allowed", Vec::new())),
                    }
                }
                _ => Ok(("404 Not Found", Vec::new())),
            }
        }
    }
}

fn push_id(path: &str) -> Option<&str> {
    let query = path.split_once('?')?.1;
    for pair in query.split('&') {
        if let Some(("push", value)) = pair.split_once('=') {
            return Some(value);
        }
    }
    None
}

fn write_bundle_records(repo: &Repository, id: &str, records: &[PackRecord]) -> Result<()> {
    if records.iter().any(|record| record.hash.is_zero()) {
        bail!("bundle contains a zero hash");
    }
    let incoming = repo.incoming_path(id)?;
    for record in records {
        if record.kind == PackKind::Object && record.payload.len() < 2 {
            bail!("object bundle record is too short");
        }
        let hex = record.hash.to_hex();
        let relative = match record.kind {
            PackKind::Object => format!("objects/{}/{}/{}", &hex[..2], &hex[2..4], hex),
            PackKind::Symbol => format!("symbols/{}/{}/{}", &hex[..2], &hex[2..4], hex),
            PackKind::Commit => format!("commits/{}/{}/{}", &hex[..2], &hex[2..4], hex),
            PackKind::Pack => format!("packs/{hex}.pack"),
        };
        let path = incoming.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = if record.kind == PackKind::Object {
            let mut bytes = b"DSO2".to_vec();
            bytes.extend_from_slice(&record.payload);
            bytes
        } else {
            record.payload.clone()
        };
        fs::write(path, bytes)?;
    }
    Ok(())
}

fn write_response(stream: &mut TcpStream, status: &str, body: &[u8]) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn run_stdio(root: &Path) -> Result<()> {
    let repo = Repository::open(root)?;
    let stdin = std::io::stdin();
    let mut input = BufReader::new(stdin.lock());
    let mut output = std::io::stdout();
    let mut push: Option<String> = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(len) = line.strip_prefix("BUNDLE ") {
            let len: usize = len.parse()?;
            let mut bytes = vec![0u8; len];
            input.read_exact(&mut bytes)?;
            let result = (|| -> Result<()> {
                let bundle = decode_bundle(&bytes)?;
                let id = format!(
                    "{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_nanos()
                );
                write_bundle_records(&repo, &id, &bundle.records)?;
                repo.publish_incoming(&id)?;
                let updates = bundle
                    .refs
                    .iter()
                    .map(|command| {
                        (
                            format!("refs/heads/{}", command.branch),
                            (!command.expected.is_zero()).then_some(command.expected),
                            command.new,
                            command.force,
                        )
                    })
                    .collect::<Vec<_>>();
                repo.update_refs_transaction(&updates)
            })();
            match result {
                Ok(()) => output.write_all(b"OK\n")?,
                Err(error) => {
                    eprintln!("{error:#}");
                    output.write_all(b"ERR\n")?;
                }
            }
            output.flush()?;
        } else if let Some(id) = line.strip_prefix("PUSH ") {
            push = Some(id.to_owned());
        } else if line == "REFS" {
            let mut body = String::new();
            for branch in repo.branch_list()? {
                let path = repo.dir().join("refs/heads").join(&branch);
                let hash = fs::read_to_string(path)?;
                body.push_str(&format!("{branch}\t{}\n", hash.trim()));
            }
            body.push_str("END\n");
            output.write_all(body.as_bytes())?;
            output.flush()?;
        } else if line == "LIST" {
            let mut body = String::new();
            for relative in record_files(repo.dir())? {
                body.push_str(&relative);
                body.push('\n');
            }
            body.push_str("END\n");
            output.write_all(body.as_bytes())?;
            output.flush()?;
        } else if let Some(relative) = line.strip_prefix("GET ") {
            if !valid_record_path(relative) {
                output.write_all(b"ERR\n")?;
                output.flush()?;
                continue;
            }
            let bytes = fs::read(repo.dir().join(relative))?;
            writeln!(output, "OK {}", bytes.len())?;
            output.write_all(&bytes)?;
            output.flush()?;
        } else if let Some(rest) = line.strip_prefix("PUT ") {
            let Some((relative, len)) = rest.rsplit_once(' ') else {
                output.write_all(b"ERR\n")?;
                output.flush()?;
                continue;
            };
            if !valid_record_path(relative) {
                output.write_all(b"ERR\n")?;
                output.flush()?;
                continue;
            }
            let len: usize = len.parse()?;
            let mut bytes = vec![0u8; len];
            input.read_exact(&mut bytes)?;
            let path = match &push {
                Some(id) => repo.incoming_path(id)?.join(relative),
                None => repo.dir().join(relative),
            };
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, bytes)?;
            output.write_all(b"OK\n")?;
            output.flush()?;
        } else if line == "REFSET" {
            let mut result = match &push {
                Some(id) => repo.publish_incoming(id).map(|_| ()),
                None => Ok(()),
            };
            if result.is_err()
                && let Some(id) = &push
            {
                let _ = repo.discard_incoming(id);
            }
            loop {
                let mut ref_line = String::new();
                if input.read_line(&mut ref_line)? == 0 {
                    return Ok(());
                }
                let ref_line = ref_line.trim_end_matches(['\r', '\n']);
                if ref_line == "END" {
                    break;
                }
                let fields = ref_line.split('\t').collect::<Vec<_>>();
                if fields.len() < 3 {
                    continue;
                }
                let branch = fields[0];
                let expected = CommitHash::from_hex(fields[1])?;
                let new = CommitHash::from_hex(fields[2])?;
                let force = fields.get(3).is_some_and(|value| *value == "1");
                if result.is_ok() {
                    result = repo.update_ref_checked(
                        &format!("refs/heads/{branch}"),
                        (!expected.is_zero()).then_some(expected),
                        new,
                        force,
                    );
                }
            }
            match result {
                Ok(()) => output.write_all(b"OK\n")?,
                Err(error) => {
                    eprintln!("{error:#}");
                    output.write_all(b"ERR\n")?;
                }
            }
            if let Some(id) = &push {
                let _ = repo.discard_incoming(id);
            }
            push = None;
            output.flush()?;
        } else {
            output.write_all(b"ERR\n")?;
            output.flush()?;
        }
    }
}
