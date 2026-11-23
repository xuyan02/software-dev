use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

use anyhow::{Context, Result, bail};
use arche::Repository;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "arche-server", version, about = "Arche HTTP server")]
struct Cli {
    /// Directory containing Arche repositories.
    #[arg(long)]
    root: PathBuf,
    /// Listen address.
    #[arg(long, default_value = "127.0.0.1:7700")]
    listen: String,
    /// Optional bearer token.
    #[arg(long)]
    token: Option<String>,
    /// Serve a single repository over stdin/stdout for SSH.
    #[arg(long)]
    stdio: bool,
}

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.stdio {
        return run_stdio(&cli.root);
    }
    fs::create_dir_all(&cli.root)?;
    let listener =
        TcpListener::bind(&cli.listen).with_context(|| format!("bind {}", cli.listen))?;
    println!("arche-server listening on {}", cli.listen);
    for stream in listener.incoming() {
        let stream = stream?;
        let root = cli.root.clone();
        let token = cli.token.clone();
        thread::spawn(move || {
            if let Err(error) = handle(stream, &root, token.as_deref()) {
                eprintln!("{error:#}");
            }
        });
    }
    Ok(())
}

fn handle(mut stream: TcpStream, root: &Path, token: Option<&str>) -> Result<()> {
    let request = read_request(&mut stream)?;
    if let Some(token) = token
        && request.authorization.as_deref() != Some(&format!("Bearer {token}"))
    {
        write_response(&mut stream, "401 Unauthorized", b"unauthorized")?;
        return Ok(());
    }
    let response = route(root, &request);
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

fn route(root: &Path, request: &Request) -> Result<(&'static str, Vec<u8>)> {
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
                    for line in String::from_utf8(request.body.clone())?.lines() {
                        let Some((branch, hash)) = line.split_once('\t') else {
                            continue;
                        };
                        let path = repo.dir().join("refs/heads").join(branch);
                        if let Some(parent) = path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(path, format!("{hash}\n"))?;
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
                            || relative.starts_with("commits/"))
                    {
                        bail!("invalid record path");
                    }
                    let path = repo.dir().join(relative);
                    match method {
                        "GET" => Ok(("200 OK", fs::read(path)?)),
                        "PUT" => {
                            if let Some(parent) = path.parent() {
                                fs::create_dir_all(parent)?;
                            }
                            fs::write(path, &request.body)?;
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

fn record_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for name in ["objects", "symbols", "commits"] {
        let base = root.join(name);
        if !base.exists() {
            continue;
        }
        let mut stack = vec![base.clone()];
        while let Some(path) = stack.pop() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    stack.push(path);
                } else {
                    let relative = path
                        .strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.push(relative);
                }
            }
        }
    }
    files.sort();
    Ok(files)
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
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "REFS" {
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
            let path = repo.dir().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(path, bytes)?;
            output.write_all(b"OK\n")?;
            output.flush()?;
        } else if line == "REFSET" {
            loop {
                let mut ref_line = String::new();
                if input.read_line(&mut ref_line)? == 0 {
                    return Ok(());
                }
                let ref_line = ref_line.trim_end_matches(['\r', '\n']);
                if ref_line == "END" {
                    break;
                }
                let Some((branch, hash)) = ref_line.split_once('\t') else {
                    continue;
                };
                let path = repo.dir().join("refs/heads").join(branch);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, format!("{hash}\n"))?;
            }
            output.write_all(b"OK\n")?;
            output.flush()?;
        } else {
            output.write_all(b"ERR\n")?;
            output.flush()?;
        }
    }
}

fn valid_record_path(relative: &str) -> bool {
    !relative.contains("..")
        && (relative.starts_with("objects/")
            || relative.starts_with("symbols/")
            || relative.starts_with("commits/"))
}
