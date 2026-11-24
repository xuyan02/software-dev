//! Arche remote client: local, HTTP and SSH transports.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use anyhow::{Context, Result, bail};
use arche_core::pack::{PackKind, PackRecord, PushBundle, RefCommand, encode_bundle};
use arche_core::protocol::{local_refs, read_ref_file, record_files};
use arche_core::{CommitHash, Hash32, ObjectHash, Repository, SymbolHash};

pub fn push(repo: &Repository, force: bool) -> Result<()> {
    let remote_url = repo.remote_url().context("no remote configured")?;
    if is_http(remote_url) {
        return http_push(repo, remote_url, force);
    }
    if is_ssh(remote_url) {
        return ssh_push(repo, remote_url, force);
    }
    let remote = Repository::open(remote_url)?;
    for name in ["objects", "symbols", "commits"] {
        copy_tree(&repo.dir().join(name), &remote.dir().join(name))?;
    }
    let branch = repo.current_branch()?;
    let name = format!("refs/heads/{branch}");
    let head = repo.head_commit()?;
    let expected = remote.ref_hash(&name)?;
    remote.update_ref_checked(&name, expected, head, force)
}

pub fn fetch(repo: &Repository) -> Result<()> {
    let remote_url = repo.remote_url().context("no remote configured")?;
    if is_http(remote_url) {
        return http_fetch(repo, remote_url);
    }
    if is_ssh(remote_url) {
        return ssh_fetch(repo, remote_url);
    }
    let remote = Repository::open(remote_url)?;
    for name in ["objects", "symbols", "commits"] {
        copy_tree(&remote.dir().join(name), &repo.dir().join(name))?;
    }
    let branch = repo.current_branch()?;
    let remote_head = read_ref_file(remote.dir(), &format!("refs/heads/{branch}"))?;
    repo.set_ref(&format!("refs/remotes/origin/{branch}"), remote_head)
}

pub fn pull(repo: &Repository) -> Result<()> {
    let head = repo.head_snapshot()?;
    if repo.index_snapshot()? != head || repo.worktree_snapshot()? != head {
        bail!("index or worktree has local changes; commit or reset first");
    }
    fetch(repo)?;
    let branch = repo.current_branch()?;
    let remote_head = read_ref_file(repo.dir(), &format!("refs/remotes/origin/{branch}"))?;
    repo.set_ref(&format!("refs/heads/{branch}"), remote_head)?;
    repo.adopt_commit(remote_head)
}

pub fn clone_from(
    remote_path: &str,
    dir: impl AsRef<Path>,
    branch: Option<&str>,
) -> Result<Repository> {
    if is_http(remote_path) || is_ssh(remote_path) {
        let repo = Repository::init(dir)?;
        repo.set_remote(remote_path)?;
        if is_http(remote_path) {
            http_fetch(&repo, remote_path)?;
        } else {
            ssh_fetch(&repo, remote_path)?;
        }
        let branches = repo.remote_branches()?;
        let (branch, hash) = match branch {
            Some(branch) => branches
                .into_iter()
                .find(|(name, _)| name == branch)
                .with_context(|| format!("remote branch {branch} not found"))?,
            None => branches
                .into_iter()
                .next()
                .context("remote has no branches")?,
        };
        repo.set_ref(&format!("refs/heads/{branch}"), hash)?;
        repo.set_ref(&format!("refs/remotes/origin/{branch}"), hash)?;
        repo.set_head(&branch)?;
        repo.adopt_commit(hash)?;
        return Ok(repo);
    }

    let remote = Repository::open(remote_path)?;
    let branch = branch.unwrap_or(&remote.current_branch()?).to_owned();
    let repo = Repository::init_with_source(dir, remote.source_url())?;
    for name in ["objects", "symbols", "commits"] {
        copy_tree(&remote.dir().join(name), &repo.dir().join(name))?;
    }
    let remote_head = read_ref_file(remote.dir(), &format!("refs/heads/{branch}"))?;
    repo.set_ref(&format!("refs/heads/{branch}"), remote_head)?;
    repo.set_ref(&format!("refs/remotes/origin/{branch}"), remote_head)?;
    if branch != "main" {
        repo.set_head(&branch)?;
    }
    repo.adopt_commit(remote_head)?;
    repo.set_remote(remote_path)?;
    Ok(repo)
}

fn http_push(repo: &Repository, url: &str, force: bool) -> Result<()> {
    let body = encode_bundle(&build_push_bundle(repo, force)?);
    let base = url.trim_end_matches('/');
    let response = http_client()
        .post(format!("{base}/push"))
        .body(body)
        .send()?;
    if !response.status().is_success() {
        bail!("push: HTTP {}", response.status());
    }
    Ok(())
}

fn build_push_bundle(repo: &Repository, force: bool) -> Result<PushBundle> {
    let mut records = Vec::new();
    for relative in record_files(repo.dir())? {
        let bytes = fs::read(repo.dir().join(&relative))?;
        let name = relative.rsplit('/').next().unwrap_or_default();
        let (kind, hash, payload) = if relative.starts_with("objects/") {
            let hash = ObjectHash::from_hex(name)?;
            let payload = if bytes.starts_with(b"DSO2") {
                bytes[4..].to_vec()
            } else {
                bytes
            };
            (PackKind::Object, hash.0, payload)
        } else if relative.starts_with("symbols/") {
            let hash = SymbolHash::from_hex(name)?;
            (PackKind::Symbol, hash.0, bytes)
        } else if relative.starts_with("commits/") {
            let hash = CommitHash::from_hex(name)?;
            (PackKind::Commit, hash.0, bytes)
        } else if relative.starts_with("packs/") {
            let hash = Hash32::from_hex(name.trim_end_matches(".pack"))?;
            (PackKind::Pack, hash, bytes)
        } else {
            continue;
        };
        records.push(PackRecord {
            kind,
            hash,
            payload,
        });
    }
    let mut refs = Vec::new();
    for (branch, hash) in local_refs(repo.dir())? {
        let expected = repo
            .ref_hash(&format!("refs/remotes/origin/{branch}"))?
            .unwrap_or(CommitHash::ZERO);
        refs.push(RefCommand {
            branch,
            expected,
            new: hash,
            force,
        });
    }
    Ok(PushBundle { records, refs })
}

fn http_fetch(repo: &Repository, url: &str) -> Result<()> {
    let client = http_client();
    let base = url.trim_end_matches('/');
    let refs = remote_refs(url)?;
    for (branch, hash) in refs {
        repo.set_ref(&format!("refs/remotes/origin/{branch}"), hash)?;
    }
    let listing = client.get(format!("{base}/files")).send()?;
    if !listing.status().is_success() {
        bail!("fetch files: HTTP {}", listing.status());
    }
    for relative in listing.text()?.lines() {
        if relative.trim().is_empty() || repo.dir().join(relative).exists() {
            continue;
        }
        let response = client.get(format!("{base}/files/{relative}")).send()?;
        if !response.status().is_success() {
            bail!("fetch {relative}: HTTP {}", response.status());
        }
        let bytes = response.bytes()?;
        repo.write_record(relative, &bytes)?;
    }
    Ok(())
}

fn remote_refs(url: &str) -> Result<Vec<(String, CommitHash)>> {
    let base = url.trim_end_matches('/');
    let response = http_client().get(format!("{base}/refs")).send()?;
    if !response.status().is_success() {
        bail!("fetch refs: HTTP {}", response.status());
    }
    let mut refs = Vec::new();
    for line in response.text()?.lines() {
        let Some((branch, hash)) = line.split_once('\t') else {
            continue;
        };
        refs.push((branch.to_owned(), CommitHash::from_hex(hash)?));
    }
    Ok(refs)
}

fn ssh_push(repo: &Repository, url: &str, force: bool) -> Result<()> {
    let (mut child, mut stdin, mut stdout) = ssh_connect(url)?;
    let bundle = encode_bundle(&build_push_bundle(repo, force)?);
    writeln!(stdin, "BUNDLE {}", bundle.len())?;
    stdin.write_all(&bundle)?;
    stdin.flush()?;
    let mut line = String::new();
    stdout.read_line(&mut line)?;
    if line.trim() != "OK" {
        bail!("ssh push failed: {line}");
    }
    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn ssh_fetch(repo: &Repository, url: &str) -> Result<()> {
    let (mut child, mut stdin, mut stdout) = ssh_connect(url)?;
    writeln!(stdin, "REFS")?;
    stdin.flush()?;
    loop {
        let mut line = String::new();
        stdout.read_line(&mut line)?;
        let line = line.trim_end();
        if line == "END" {
            break;
        }
        if let Some((branch, hash)) = line.split_once('\t') {
            repo.set_ref(
                &format!("refs/remotes/origin/{branch}"),
                CommitHash::from_hex(hash)?,
            )?;
        }
    }
    writeln!(stdin, "LIST")?;
    stdin.flush()?;
    loop {
        let mut line = String::new();
        stdout.read_line(&mut line)?;
        let relative = line.trim_end();
        if relative == "END" {
            break;
        }
        if repo.dir().join(relative).exists() {
            continue;
        }
        writeln!(stdin, "GET {relative}")?;
        stdin.flush()?;
        let mut header = String::new();
        stdout.read_line(&mut header)?;
        let len = header
            .trim()
            .strip_prefix("OK ")
            .with_context(|| format!("ssh fetch {relative}: {header}"))?;
        let len: usize = len.parse()?;
        let mut bytes = vec![0u8; len];
        stdout.read_exact(&mut bytes)?;
        repo.write_record(relative, &bytes)?;
    }
    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn http_client() -> reqwest::blocking::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(token) = auth_token()
        && let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(reqwest::header::AUTHORIZATION, value);
    }
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn auth_token() -> Option<String> {
    if let Ok(token) = std::env::var("ARCHE_TOKEN") {
        return Some(token);
    }
    let mut paths = vec![PathBuf::from("/run/foundry/arche_token")];
    if let Ok(config) = std::env::var("XDG_CONFIG_HOME") {
        paths.push(PathBuf::from(config).join("arche/token"));
    }
    if let Ok(home) = std::env::var("HOME") {
        paths.push(PathBuf::from(home).join(".config/arche/token"));
    }
    for path in paths {
        if let Ok(token) = fs::read_to_string(path) {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn is_http(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn is_ssh(url: &str) -> bool {
    url.starts_with("ssh://")
}

fn ssh_connect(url: &str) -> Result<(Child, ChildStdin, BufReader<ChildStdout>)> {
    let (authority, port, path) = parse_ssh_url(url)?;
    let mut command = Command::new("ssh");
    if let Some(port) = port {
        command.args(["-p", &port]);
    }
    command
        .arg(authority)
        .arg("arche")
        .arg("serve")
        .arg("--stdio")
        .arg("--root")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = command.spawn().context("spawn ssh")?;
    let stdin = child.stdin.take().context("ssh stdin")?;
    let stdout = child.stdout.take().context("ssh stdout")?;
    Ok((child, stdin, BufReader::new(stdout)))
}

fn parse_ssh_url(url: &str) -> Result<(String, Option<String>, String)> {
    let rest = url.strip_prefix("ssh://").context("invalid ssh url")?;
    let (authority, path) = rest.split_once('/').context("ssh url has no path")?;
    if path.is_empty() {
        bail!("ssh url has an empty path");
    }
    let (authority, port) = if let Some((host, port)) = authority.rsplit_once(':') {
        (host.to_owned(), Some(port.to_owned()))
    } else {
        (authority.to_owned(), None)
    };
    Ok((authority, port, format!("/{path}")))
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if !from.exists() {
        return Ok(());
    }
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&source, &target)?;
        } else if !target.exists() {
            fs::copy(&source, &target)
                .with_context(|| format!("copy {} to {}", source.display(), target.display()))?;
        }
    }
    Ok(())
}
