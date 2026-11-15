//! Read-only access to a local design space for the console.
//!
//! Foundry starts its own `ds-server` in local mode and therefore also owns the
//! repository root. The console reads that repository directly: object nodes
//! give the element tree and design text, and a small bare-clone cache of the
//! project's git repository gives source snippets. No source code is executed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use design_space as ds;
use ds::hash::{from_hex, is_zero, to_commit_hex, to_hex};
use ds::node::Node;
use ds::store::Store;
use ds::types::{T_FILE, type_name};
use foundry_domain::Project;
use foundry_web::{
    DesignSpaceDetail, DesignSpaceElement, DesignSpaceLine, DesignSpaceOverview, DesignSpaceSource,
};
use sha2::{Digest, Sha256};

const SOURCE_CONTEXT: u32 = 18;
const SOURCE_MAX_LINES: u32 = 600;

/// Local design-space root plus the source-clone cache.
#[derive(Clone)]
pub struct DesignSpaceAccess {
    root: PathBuf,
    cache_root: Option<PathBuf>,
    git_lock: Arc<Mutex<()>>,
}

impl DesignSpaceAccess {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let cache_root = std::env::var_os("FOUNDRY_SOURCE_CACHE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| PathBuf::from(home).join(".foundry").join("source-cache"))
            });
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
        let project = self.local_project(project)?;
        let store = Store::new(&project.dir);
        let (branch, version) = current_version(&project.dir)?;
        let mut overview = DesignSpaceOverview {
            branch,
            version: version.as_ref().map(|version| to_hex(version)),
            source_commit: String::new(),
            file_count: 0,
            element_count: 0,
            designed_count: 0,
            elements: Vec::new(),
        };
        let Some(version) = version else {
            return Ok(overview);
        };
        let version_node = ds::space::load_version(&store, &version).map_err(display)?;
        overview.source_commit = to_commit_hex(&version_node.source_commit);

        let Some(view) = ds::space::view_root(&store, &version_node, b"files").map_err(display)?
        else {
            return Ok(overview);
        };
        let mut entries = Vec::new();
        ds::space::collect_view(&store, &view, &mut entries).map_err(display)?;
        let mut paths = std::collections::BTreeSet::new();

        for (_, object_hash) in entries {
            let Node::Object(object) = store.get(&object_hash).map_err(display)? else {
                continue;
            };
            let (lang, kind) = type_labels(object.ty);
            let is_file = object.ty == T_FILE;
            let has_design = !is_zero(&object.design_hash);
            if !object.path.is_empty() {
                paths.insert(object.path.clone());
            }
            if !is_file {
                overview.element_count += 1;
                if has_design {
                    overview.designed_count += 1;
                }
            }
            overview.elements.push(DesignSpaceElement {
                hash: to_hex(&object_hash),
                name: object.name,
                path: object.path,
                lang,
                kind,
                start_line: object.start_line,
                end_line: object.end_line,
                has_design,
                is_file,
            });
        }
        overview.file_count = paths.len();
        overview.elements.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.start_line.cmp(&right.start_line))
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(overview)
    }

    fn element_blocking(&self, project: &Project, hash: &str) -> Result<DesignSpaceDetail, String> {
        let project = self.local_project(project)?;
        let store = Store::new(&project.dir);
        let address = from_hex(hash).map_err(|error| format!("invalid element hash: {error}"))?;
        let Node::Object(object) = store.get(&address).map_err(display)? else {
            return Err("该哈希不是语言元素".to_string());
        };
        let design = if is_zero(&object.design_hash) {
            None
        } else {
            match store.get(&object.design_hash).map_err(display)? {
                Node::Design(design) => Some(design.text),
                _ => None,
            }
        };
        let (lang, kind) = type_labels(object.ty);
        Ok(DesignSpaceDetail {
            hash: to_hex(&address),
            name: object.name,
            lang,
            kind,
            path: object.path,
            start_line: object.start_line,
            end_line: object.end_line,
            design,
        })
    }

    fn source_blocking(&self, project: &Project, hash: &str) -> Result<DesignSpaceSource, String> {
        let local = self.local_project(project)?;
        let store = Store::new(&local.dir);
        let address = from_hex(hash).map_err(|error| format!("invalid element hash: {error}"))?;
        let Node::Object(object) = store.get(&address).map_err(display)? else {
            return Err("该哈希不是语言元素".to_string());
        };
        let commit = to_commit_hex(&object.commit);
        let repo = self.ensure_source(project, &commit)?;
        let text = read_blob(&repo, &commit, &object.path)?;
        source_lines(
            &text,
            &commit,
            &object.path,
            object.start_line,
            object.end_line,
        )
    }

    fn local_project(&self, project: &Project) -> Result<ds::project::Project, String> {
        let key = design_space_key(&project.design_space_url)
            .ok_or_else(|| "项目还没有分配设计空间".to_string())?;
        if key.contains('/') {
            // A pre-id address recorded an absolute repository path; only the
            // final directory name is meaningful here.
            let slug = percent_decode(key.rsplit('/').next().unwrap_or(&key));
            ds::project::open(&self.root, &slug).map_err(display)
        } else {
            ds::project::find_by_id(&self.root, &key).map_err(display)
        }
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

fn current_version(dir: &Path) -> Result<(String, Option<ds::hash::Hash>), String> {
    let current = ds::branch::current(dir).map_err(display)?;
    if let Some(version) = ds::space::read_ref_optional(dir, &current).map_err(display)? {
        return Ok((current, Some(version)));
    }
    for name in ds::branch::list(dir).map_err(display)? {
        if let Some(version) = ds::space::read_ref_optional(dir, &name).map_err(display)? {
            return Ok((name, Some(version)));
        }
    }
    Ok((current, None))
}

fn type_labels(ty: u32) -> (String, String) {
    let full = type_name(ty).unwrap_or("unknown");
    match full.split_once(':') {
        Some((lang, kind)) => (lang.to_owned(), kind.to_owned()),
        None => (String::new(), full.to_owned()),
    }
}

/// Project id from `ds://[user@]host[:port]/<id>`; legacy addresses keep their
/// path, which the caller handles separately.
fn design_space_key(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("ds://")?;
    let (_, path) = rest.split_once('/')?;
    let path = path.trim_matches('/');
    if path.is_empty() {
        None
    } else {
        Some(path.to_owned())
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

    #[test]
    fn reads_ids_and_legacy_paths_from_addresses() {
        assert_eq!(
            design_space_key("ds://xuyan@localhost/f5acb3bbde1a05e1").as_deref(),
            Some("f5acb3bbde1a05e1")
        );
        assert_eq!(
            design_space_key("ds://host/home/x/%E8%BD%AF%E4%BB%B6").as_deref(),
            Some("home/x/%E8%BD%AF%E4%BB%B6")
        );
        assert_eq!(design_space_key("ds://host/"), None);
        assert_eq!(percent_decode("%E8%BD%AF%E4%BB%B6"), "软件");
    }

    #[test]
    fn splits_type_labels() {
        assert_eq!(
            type_labels(ds::types::RUST_FUNCTION),
            ("rust".to_string(), "function".to_string())
        );
        assert_eq!(type_labels(T_FILE), (String::new(), "file".to_string()));
        assert_eq!(
            type_labels(ds::types::T_CONCEPT),
            (String::new(), "concept".to_string())
        );
    }

    #[test]
    fn reads_overview_and_element_from_a_local_project() {
        let root = std::env::temp_dir().join(format!("foundry-ds-{}", uuid::Uuid::now_v7()));
        let _ = std::fs::remove_dir_all(&root);
        let local = ds::project::create(
            &root,
            "Demo",
            Some("git@example.com:demo.git"),
            "localhost",
            Some("u"),
            None,
        )
        .unwrap();
        let store = Store::new(&local.dir);
        let elements = vec![ds::element::Element {
            name: "crate::main".to_string(),
            ty: ds::types::RUST_FUNCTION,
            path: "src/main.rs".to_string(),
            commit: "00".repeat(20),
            start_byte: 0,
            end_byte: 12,
            start_line: 1,
            end_line: 3,
            own_text: "fn main() {}".to_string(),
            parent: None,
            children: Vec::new(),
            design: Some("程序入口。".to_string()),
            content_hash: None,
        }];
        ds::space::commit(
            &store,
            &local.dir,
            &elements,
            &ds::space::CommitOptions {
                message: "test".to_string(),
                author: "test".to_string(),
                source_commit: [0u8; 32],
                pipeline: "test".to_string(),
                ref_name: "main".to_string(),
            },
        )
        .unwrap();

        let mut project = Project::new(
            "Demo",
            None,
            "git@example.com:demo.git",
            uuid::Uuid::now_v7(),
        )
        .unwrap();
        project.design_space_url = local.meta.design_space.clone();

        let access = DesignSpaceAccess::new(&root);
        let overview = access.overview_blocking(&project).unwrap();
        assert_eq!(overview.branch, "main");
        assert_eq!(overview.file_count, 1);
        assert_eq!(overview.element_count, 1);
        assert_eq!(overview.designed_count, 1);
        assert_eq!(overview.elements[0].name, "crate::main");
        assert_eq!(overview.elements[0].lang, "rust");
        assert_eq!(overview.elements[0].kind, "function");

        let detail = access
            .element_blocking(&project, &overview.elements[0].hash)
            .unwrap();
        assert_eq!(detail.design.as_deref(), Some("程序入口。"));
        assert_eq!(detail.path, "src/main.rs");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_conceptual_elements_without_source_anchors() {
        let root =
            std::env::temp_dir().join(format!("foundry-ds-concept-{}", uuid::Uuid::now_v7()));
        let _ = std::fs::remove_dir_all(&root);
        let local = ds::project::create(
            &root,
            "Demo",
            Some("git@example.com:demo.git"),
            "localhost",
            Some("u"),
            None,
        )
        .unwrap();
        let store = Store::new(&local.dir);
        let elements = vec![ds::element::Element {
            name: "Auth::Login".to_string(),
            ty: ds::types::T_CONCEPT,
            path: String::new(),
            commit: "00".repeat(20),
            start_byte: 0,
            end_byte: 0,
            start_line: 0,
            end_line: 0,
            own_text: String::new(),
            parent: None,
            children: Vec::new(),
            design: Some("登录流程".to_string()),
            content_hash: None,
        }];
        ds::space::commit(
            &store,
            &local.dir,
            &elements,
            &ds::space::CommitOptions {
                message: "create Auth::Login".to_string(),
                author: "agent".to_string(),
                source_commit: [0u8; 32],
                pipeline: "agent/v1".to_string(),
                ref_name: "main".to_string(),
            },
        )
        .unwrap();
        let mut project = Project::new(
            "Demo",
            None,
            "git@example.com:demo.git",
            uuid::Uuid::now_v7(),
        )
        .unwrap();
        project.design_space_url = local.meta.design_space.clone();
        let access = DesignSpaceAccess::new(&root);
        let overview = access.overview_blocking(&project).unwrap();
        assert_eq!(overview.file_count, 0);
        assert_eq!(overview.element_count, 1);
        assert_eq!(overview.elements[0].kind, "concept");
        assert!(overview.elements[0].path.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reads_source_from_a_local_git_repository() {
        let root = std::env::temp_dir().join(format!("foundry-ds-src-{}", uuid::Uuid::now_v7()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("source");
        std::fs::create_dir_all(&source).unwrap();
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .arg("-C")
                .arg(&source)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "test"]);
        std::fs::write(
            source.join("main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        run(&["add", "main.rs"]);
        run(&["commit", "-q", "-m", "init"]);
        let commit = run(&["rev-parse", "HEAD"]);

        let spaces = root.join("spaces");
        let local = ds::project::create(
            &spaces,
            "Demo",
            Some(&source.to_string_lossy()),
            "localhost",
            Some("u"),
            None,
        )
        .unwrap();
        let store = Store::new(&local.dir);
        let elements = vec![ds::element::Element {
            name: "crate::main".to_string(),
            ty: ds::types::RUST_FUNCTION,
            path: "main.rs".to_string(),
            commit: commit.clone(),
            start_byte: 0,
            end_byte: 13,
            start_line: 1,
            end_line: 1,
            own_text: "fn main()".to_string(),
            parent: None,
            children: Vec::new(),
            design: None,
            content_hash: None,
        }];
        ds::space::commit(
            &store,
            &local.dir,
            &elements,
            &ds::space::CommitOptions {
                message: "test".to_string(),
                author: "test".to_string(),
                source_commit: [0u8; 32],
                pipeline: "test".to_string(),
                ref_name: "main".to_string(),
            },
        )
        .unwrap();

        let mut project =
            Project::new("Demo", None, source.to_string_lossy(), uuid::Uuid::now_v7()).unwrap();
        project.design_space_url = local.meta.design_space.clone();
        let access = DesignSpaceAccess::new(&spaces);
        let overview = access.overview_blocking(&project).unwrap();
        let source_view = access
            .source_blocking(&project, &overview.elements[0].hash)
            .unwrap();
        assert_eq!(source_view.path, "main.rs");
        assert!(
            source_view
                .lines
                .iter()
                .any(|line| line.text.contains("fn main()"))
        );
        assert!(source_view.lines.iter().any(|line| line.in_element));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extracts_a_line_window() {
        let text = (1..=100)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let source = source_lines(&text, "abc", "src/a.rs", 50, 52).unwrap();
        assert_eq!(source.first_line, 32);
        assert_eq!(source.last_line, 70);
        assert_eq!(source.start_line, 50);
        assert!(source.lines.iter().filter(|line| line.in_element).count() == 3);
    }
}
