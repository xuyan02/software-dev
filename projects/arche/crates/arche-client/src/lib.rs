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
    remote.update_ref_checked(&name, expected, head, force)?;
    let merged = remote.import_comments(&repo.export_comments()?, false)?;
    repo.replace_comments(&merged)?;
    Ok(())
}

pub fn fetch(repo: &Repository) -> Result<()> {
    let remote_url = repo.remote_url().context("no remote configured")?;
    if is_http(remote_url) {
        http_fetch(repo, remote_url)?;
        return Ok(());
    }
    if is_ssh(remote_url) {
        ssh_fetch(repo, remote_url)?;
        return Ok(());
    }
    let remote = Repository::open(remote_url)?;
    for name in ["objects", "symbols", "commits"] {
        copy_tree(&remote.dir().join(name), &repo.dir().join(name))?;
    }
    let branch = repo.current_branch()?;
    let remote_head = read_ref_file(remote.dir(), &format!("refs/heads/{branch}"))?;
    repo.set_ref(&format!("refs/remotes/origin/{branch}"), remote_head)?;
    repo.import_comments(&remote.export_comments()?, true)?;
    Ok(())
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
    let path = dir.as_ref().to_path_buf();
    if is_http(remote_path) || is_ssh(remote_path) {
        let repo = Repository::init(&path)?;
        repo.set_remote(remote_path)?;
        let repo = Repository::open(&path)?;
        let source = if is_http(remote_path) {
            http_fetch(&repo, remote_path)?
        } else {
            ssh_fetch(&repo, remote_path)?
        };
        if let Some(source) = source {
            repo.set_source_url(&source)?;
        }
        let branches = repo.remote_branches()?;
        let (branch, hash) = match branch {
            Some(branch) => branches
                .iter()
                .find(|(name, _)| name == branch)
                .map(|(name, hash)| (name.clone(), *hash))
                .with_context(|| format!("remote branch {branch} not found"))?,
            None => branches
                .first()
                .map(|(name, hash)| (name.clone(), *hash))
                .context("remote has no branches")?,
        };
        // Repository::init creates a bootstrap main branch. Point every local
        // branch at the remote commit so a subsequent push does not send a
        // stale, non-fast-forward branch.
        for (name, remote_hash) in &branches {
            repo.set_ref(&format!("refs/heads/{name}"), *remote_hash)?;
        }
        repo.set_ref(&format!("refs/heads/{branch}"), hash)?;
        repo.set_ref(&format!("refs/remotes/origin/{branch}"), hash)?;
        repo.set_head(&branch)?;
        repo.adopt_commit(hash)?;
        return Repository::open(&path);
    }

    let remote = Repository::open(remote_path)?;
    let branch = branch.unwrap_or(&remote.current_branch()?).to_owned();
    let repo = Repository::init_with_source(&path, remote.source_url())?;
    for name in ["objects", "symbols", "commits"] {
        copy_tree(&remote.dir().join(name), &repo.dir().join(name))?;
    }
    let remote_head = read_ref_file(remote.dir(), &format!("refs/heads/{branch}"))?;
    repo.set_ref(&format!("refs/heads/{branch}"), remote_head)?;
    repo.set_ref(&format!("refs/remotes/origin/{branch}"), remote_head)?;
    repo.import_comments(&remote.export_comments()?, true)?;
    if branch != "main" {
        repo.set_head(&branch)?;
    }
    repo.adopt_commit(remote_head)?;
    repo.set_remote(remote_path)?;
    Repository::open(&path)
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
    let comments = http_client()
        .put(format!("{base}/comments"))
        .body(repo.export_comments()?)
        .send()?;
    if !comments.status().is_success() {
        bail!("push comments: HTTP {}", comments.status());
    }
    repo.replace_comments(&comments.bytes()?)?;
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

fn http_fetch(repo: &Repository, url: &str) -> Result<Option<String>> {
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
    let comments = client.get(format!("{base}/comments")).send()?;
    if comments.status().is_success() {
        repo.import_comments(&comments.bytes()?, true)?;
    } else if comments.status() != reqwest::StatusCode::NOT_FOUND {
        bail!("fetch comments: HTTP {}", comments.status());
    }
    http_source(url)
}

fn http_source(url: &str) -> Result<Option<String>> {
    let base = url.trim_end_matches('/');
    let response = http_client().get(format!("{base}/source")).send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!("fetch source: HTTP {}", response.status());
    }
    let source = response.text()?.trim_end_matches(['\r', '\n']).to_owned();
    Ok((!source.is_empty()).then_some(source))
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
    if stdout.read_line(&mut line)? == 0 {
        bail!("ssh push failed: connection closed before response");
    }
    if line.trim() != "OK" {
        bail!("ssh push failed: {line}");
    }
    let comments = repo.export_comments()?;
    writeln!(stdin, "COMMENTS {}", comments.len())?;
    stdin.write_all(&comments)?;
    stdin.flush()?;
    let mut header = String::new();
    if stdout.read_line(&mut header)? == 0 {
        bail!("ssh push comments: connection closed before response");
    }
    let len = header
        .trim()
        .strip_prefix("OK ")
        .with_context(|| format!("ssh push comments: {header}"))?;
    let len: usize = len.parse()?;
    let mut bytes = vec![0u8; len];
    stdout.read_exact(&mut bytes)?;
    repo.replace_comments(&bytes)?;
    drop(stdin);
    let _ = child.wait();
    Ok(())
}

fn read_ssh_file_list(stdout: &mut impl BufRead) -> Result<Vec<String>> {
    let mut files = Vec::new();
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line)? == 0 {
            bail!("ssh fetch: connection closed while reading file list");
        }
        let relative = line.trim_end();
        if relative == "END" {
            break;
        }
        files.push(relative.to_owned());
    }
    Ok(files)
}

fn ssh_fetch(repo: &Repository, url: &str) -> Result<Option<String>> {
    let (mut child, mut stdin, mut stdout) = ssh_connect(url)?;
    writeln!(stdin, "REFS")?;
    stdin.flush()?;
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line)? == 0 {
            bail!("ssh fetch: connection closed while reading refs");
        }
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
    let files = read_ssh_file_list(&mut stdout)?;
    for relative in files {
        if repo.dir().join(&relative).exists() {
            continue;
        }
        writeln!(stdin, "GET {relative}")?;
        stdin.flush()?;
        let mut header = String::new();
        if stdout.read_line(&mut header)? == 0 {
            bail!("ssh fetch {relative}: connection closed before response");
        }
        let len = header
            .trim()
            .strip_prefix("OK ")
            .with_context(|| format!("ssh fetch {relative}: {header}"))?;
        let len: usize = len.parse()?;
        let mut bytes = vec![0u8; len];
        stdout.read_exact(&mut bytes)?;
        repo.write_record(&relative, &bytes)?;
    }
    writeln!(stdin, "SOURCE")?;
    stdin.flush()?;
    let mut header = String::new();
    if stdout.read_line(&mut header)? == 0 {
        bail!("ssh fetch: connection closed before source response");
    }
    let len = header
        .trim()
        .strip_prefix("OK ")
        .with_context(|| format!("ssh fetch source: {header}"))?;
    let len: usize = len.parse()?;
    let mut bytes = vec![0u8; len];
    stdout.read_exact(&mut bytes)?;
    let source = if bytes.is_empty() {
        None
    } else {
        Some(String::from_utf8(bytes).context("ssh source URL is not UTF-8")?)
    };
    writeln!(stdin, "COMMENTS")?;
    stdin.flush()?;
    let mut header = String::new();
    if stdout.read_line(&mut header)? == 0 {
        bail!("ssh fetch comments: connection closed before response");
    }
    let len = header
        .trim()
        .strip_prefix("OK ")
        .with_context(|| format!("ssh fetch comments: {header}"))?;
    let len: usize = len.parse()?;
    let mut bytes = vec![0u8; len];
    stdout.read_exact(&mut bytes)?;
    repo.import_comments(&bytes, true)?;
    drop(stdin);
    let _ = child.wait();
    Ok(source)
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

#[cfg(test)]
mod tests {
    use super::read_ssh_file_list;
    use std::io::Cursor;

    #[test]
    fn reads_the_whole_file_list_before_getting_files() {
        let mut text = String::new();
        for index in 0..500 {
            text.push_str(&format!("objects/{index:04}/record\n"));
        }
        text.push_str("END\n");
        let mut reader = Cursor::new(text.into_bytes());
        let files = read_ssh_file_list(&mut reader).unwrap();
        assert_eq!(files.len(), 500);
        assert_eq!(files.last().unwrap(), "objects/0499/record");
    }

    #[test]
    fn rejects_a_file_list_that_ends_without_end() {
        let mut reader = Cursor::new(b"objects/0001/record\n".to_vec());
        assert!(read_ssh_file_list(&mut reader).is_err());
    }
}
