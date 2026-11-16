//! Read-only access to a local design-space repository for the console.
//!
//! Foundry owns the `ds-server` repository root. Each project is one
//! repository under that root. The console reads commits, Views, objects and
//! design documents directly from the repository; a bare clone of the project's
//! Git repository supplies source snippets. No source code is executed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use design_space as ds;
use ds::catalog::Catalog;
use ds::model::{
    CommitHash, DesignObject, DocHash, GitOid, Hash32, Kind, ObjectPayload, ViewPayload,
};
use ds::store::Store;
use foundry_domain::Project;
use foundry_web::{
    DesignSpaceDetail, DesignSpaceElement, DesignSpaceLine, DesignSpaceOverview, DesignSpaceSource,
};
use sha2::{Digest, Sha256};

const SOURCE_CONTEXT: u32 = 18;
const SOURCE_MAX_LINES: u32 = 600;

/// Repository collection root plus the source-clone cache.
#[derive(Clone)]
pub struct DesignSpaceAccess {
    root: PathBuf,
    cache_root: Option<PathBuf>,
    git_lock: Arc<Mutex<()>>,
}

impl DesignSpaceAccess {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let cache_root = foundry_shared::default_config_path()
            .and_then(|path| path.parent().map(|dir| dir.join("source-cache")));
        Self {
            root: root.into(),
            cache_root,
            git_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn overview(&self, project: &Project) -> Result<DesignSpaceOverview, String> {
        let access = self.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || access.overview_blocking(&project))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn element(
        &self,
        project: &Project,
        hash: &str,
    ) -> Result<DesignSpaceDetail, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || access.element_blocking(&project, &hash))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn source(&self, project: &Project, hash: &str) -> Result<DesignSpaceSource, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        tokio::task::spawn_blocking(move || access.source_blocking(&project, &hash))
            .await
            .map_err(|error| error.to_string())?
    }

    fn overview_blocking(&self, project: &Project) -> Result<DesignSpaceOverview, String> {
        let branch = branch_of(project);
        let mut overview = DesignSpaceOverview {
            branch: branch.clone(),
            version: None,
            source_commit: String::new(),
            file_count: 0,
            element_count: 0,
            designed_count: 0,
            elements: Vec::new(),
        };
        let Some(loaded) = self.load_branch(project, &branch)? else {
            return Ok(overview);
        };
        overview.version = Some(loaded.commit_hash.to_hex());
        overview.source_commit = git_oid_hex(loaded.view_payload.source_commit);

        for (qname, object) in loaded.catalog.iter() {
            let element = element_info(&loaded.store, qname, object)?;
            if element.is_file {
                overview.file_count += 1;
            } else {
                overview.element_count += 1;
                if element.has_design {
                    overview.designed_count += 1;
                }
            }
            overview.elements.push(element);
        }
        overview.elements.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(overview)
    }

    fn element_blocking(&self, project: &Project, hash: &str) -> Result<DesignSpaceDetail, String> {
        let branch = branch_of(project);
        let loaded = self
            .load_branch(project, &branch)?
            .ok_or_else(|| "设计空间分支还没有提交".to_string())?;
        let (qname, object) = find_object(&loaded.catalog, hash)?;
        let info = element_info(&loaded.store, qname, object)?;
        let design = design_doc(object)
            .filter(|hash| !hash.is_zero())
            .map(|hash| loaded.store.get_doc(hash).map_err(display))
            .transpose()?;
        Ok(DesignSpaceDetail {
            hash: info.hash,
            name: info.name,
            lang: info.lang,
            kind: info.kind,
            path: info.path,
            start_line: info.start_line,
            end_line: info.end_line,
            design,
        })
    }

    fn source_blocking(&self, project: &Project, hash: &str) -> Result<DesignSpaceSource, String> {
        let branch = branch_of(project);
        let loaded = self
            .load_branch(project, &branch)?
            .ok_or_else(|| "设计空间分支还没有提交".to_string())?;
        let (_, object) = find_object(&loaded.catalog, hash)?;
        let commit = git_oid_hex(loaded.view_payload.source_commit);
        if commit.is_empty() {
            return Err("该设计空间还没有绑定源码提交".to_string());
        }
        let Some(path) = source_path(&loaded.store, object)? else {
            return Err("该对象没有源码锚点".to_string());
        };
        let (start_line, end_line) = source_span(object);
        let repo = self.ensure_source(project, &commit)?;
        let text = read_blob(&repo, &commit, &path)?;
        source_lines(&text, &commit, &path, start_line, end_line)
    }

    fn repo_path(&self, project: &Project) -> Result<PathBuf, String> {
        let name = repo_name(&project.design_space_url)
            .ok_or_else(|| "项目还没有分配设计空间".to_string())?;
        let path = self.root.join(&name);
        if !path.is_dir() {
            return Err(format!("设计空间仓库不存在: {name}"));
        }
        Ok(path)
    }

    fn load_branch(&self, project: &Project, branch: &str) -> Result<Option<Loaded>, String> {
        let path = self.repo_path(project)?;
        let Some(commit_hash) = read_branch_commit(&path, branch)? else {
            return Ok(None);
        };
        let store = Store::open(&path).map_err(display)?;
        let commit = store.get_commit(commit_hash).map_err(display)?;
        let view_object = store.get_object(commit.view).map_err(display)?;
        let ObjectPayload::View(view_payload) = view_object.payload else {
            return Err("提交的 View 不是 view 对象".to_string());
        };
        let catalog = Catalog::load(&store, commit.view).map_err(display)?;
        Ok(Some(Loaded {
            store,
            commit_hash,
            view_payload,
            catalog,
        }))
    }

    fn ensure_source(&self, project: &Project, commit: &str) -> Result<PathBuf, String> {
        if project.git_repository.trim().is_empty() {
            return Err("项目没有配置 Git 仓库".to_string());
        }
        let cache_root = self
            .cache_root
            .as_ref()
            .ok_or_else(|| "无法确定源码缓存目录".to_string())?;
        fs::create_dir_all(cache_root).map_err(|error| error.to_string())?;
        let key = format!(
            "{}-{}",
            project.id,
            short_hash(project.git_repository.trim())
        );
        let dir = cache_root.join(key);
        let _guard = self.git_lock.lock().map_err(|error| error.to_string())?;

        if !dir.join("HEAD").is_file() {
            if dir.exists() {
                fs::remove_dir_all(&dir)
                    .map_err(|error| format!("清理不完整的源码缓存 {}: {error}", dir.display()))?;
            }
            let output = git(
                None,
                &[
                    "clone",
                    "--bare",
                    "--quiet",
                    "--",
                    project.git_repository.trim(),
                    &dir.to_string_lossy(),
                ],
            )?;
            if !output.status.success() {
                let _ = fs::remove_dir_all(&dir);
                return Err(git_error("无法克隆源码仓库", &output));
            }
        }

        if has_commit(&dir, commit) {
            return Ok(dir);
        }
        let output = git(Some(&dir), &["fetch", "--quiet", "origin"])?;
        if !output.status.success() {
            return Err(git_error("无法从源码仓库拉取缺失的 commit", &output));
        }
        if !has_commit(&dir, commit) {
            return Err(format!(
                "源码仓库中没有 commit {}；可能还没有推送该提交",
                short(commit)
            ));
        }
        Ok(dir)
    }
}

struct Loaded {
    store: Store,
    commit_hash: CommitHash,
    view_payload: ViewPayload,
    catalog: Catalog,
}

fn branch_of(project: &Project) -> String {
    let branch = project.design_space_branch.trim();
    if branch.is_empty() {
        "main".to_owned()
    } else {
        branch.to_owned()
    }
}

fn read_branch_commit(path: &Path, branch: &str) -> Result<Option<CommitHash>, String> {
    if branch.is_empty()
        || branch
            .split('/')
            .any(|part| part == ".." || part.is_empty())
    {
        return Err(format!("invalid branch: {branch}"));
    }
    let file = path.join("refs/heads").join(branch);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let hash =
        Hash32::from_hex(text.trim()).ok_or_else(|| format!("invalid ref for branch {branch}"))?;
    if hash.is_zero() {
        return Err(format!("zero ref for branch {branch}"));
    }
    Ok(Some(CommitHash(hash)))
}

fn find_object<'a>(
    catalog: &'a Catalog,
    hash: &str,
) -> Result<(ds::model::SymbolHash, &'a DesignObject), String> {
    let expected = Hash32::from_hex(hash).ok_or_else(|| "invalid element hash".to_string())?;
    for (qname, object) in catalog.iter() {
        if ds::format::object_hash(object).map_err(display)?.0 == expected {
            return Ok((qname, object));
        }
    }
    Err("design-space object not found".to_string())
}

fn element_info(
    store: &Store,
    qname: ds::model::SymbolHash,
    object: &DesignObject,
) -> Result<DesignSpaceElement, String> {
    let qname_text = store.get_symbol(qname).map_err(display)?;
    let path = source_path(store, object)?.unwrap_or_else(|| qname_text.clone());
    let (start_line, end_line) = source_span(object);
    Ok(DesignSpaceElement {
        hash: ds::format::object_hash(object).map_err(display)?.to_hex(),
        name: local_name(&qname_text),
        lang: language_for_path(&path),
        kind: kind_label(object.kind).to_owned(),
        path,
        start_line,
        end_line,
        has_design: design_doc(object).is_some_and(|doc| !doc.is_zero()),
        is_file: object.kind == Kind::File,
    })
}

fn source_path(store: &Store, object: &DesignObject) -> Result<Option<String>, String> {
    let path = match &object.payload {
        ObjectPayload::Directory(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::File(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Namespace(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Class(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Interface(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Method(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Function(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Part(p) => Some(store.get_symbol(p.source.path)),
        ObjectPayload::Mixin(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::View(_) => None,
    };
    path.transpose().map_err(display)
}

fn source_span(object: &DesignObject) -> (u32, u32) {
    let span = match &object.payload {
        ObjectPayload::Namespace(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Class(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Interface(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Method(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Function(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Part(p) => Some(p.source.span),
        ObjectPayload::Mixin(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Directory(_) | ObjectPayload::File(_) | ObjectPayload::View(_) => None,
    };
    span.map(|span| (span.start_line, span.end_line))
        .unwrap_or((0, 0))
}

fn design_doc(object: &DesignObject) -> Option<DocHash> {
    match &object.payload {
        ObjectPayload::View(_) => None,
        ObjectPayload::Directory(p) => Some(p.design_doc),
        ObjectPayload::File(p) => Some(p.design_doc),
        ObjectPayload::Namespace(p) => Some(p.design_doc),
        ObjectPayload::Class(p) => Some(p.design_doc),
        ObjectPayload::Interface(p) => Some(p.design_doc),
        ObjectPayload::Method(p) => Some(p.design_doc),
        ObjectPayload::Function(p) => Some(p.design_doc),
        ObjectPayload::Part(p) => Some(p.design_doc),
        ObjectPayload::Mixin(p) => Some(p.design_doc),
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::View => "view",
        Kind::Directory => "directory",
        Kind::File => "file",
        Kind::Namespace => "namespace",
        Kind::Class => "class",
        Kind::Interface => "interface",
        Kind::Method => "method",
        Kind::Function => "function",
        Kind::Part => "part",
        Kind::Mixin => "mixin",
    }
}

fn local_name(qname: &str) -> String {
    qname.rsplit('/').next().unwrap_or(qname).to_owned()
}

fn language_for_path(path: &str) -> String {
    let extension = path
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, extension)| extension)
        .unwrap_or_default();
    match extension {
        "rs" => "rust",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "h" => "c",
        "py" => "python",
        "ts" => "typescript",
        "tsx" => "tsx",
        "js" => "javascript",
        "jsx" => "jsx",
        "go" => "go",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "cs" => "csharp",
        "php" => "php",
        "swift" => "swift",
        "scala" => "scala",
        "m" | "mm" => "objc",
        _ => "",
    }
    .to_owned()
}

fn repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let path = if let Some(rest) = url.strip_prefix("ds://") {
        rest.split_once('/').map(|(_, path)| path)?
    } else if let Some(index) = url.find("://") {
        let rest = &url[index + 3..];
        rest.split_once('/').map(|(_, path)| path)?
    } else {
        url.trim_end_matches('/')
    };
    if path.split('/').any(|part| part == ".." || part == ".") {
        return None;
    }
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let name = percent_decode(last);
    valid_repo_name(&name).then_some(name)
}

fn valid_repo_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn git_oid_hex(oid: GitOid) -> String {
    if oid.is_zero() {
        return String::new();
    }
    let full = oid.to_hex();
    if oid.0[20..].iter().all(|byte| *byte == 0) {
        full[..40].to_owned()
    } else {
        full
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn short_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    to_hex_hash(&hasher.finalize())[..12].to_string()
}

fn to_hex_hash(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn git(cwd: Option<&Path>, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = Command::new("git");
    if let Some(cwd) = cwd {
        command.arg("-C").arg(cwd);
    }
    command
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes");
    }
    command
        .output()
        .map_err(|error| format!("无法运行 git: {error}"))
}

fn git_error(message: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        message.to_string()
    } else {
        format!("{message}: {detail}")
    }
}

fn has_commit(dir: &Path, commit: &str) -> bool {
    let spec = format!("{commit}^{{commit}}");
    git(Some(dir), &["cat-file", "-e", &spec])
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn read_blob(dir: &Path, commit: &str, path: &str) -> Result<String, String> {
    let spec = format!("{commit}:{path}");
    let output = git(Some(dir), &["cat-file", "blob", &spec])?;
    if !output.status.success() {
        return Err(git_error(&format!("无法读取 {} 的源码", path), &output));
    }
    if output.stdout.contains(&0) {
        return Err("该文件是二进制，无法显示源码".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn source_lines(
    text: &str,
    commit: &str,
    path: &str,
    start_line: u32,
    end_line: u32,
) -> Result<DesignSpaceSource, String> {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len() as u32;
    if total == 0 {
        return Err("源码文件为空".to_string());
    }
    let (highlight_start, highlight_end) = if end_line == 0 {
        (0, 0)
    } else {
        (
            start_line.max(1).min(total),
            end_line.max(start_line).min(total),
        )
    };
    let first = if highlight_start == 0 {
        1
    } else {
        highlight_start.saturating_sub(SOURCE_CONTEXT).max(1)
    };
    let mut last = if highlight_end == 0 {
        total
    } else {
        highlight_end.saturating_add(SOURCE_CONTEXT).min(total)
    };
    if last < first {
        last = first;
    }
    if last - first + 1 > SOURCE_MAX_LINES {
        last = first + SOURCE_MAX_LINES - 1;
    }

    let mut out = Vec::with_capacity((last - first + 1) as usize);
    for number in first..=last {
        let text = lines[(number - 1) as usize];
        out.push(DesignSpaceLine {
            number,
            text: text.to_owned(),
            in_element: highlight_start != 0
                && number >= highlight_start
                && number <= highlight_end,
        });
    }
    Ok(DesignSpaceSource {
        commit: commit.to_owned(),
        path: path.to_owned(),
        start_line: highlight_start,
        end_line: highlight_end,
        first_line: first,
        last_line: last,
        total_lines: total,
        lines: out,
    })
}

fn display(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn short(value: &str) -> &str {
    &value[..value.len().min(12)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use design_space::model::{DirectoryPayload, NamespacePayload, ParentRef};
    use design_space::{Catalog, Commit, DesignObject, ObjectPayload};
    use tempfile::tempdir;

    #[test]
    fn reads_repository_names_from_addresses() {
        assert_eq!(
            repo_name("ds://xuyan@localhost/f5acb3bbde1a05e1").as_deref(),
            Some("f5acb3bbde1a05e1")
        );
        assert_eq!(
            repo_name("http://127.0.0.1:7700/alpha").as_deref(),
            Some("alpha")
        );
        assert_eq!(repo_name("http://host/").as_deref(), None);
        assert_eq!(repo_name("../escape").as_deref(), None);
        assert_eq!(percent_decode("%E8%BD%AF%E4%BB%B6"), "软件");
    }

    #[test]
    fn labels_languages_and_kinds() {
        assert_eq!(language_for_path("src/lib.rs"), "rust");
        assert_eq!(language_for_path("src/main.cpp"), "cpp");
        assert_eq!(language_for_path("README"), "");
        assert_eq!(kind_label(Kind::Function), "function");
        assert_eq!(kind_label(Kind::File), "file");
        assert_eq!(local_name("namespaces/app/Auth"), "Auth");
        assert_eq!(git_oid_hex(GitOid::ZERO), "");
    }

    #[tokio::test]
    async fn reads_overview_from_a_created_repository() {
        let dir = tempdir().unwrap();
        let repo = dir.path().join("repos/alpha");
        let store = Store::open(&repo).unwrap();
        let files = store.put_symbol("files").unwrap();
        let namespaces = store.put_symbol("namespaces").unwrap();
        let mut catalog = Catalog::new(GitOid::ZERO);
        catalog
            .insert(
                files,
                DesignObject::new(
                    Kind::Directory,
                    ObjectPayload::Directory(DirectoryPayload {
                        qualified_name: files,
                        parent: ParentRef::View,
                        source: None,
                        design_doc: DocHash::ZERO,
                        children: vec![],
                    }),
                ),
            )
            .unwrap();
        catalog
            .insert(
                namespaces,
                DesignObject::new(
                    Kind::Namespace,
                    ObjectPayload::Namespace(NamespacePayload {
                        qualified_name: namespaces,
                        parent: ParentRef::View,
                        source: None,
                        content_hash: Hash32::ZERO,
                        design_doc: DocHash::ZERO,
                        children: vec![],
                    }),
                ),
            )
            .unwrap();
        let view = catalog.rebuild(&store).unwrap();
        let commit = Commit {
            view,
            parent: CommitHash::ZERO,
            message: "initialize".to_owned(),
            author: "test".to_owned(),
            timestamp: 1,
        };
        let commit_hash = store.put_commit(&commit).unwrap();
        std::fs::create_dir_all(repo.join("refs/heads")).unwrap();
        std::fs::write(
            repo.join("refs/heads/main"),
            format!("{}\n", commit_hash.to_hex()),
        )
        .unwrap();

        let owner = uuid::Uuid::now_v7();
        let mut project =
            Project::new("Alpha", None, "git@example.com:test/repo.git", owner).unwrap();
        project.design_space_url = "http://127.0.0.1:7700/alpha".to_owned();
        project.design_space_branch = "main".to_owned();

        let access = DesignSpaceAccess::new(dir.path().join("repos"));
        let overview = access.overview(&project).await.unwrap();
        assert_eq!(overview.branch, "main");
        assert_eq!(
            overview.version.as_deref(),
            Some(commit_hash.to_hex().as_str())
        );
        assert_eq!(overview.element_count, 2);
        assert_eq!(overview.elements.len(), 2);
    }
}
