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
    CommitHash, DesignObject, DocComment, GitOid, Hash32, Kind, ObjectPayload, SymbolHash,
    ViewPayload,
};
use ds::store::Store;
use foundry_app::{DesignSpaceGateway, DesignSpaceGatewayError};
use foundry_domain::{MergeRequestChange, MergeRequestChangeStatus, Project};
use foundry_web::{
    DesignSpaceComment, DesignSpaceDetail, DesignSpaceElement, DesignSpaceLine,
    DesignSpaceMember, DesignSpaceOverview, DesignSpaceSource,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

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

    pub async fn branches(&self, project: &Project) -> Result<Vec<String>, String> {
        let access = self.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || access.branches_blocking(&project))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn overview(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<DesignSpaceOverview, String> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.overview_blocking(&project, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn element(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceDetail, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.element_blocking(&project, &hash, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn source(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceSource, String> {
        let access = self.clone();
        let project = project.clone();
        let hash = hash.to_owned();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.source_blocking(&project, &hash, &branch))
            .await
            .map_err(|error| error.to_string())?
    }

    fn overview_blocking(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<DesignSpaceOverview, String> {
        let mut overview = DesignSpaceOverview {
            branch: branch.to_owned(),
            version: None,
            source_commit: String::new(),
            file_count: 0,
            element_count: 0,
            document_count: 0,
            designed_count: 0,
            elements: Vec::new(),
        };
        let Some(loaded) = self.load_branch(project, branch)? else {
            return Ok(overview);
        };
        overview.version = Some(loaded.commit_hash.to_hex());
        overview.source_commit = git_oid_hex(loaded.view_payload.source_commit);

        for (qname, object) in loaded.catalog.iter() {
            let qname_text = loaded.store.get_symbol(qname).map_err(display)?;
            if qname_text == "files" || qname_text == "namespaces" || qname_text == "documents" {
                continue;
            }
            if is_document_kind(object.kind)
                && object.kind != Kind::Document
                && object.kind != Kind::DocumentDirectory
            {
                continue;
            }
            let element = element_info(&loaded.store, qname, object)?;
            if object.kind == Kind::Document {
                overview.document_count += 1;
            } else if is_document_kind(object.kind) {
                // Document directories are folders, not design elements.
            } else if element.is_file {
                overview.file_count += 1;
            } else if element.is_structure {
                // Systems and modules are counted by the structure tree.
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

    fn element_blocking(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceDetail, String> {
        let loaded = self
            .load_branch(project, branch)?
            .ok_or_else(|| "设计空间分支还没有提交".to_string())?;
        let (qname, object) = find_object(&loaded.catalog, hash)?;
        let info = element_info(&loaded.store, qname, object)?;
        let (design, comments) = match document_target(object) {
            Some(target) => {
                let (design, comments) =
                    render_document(&loaded.store, &loaded.catalog, target)?;
                (Some(design), comments)
            }
            None if is_document_kind(object.kind) => {
                let (design, comments) =
                    render_document(&loaded.store, &loaded.catalog, qname)?;
                (Some(design), comments)
            }
            None => (None, Vec::new()),
        };
        Ok(DesignSpaceDetail {
            hash: info.hash,
            name: info.name,
            lang: info.lang,
            kind: info.kind,
            path: info.path,
            start_line: info.start_line,
            end_line: info.end_line,
            design,
            comments,
            members: structure_member_infos(&loaded.store, &loaded.catalog, qname, object)?,
        })
    }

    fn source_blocking(
        &self,
        project: &Project,
        hash: &str,
        branch: &str,
    ) -> Result<DesignSpaceSource, String> {
        let loaded = self
            .load_branch(project, branch)?
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

    fn branches_blocking(&self, project: &Project) -> Result<Vec<String>, String> {
        let path = self.repo_path(project)?;
        let dir = path.join("refs/heads");
        let mut branches = Vec::new();
        collect_branches(&dir, &dir, &mut branches)?;
        branches.sort();
        Ok(branches)
    }

    fn branch_head_blocking(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, String> {
        let path = self.repo_path(project)?;
        Ok(read_branch_commit(&path, branch)?.map(CommitHash::to_hex))
    }

    fn is_ancestor_blocking(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, String> {
        let path = self.repo_path(project)?;
        let store = Store::open(&path).map_err(display)?;
        let ancestor = commit_hash(ancestor)?;
        let mut current = commit_hash(descendant)?;
        let mut steps = 0u32;
        while !current.is_zero() {
            if current == ancestor {
                return Ok(true);
            }
            if steps > 1_000_000 {
                return Err("commit history is too deep".to_string());
            }
            current = store.get_commit(current).map_err(display)?.parent;
            steps += 1;
        }
        Ok(false)
    }

    fn diff_blocking(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<MergeRequestChange>, String> {
        let path = self.repo_path(project)?;
        let store = Store::open(&path).map_err(display)?;
        let from = load_commit_catalog(&store, from_commit)?;
        let to = load_commit_catalog(&store, to_commit)?;
        let mut names = BTreeSet::new();
        names.extend(from.catalog.iter().map(|(name, _)| name));
        names.extend(to.catalog.iter().map(|(name, _)| name));
        let mut changes = Vec::with_capacity(names.len());
        for qname in names {
            let qname_text = store.get_symbol(qname).map_err(display)?;
            match (from.catalog.get(qname), to.catalog.get(qname)) {
                (None, Some(object)) => changes.push(MergeRequestChange {
                    status: MergeRequestChangeStatus::Added,
                    qname: qname_text,
                    kind: kind_label(object.kind).to_string(),
                }),
                (Some(object), None) => changes.push(MergeRequestChange {
                    status: MergeRequestChangeStatus::Removed,
                    qname: qname_text,
                    kind: kind_label(object.kind).to_string(),
                }),
                (Some(before), Some(after)) if before != after => {
                    changes.push(MergeRequestChange {
                        status: MergeRequestChangeStatus::Modified,
                        qname: qname_text,
                        kind: kind_label(after.kind).to_string(),
                    });
                }
                _ => {}
            }
        }
        changes.sort_by(|left, right| left.qname.cmp(&right.qname));
        Ok(changes)
    }

    fn comment_count_blocking(&self, project: &Project, commit: &str) -> Result<usize, String> {
        let path = self.repo_path(project)?;
        let store = Store::open(&path).map_err(display)?;
        let loaded = load_commit_catalog(&store, commit)?;
        let count = loaded
            .catalog
            .iter()
            .map(|(_, object)| document_comments(object).len())
            .sum();
        Ok(count)
    }

    fn content_digest_blocking(&self, project: &Project, commit: &str) -> Result<String, String> {
        let path = self.repo_path(project)?;
        let store = Store::open(&path).map_err(display)?;
        let loaded = load_commit_catalog(&store, commit)?;
        loaded
            .catalog
            .content_digest()
            .map(|digest| digest.to_hex())
            .map_err(display)
    }

    fn delete_branch_blocking(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let path = self
            .repo_path(project)
            .map_err(DesignSpaceGatewayError::Backend)?;
        if branch.is_empty()
            || branch.starts_with('/')
            || branch.ends_with('/')
            || branch
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(DesignSpaceGatewayError::Conflict(format!(
                "invalid branch: {branch}"
            )));
        }
        let file = path.join("refs/heads").join(branch);
        match fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(DesignSpaceGatewayError::Backend(error.to_string())),
        }
    }

    fn fast_forward_blocking(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let _guard = self
            .git_lock
            .lock()
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        let path = self
            .repo_path(project)
            .map_err(DesignSpaceGatewayError::Backend)?;
        let current = read_branch_commit(&path, target_branch)
            .map_err(DesignSpaceGatewayError::Backend)?
            .ok_or_else(|| {
                DesignSpaceGatewayError::NotFound(format!("分支不存在: {target_branch}"))
            })?;
        if current.to_hex() == source_commit {
            // A previous attempt already moved the ref but failed before the
            // merge-request row was updated. Treat the retry as successful.
            return Ok(());
        }
        if current.to_hex() != expected_target {
            return Err(DesignSpaceGatewayError::Conflict(
                "目标分支在合并请求创建后已经前进，请重新创建合并请求".to_string(),
            ));
        }
        let target = commit_hash(expected_target).map_err(DesignSpaceGatewayError::Backend)?;
        let source = commit_hash(source_commit).map_err(DesignSpaceGatewayError::Backend)?;
        if source == target {
            return Err(DesignSpaceGatewayError::Conflict(
                "源分支没有新的提交".to_string(),
            ));
        }
        let store = Store::open(&path)
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?;
        store
            .get_commit(source)
            .map_err(|error| DesignSpaceGatewayError::NotFound(format!("源提交不存在: {error}")))?;
        store.get_commit(target).map_err(|error| {
            DesignSpaceGatewayError::NotFound(format!("目标提交不存在: {error}"))
        })?;
        let mut cursor = source;
        let mut ancestor = false;
        let mut steps = 0u32;
        while !cursor.is_zero() {
            if cursor == target {
                ancestor = true;
                break;
            }
            if steps > 1_000_000 {
                return Err(DesignSpaceGatewayError::Conflict(
                    "提交历史过深".to_string(),
                ));
            }
            cursor = store
                .get_commit(cursor)
                .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
                .parent;
            steps += 1;
        }
        if !ancestor {
            return Err(DesignSpaceGatewayError::Conflict(
                "目标分支不是源分支的祖先，无法快进合并".to_string(),
            ));
        }
        write_branch_ref(&path, target_branch, source).map_err(DesignSpaceGatewayError::Backend)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl DesignSpaceGateway for DesignSpaceAccess {
    async fn branches(&self, project: &Project) -> Result<Vec<String>, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        tokio::task::spawn_blocking(move || access.branches_blocking(&project))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn branch_head(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<Option<String>, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.branch_head_blocking(&project, &branch))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn is_ancestor(
        &self,
        project: &Project,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let ancestor = ancestor.to_owned();
        let descendant = descendant.to_owned();
        tokio::task::spawn_blocking(move || {
            access.is_ancestor_blocking(&project, &ancestor, &descendant)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
        .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn diff(
        &self,
        project: &Project,
        from_commit: &str,
        to_commit: &str,
    ) -> Result<Vec<MergeRequestChange>, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let from_commit = from_commit.to_owned();
        let to_commit = to_commit.to_owned();
        tokio::task::spawn_blocking(move || {
            access.diff_blocking(&project, &from_commit, &to_commit)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
        .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn comment_count(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<usize, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let commit = commit.to_owned();
        tokio::task::spawn_blocking(move || access.comment_count_blocking(&project, &commit))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn content_digest(
        &self,
        project: &Project,
        commit: &str,
    ) -> Result<String, DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let commit = commit.to_owned();
        tokio::task::spawn_blocking(move || access.content_digest_blocking(&project, &commit))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
            .map_err(DesignSpaceGatewayError::Backend)
    }

    async fn delete_branch(
        &self,
        project: &Project,
        branch: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let branch = branch.to_owned();
        tokio::task::spawn_blocking(move || access.delete_branch_blocking(&project, &branch))
            .await
            .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }

    async fn fast_forward(
        &self,
        project: &Project,
        target_branch: &str,
        expected_target: &str,
        source_commit: &str,
    ) -> Result<(), DesignSpaceGatewayError> {
        let access = self.clone();
        let project = project.clone();
        let target_branch = target_branch.to_owned();
        let expected_target = expected_target.to_owned();
        let source_commit = source_commit.to_owned();
        tokio::task::spawn_blocking(move || {
            access.fast_forward_blocking(&project, &target_branch, &expected_target, &source_commit)
        })
        .await
        .map_err(|error| DesignSpaceGatewayError::Backend(error.to_string()))?
    }
}

struct CommitCatalog {
    catalog: Catalog,
}

fn load_commit_catalog(store: &Store, commit: &str) -> Result<CommitCatalog, String> {
    let hash = commit_hash(commit)?;
    let commit = store.get_commit(hash).map_err(display)?;
    let object = store.get_object(commit.view).map_err(display)?;
    if object.kind != Kind::View {
        return Err("提交的 View 不是 view 对象".to_string());
    }
    Ok(CommitCatalog {
        catalog: Catalog::load(store, commit.view).map_err(display)?,
    })
}

fn commit_hash(text: &str) -> Result<CommitHash, String> {
    Hash32::from_hex(text)
        .map(CommitHash)
        .ok_or_else(|| format!("invalid commit hash: {text}"))
}

fn collect_branches(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_branches(root, &path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if Hash32::from_hex(text.trim()).is_some() {
            out.push(relative);
        }
    }
    Ok(())
}

fn write_branch_ref(root: &Path, branch: &str, commit: CommitHash) -> Result<(), String> {
    if branch.is_empty()
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("invalid branch: {branch}"));
    }
    let path = root.join("refs/heads").join(branch);
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid branch path: {branch}"))?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    fs::create_dir_all(root.join("tmp")).map_err(|error| error.to_string())?;
    let temp = root.join("tmp").join(format!(
        "ref-{}-{}.tmp",
        std::process::id(),
        short(&commit.to_hex())
    ));
    fs::write(&temp, format!("{}\n", commit.to_hex())).map_err(|error| error.to_string())?;
    fs::rename(&temp, &path).map_err(|error| error.to_string())?;
    Ok(())
}

struct Loaded {
    store: Store,
    commit_hash: CommitHash,
    view_payload: ViewPayload,
    catalog: Catalog,
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

fn structure_member_infos(
    store: &Store,
    catalog: &Catalog,
    qname: SymbolHash,
    object: &DesignObject,
) -> Result<Vec<DesignSpaceMember>, String> {
    if !matches!(
        object.kind,
        Kind::DesignStructure | Kind::System | Kind::Module
    ) {
        return Ok(Vec::new());
    }
    let mut members = Vec::new();
    if let ObjectPayload::Module(module) = &object.payload {
        members.extend(module.members.iter().copied());
    }
    members.extend(catalog.children(qname));
    members.sort();
    members.dedup();
    let mut out = Vec::with_capacity(members.len());
    for member in members {
        let Some(target) = catalog.get(member) else {
            continue;
        };
        let qname = store.get_symbol(member).map_err(display)?;
        out.push(DesignSpaceMember {
            hash: ds::format::object_hash(target).map_err(display)?.to_hex(),
            name: structure_title(target).unwrap_or_else(|| local_name(&qname)),
            qname,
            kind: kind_label(target.kind).to_owned(),
        });
    }
    out.sort_by(|left, right| left.qname.cmp(&right.qname));
    Ok(out)
}

fn element_info(
    store: &Store,
    qname: ds::model::SymbolHash,
    object: &DesignObject,
) -> Result<DesignSpaceElement, String> {
    let qname_text = store.get_symbol(qname).map_err(display)?;
    let is_document = is_document_kind(object.kind);
    let path = if is_document {
        qname_text.clone()
    } else {
        source_path(store, object)?.unwrap_or_else(|| qname_text.clone())
    };
    let (start_line, end_line) = source_span(object);
    Ok(DesignSpaceElement {
        hash: ds::format::object_hash(object).map_err(display)?.to_hex(),
        qname: qname_text.clone(),
        name: if is_document {
            document_name(object, &qname_text)
        } else if let Some(title) = structure_title(object) {
            title
        } else {
            local_name(&qname_text)
        },
        lang: if is_document || is_structure_kind(object.kind) {
            String::new()
        } else {
            language_for_path(&path)
        },
        kind: kind_label(object.kind).to_owned(),
        path,
        start_line,
        end_line,
        has_design: is_document || document_target(object).is_some(),
        is_file: object.kind == Kind::File,
        is_structure: matches!(
            object.kind,
            Kind::DesignStructure | Kind::System | Kind::Module
        ),
        members: module_members(store, object)?,
    })
}

fn structure_title(object: &DesignObject) -> Option<String> {
    match &object.payload {
        ObjectPayload::DesignStructure(p) => Some(p.title.clone()),
        ObjectPayload::System(p) => Some(p.title.clone()),
        ObjectPayload::Module(p) => Some(p.title.clone()),
        _ => None,
    }
}

fn is_structure_kind(kind: Kind) -> bool {
    matches!(kind, Kind::DesignStructure | Kind::System | Kind::Module)
}

fn module_members(store: &Store, object: &DesignObject) -> Result<Vec<String>, String> {
    let ObjectPayload::Module(module) = &object.payload else {
        return Ok(Vec::new());
    };
    module
        .members
        .iter()
        .map(|member| store.get_symbol(*member).map_err(display))
        .collect()
}

fn is_document_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::DocumentDirectory
            | Kind::Document
            | Kind::DocumentSection
            | Kind::DocumentList
            | Kind::DocumentText
            | Kind::DocumentTopic
    )
}

fn document_name(object: &DesignObject, qname: &str) -> String {
    match &object.payload {
        ObjectPayload::Document(p) => p.title.clone(),
        ObjectPayload::DocumentSection(p) => p.title.clone(),
        ObjectPayload::DocumentTopic(p) => p.title.clone(),
        ObjectPayload::DocumentList(_) => "列表".to_owned(),
        ObjectPayload::DocumentText(p) => {
            let mut text = p.text.chars().take(40).collect::<String>();
            if p.text.chars().count() > 40 {
                text.push('…');
            }
            if text.is_empty() {
                "文本".to_owned()
            } else {
                text
            }
        }
        ObjectPayload::DocumentDirectory(_) => local_name(qname),
        _ => local_name(qname),
    }
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
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => p
            .source
            .as_ref()
            .map(|source| store.get_symbol(source.path)),
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => p
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
        ObjectPayload::View(_)
        | ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::DesignStructure(_)
        | ObjectPayload::System(_)
        | ObjectPayload::Module(_) => None,
    };
    path.transpose().map_err(display)
}

fn source_span(object: &DesignObject) -> (u32, u32) {
    let span = match &object.payload {
        ObjectPayload::Namespace(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => {
            p.source.as_ref().map(|source| source.span)
        }
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => {
            p.source.as_ref().map(|source| source.span)
        }
        ObjectPayload::Method(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Function(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::Part(p) => Some(p.source.span),
        ObjectPayload::Mixin(p) => p.source.as_ref().map(|source| source.span),
        ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::DocumentTopic(_)
        | ObjectPayload::DesignStructure(_)
        | ObjectPayload::System(_)
        | ObjectPayload::Module(_)
        | ObjectPayload::Directory(_)
        | ObjectPayload::File(_)
        | ObjectPayload::View(_) => None,
    };
    span.map(|span| (span.start_line, span.end_line))
        .unwrap_or((0, 0))
}

fn document_target(object: &DesignObject) -> Option<SymbolHash> {
    let target = match &object.payload {
        ObjectPayload::View(_) => return None,
        ObjectPayload::Directory(p) => p.document,
        ObjectPayload::File(p) => p.document,
        ObjectPayload::Namespace(p) => p.document,
        ObjectPayload::Class(p) | ObjectPayload::InnerClass(p) => p.document,
        ObjectPayload::Interface(p) | ObjectPayload::InnerInterface(p) => p.document,
        ObjectPayload::Method(p) => p.document,
        ObjectPayload::Function(p) => p.document,
        ObjectPayload::Mixin(p) => p.document,
        ObjectPayload::DesignStructure(p) => p.document,
        ObjectPayload::System(p) => p.document,
        ObjectPayload::Module(p) => p.document,
        ObjectPayload::Part(_)
        | ObjectPayload::DocumentDirectory(_)
        | ObjectPayload::Document(_)
        | ObjectPayload::DocumentSection(_)
        | ObjectPayload::DocumentList(_)
        | ObjectPayload::DocumentText(_)
        | ObjectPayload::DocumentTopic(_) => return None,
    };
    (!target.is_zero()).then_some(target)
}

fn render_document(
    store: &Store,
    catalog: &Catalog,
    qname: SymbolHash,
) -> Result<(String, Vec<DesignSpaceComment>), String> {
    let mut out = String::new();
    let mut comments = Vec::new();
    render_doc(store, catalog, qname, 0, &mut out, &mut comments)?;
    Ok((out, comments))
}

fn render_doc(
    store: &Store,
    catalog: &Catalog,
    qname: SymbolHash,
    depth: usize,
    out: &mut String,
    comments: &mut Vec<DesignSpaceComment>,
) -> Result<(), String> {
    let object = catalog
        .get(qname)
        .ok_or_else(|| format!("document fragment not found: {}", qname.to_hex()))?;
    let marker = comment_marker(object);
    match &object.payload {
        ObjectPayload::DocumentDirectory(_) => {
            if !marker.is_empty() {
                out.push_str(&marker);
                out.push_str("\n\n");
            }
            let mut children = catalog.children(qname);
            children.sort_by(|left, right| {
                store
                    .get_symbol(*left)
                    .unwrap_or_default()
                    .cmp(&store.get_symbol(*right).unwrap_or_default())
            });
            for child in children {
                render_doc(store, catalog, child, depth, out, comments)?;
            }
        }
        ObjectPayload::Document(p) => {
            out.push_str(&format!("{}{}\n\n", p.title, marker));
            for section in &p.sections {
                render_doc(store, catalog, *section, depth + 1, out, comments)?;
            }
        }
        ObjectPayload::DocumentSection(p) => {
            out.push_str(&format!(
                "{} {}{}\n\n",
                "#".repeat((depth + 1).min(6)),
                p.title,
                marker
            ));
            for paragraph in &p.body {
                render_doc(store, catalog, *paragraph, depth + 1, out, comments)?;
            }
            for section in &p.sections {
                render_doc(store, catalog, *section, depth + 1, out, comments)?;
            }
        }
        ObjectPayload::DocumentList(p) => {
            let last = p.items.len().saturating_sub(1);
            for (position, item) in p.items.iter().enumerate() {
                let item_object = catalog
                    .get(*item)
                    .ok_or_else(|| format!("document item not found: {}", item.to_hex()))?;
                if let ObjectPayload::DocumentText(text) = &item_object.payload {
                    let trailing = if position == last { marker.as_str() } else { "" };
                    out.push_str(&format!("- {}{}\n", text.text, trailing));
                } else {
                    render_doc(store, catalog, *item, depth + 1, out, comments)?;
                    if position == last && !marker.is_empty() {
                        out.push_str(&marker);
                        out.push('\n');
                    }
                }
            }
            if p.items.is_empty() && !marker.is_empty() {
                out.push_str(&marker);
                out.push('\n');
            }
            out.push('\n');
        }
        ObjectPayload::DocumentText(p) => {
            out.push_str(&format!("{}{}\n\n", p.text, marker));
        }
        ObjectPayload::DocumentTopic(p) => {
            out.push_str(&format!("**{}**{}\n\n", p.title, marker));
            for paragraph in &p.paragraphs {
                render_doc(store, catalog, *paragraph, depth + 1, out, comments)?;
            }
        }
        _ => return Err("document reference does not point at a document fragment".to_owned()),
    }
    let target = store.get_symbol(qname).map_err(display)?;
    for comment in document_comments(object) {
        comments.push(DesignSpaceComment {
            id: comment.id,
            resolved: comment.resolved,
            target: target.clone(),
            text: comment.text.clone(),
        });
    }
    Ok(())
}

/// A marker rendered by the web UI as a collapsed comment button. Comment
/// bodies stay out of the document flow and open in a popover on click.
fn comment_marker(object: &DesignObject) -> String {
    let ids = document_comments(object)
        .iter()
        .map(|comment| comment.id.to_string())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        String::new()
    } else {
        format!("{{{{comments:{}}}}}", ids.join(","))
    }
}

fn document_comments(object: &DesignObject) -> &[DocComment] {
    match &object.payload {
        ObjectPayload::DocumentDirectory(p) => &p.comments,
        ObjectPayload::Document(p) => &p.comments,
        ObjectPayload::DocumentSection(p) => &p.comments,
        ObjectPayload::DocumentList(p) => &p.comments,
        ObjectPayload::DocumentText(p) => &p.comments,
        ObjectPayload::DocumentTopic(p) => &p.comments,
        _ => &[],
    }
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::View => "view",
        Kind::Directory => "directory",
        Kind::File => "file",
        Kind::Namespace => "namespace",
        Kind::Class => "class",
        Kind::InnerClass => "inner-class",
        Kind::Interface => "interface",
        Kind::InnerInterface => "inner-interface",
        Kind::Method => "method",
        Kind::Function => "function",
        Kind::Part => "part",
        Kind::Mixin => "mixin",
        Kind::DocumentDirectory => "document-directory",
        Kind::Document => "document",
        Kind::DocumentSection => "document-section",
        Kind::DocumentList => "document-list",
        Kind::DocumentText => "document-text",
        Kind::DocumentTopic => "document-topic",
        Kind::DesignStructure => "design-structure",
        Kind::System => "system",
        Kind::Module => "module",
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
    use design_space::model::{
        DirectoryPayload, DocumentDirectoryPayload, DocumentPayload, DocumentSectionPayload,
        DocumentTextPayload, NamespacePayload, ParentRef, SymbolHash,
    };
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
        let documents = store.put_symbol("documents").unwrap();
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
                        document: SymbolHash::ZERO,
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
                        document: SymbolHash::ZERO,
                        children: vec![],
                    }),
                ),
            )
            .unwrap();
        catalog
            .insert(
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
            )
            .unwrap();
        let design_dir = store.put_symbol("documents/design").unwrap();
        let technical = store.put_symbol("documents/design/technical").unwrap();
        let section = store
            .put_symbol("documents/design/technical/section1")
            .unwrap();
        let text = store
            .put_symbol("documents/design/technical/section1/body1")
            .unwrap();
        catalog
            .insert(
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
            )
            .unwrap();
        catalog
            .insert(
                technical,
                DesignObject::new(
                    Kind::Document,
                    ObjectPayload::Document(DocumentPayload {
                        qualified_name: technical,
                        parent: design_dir,
                        title: "技术设计".to_owned(),
                        sections: vec![section],
                        children: vec![],
                        comments: vec![],
                    }),
                ),
            )
            .unwrap();
        catalog
            .insert(
                section,
                DesignObject::new(
                    Kind::DocumentSection,
                    ObjectPayload::DocumentSection(DocumentSectionPayload {
                        qualified_name: section,
                        parent: technical,
                        title: "登录".to_owned(),
                        body: vec![text],
                        sections: vec![],
                        children: vec![],
                        comments: vec![DocComment {
                            id: 7,
                            text: "需要补充失败语义".to_owned(),
                            resolved: false,
                        }],
                    }),
                ),
            )
            .unwrap();
        catalog
            .insert(
                text,
                DesignObject::new(
                    Kind::DocumentText,
                    ObjectPayload::DocumentText(DocumentTextPayload {
                        qualified_name: text,
                        parent: section,
                        text: "登录使用 OAuth。".to_owned(),
                        comments: vec![],
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
            repo.join("refs/heads/source"),
            format!("{}\n", commit_hash.to_hex()),
        )
        .unwrap();

        let owner = uuid::Uuid::now_v7();
        let mut project =
            Project::new("Alpha", None, "git@example.com:test/repo.git", owner).unwrap();
        project.design_space_url = "http://127.0.0.1:7700/alpha".to_owned();
        project.design_space_branch = "source".to_owned();

        let access = DesignSpaceAccess::new(dir.path().join("repos"));
        let overview = access.overview(&project, "source").await.unwrap();
        assert_eq!(overview.branch, "source");
        assert_eq!(
            overview.version.as_deref(),
            Some(commit_hash.to_hex().as_str())
        );
        // `files` and `namespaces` are structural roots, not elements.
        assert_eq!(overview.element_count, 0);
        assert_eq!(overview.document_count, 1);
        let document = overview
            .elements
            .iter()
            .find(|element| element.qname == "documents/design/technical")
            .expect("document element");
        assert_eq!(document.kind, "document");
        assert_eq!(document.name, "技术设计");
        assert!(document.has_design);
        assert!(
            !overview
                .elements
                .iter()
                .any(|element| element.qname.contains("/section1"))
        );
        let detail = access
            .element(&project, &document.hash, "source")
            .await
            .unwrap();
        assert!(
            detail
                .design
                .as_deref()
                .unwrap()
                .contains("登录使用 OAuth。")
        );
        assert!(
            detail
                .design
                .as_deref()
                .unwrap()
                .contains("## 登录{{comments:7}}")
        );
        assert_eq!(detail.comments.len(), 1);
        assert_eq!(detail.comments[0].id, 7);
    }
}

#[cfg(test)]
mod merge_gateway_tests {
    use super::*;
    use design_space::model::{Commit, DirectoryPayload, ParentRef, SymbolHash};
    use design_space::server::RepoCollection;
    use design_space::{Catalog, DesignObject, ObjectPayload};
    use tempfile::tempdir;

    fn project(owner: uuid::Uuid) -> Project {
        let mut project = Project::new(
            "Merge gateway",
            None,
            "git@example.com:test/repo.git",
            owner,
        )
        .unwrap();
        project.design_space_url = "http://example.test/alpha".to_string();
        project.design_space_branch = "source".to_string();
        project
    }

    #[tokio::test]
    async fn fast_forwards_only_an_ancestor_and_reports_object_diff() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repos");
        let collection = RepoCollection::open(&root).unwrap();
        collection.create("alpha", "main").unwrap();
        let repo = root.join("alpha");
        let store = Store::open(&repo).unwrap();
        let main = read_branch_commit(&repo, "main").unwrap().unwrap();
        let commit = store.get_commit(main).unwrap();
        let mut catalog = Catalog::load(&store, commit.view).unwrap();
        let files = store.put_symbol("files").unwrap();
        let feature_name = store.put_symbol("files/feature").unwrap();
        catalog
            .insert(
                feature_name,
                DesignObject::new(
                    Kind::Directory,
                    ObjectPayload::Directory(DirectoryPayload {
                        qualified_name: feature_name,
                        parent: ParentRef::Object(files),
                        source: None,
                        document: SymbolHash::ZERO,
                        children: vec![],
                    }),
                ),
            )
            .unwrap();
        let view = catalog.rebuild(&store).unwrap();
        let feature = store
            .put_commit(&Commit {
                view,
                parent: main,
                message: "feature".to_string(),
                author: "test".to_string(),
                timestamp: 2,
            })
            .unwrap();
        write_branch_ref(&repo, "feature", feature).unwrap();

        let access = DesignSpaceAccess::new(&root);
        let project = project(uuid::Uuid::now_v7());
        let main_hex = main.to_hex();
        let feature_hex = feature.to_hex();
        assert_eq!(
            access
                .branch_head(&project, "main")
                .await
                .unwrap()
                .as_deref(),
            Some(main_hex.as_str())
        );
        assert!(
            access
                .is_ancestor(&project, &main_hex, &feature_hex)
                .await
                .unwrap()
        );
        let changes = access
            .diff(&project, &main_hex, &feature_hex)
            .await
            .unwrap();
        let added = changes
            .iter()
            .find(|change| change.qname == "files/feature")
            .expect("added feature directory");
        assert_eq!(added.status, MergeRequestChangeStatus::Added);
        assert!(
            changes.iter().any(|change| change.qname == "files"
                && change.status == MergeRequestChangeStatus::Modified)
        );

        access
            .fast_forward(&project, "main", &main_hex, &feature_hex)
            .await
            .unwrap();
        assert_eq!(
            access.branch_head(&project, "main").await.unwrap(),
            Some(feature_hex)
        );
    }

    #[tokio::test]
    async fn rejects_a_non_ancestor_and_a_moved_target() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("repos");
        let collection = RepoCollection::open(&root).unwrap();
        collection.create("alpha", "main").unwrap();
        let repo = root.join("alpha");
        let store = Store::open(&repo).unwrap();
        let main = read_branch_commit(&repo, "main").unwrap().unwrap();
        let commit = store.get_commit(main).unwrap();
        let view = Catalog::load(&store, commit.view).unwrap();
        let other = store
            .put_commit(&Commit {
                view: view.rebuild(&store).unwrap(),
                parent: CommitHash::ZERO,
                message: "unrelated".to_string(),
                author: "test".to_string(),
                timestamp: 2,
            })
            .unwrap();
        write_branch_ref(&repo, "feature", other).unwrap();

        let access = DesignSpaceAccess::new(&root);
        let project = project(uuid::Uuid::now_v7());
        assert!(
            !access
                .is_ancestor(&project, &main.to_hex(), &other.to_hex())
                .await
                .unwrap()
        );
        assert!(
            access
                .fast_forward(&project, "main", &main.to_hex(), &other.to_hex())
                .await
                .is_err()
        );
        assert!(
            access
                .fast_forward(&project, "main", &"0".repeat(64), &other.to_hex())
                .await
                .is_err()
        );
    }
}
