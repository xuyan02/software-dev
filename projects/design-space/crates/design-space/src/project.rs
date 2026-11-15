//! Project provisioning: a project owns one server-side design space.
//!
//! The server repository lives under `<root>/<project id>` and its address is
//! exposed as `ds://[user@]host[:port]/<project id>`. The URL never contains
//! the server's filesystem path. Names are display metadata and may repeat:
//! every creation allocates a new id and therefore a new repository.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hash::{hash, to_hex};
use crate::remote::ServerRepo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub slug: String,
    /// Stable ASCII identifier used in `ds://` addresses.
    #[serde(default)]
    pub id: String,
    /// `ds://` address clients use for push/fetch.
    pub design_space: String,
    /// Source git repository this design space describes, when the caller
    /// supplied one. Optional: `ds` itself has no git requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub dir: PathBuf,
    pub meta: ProjectMeta,
}

impl Project {
    pub fn root(&self) -> &Path {
        &self.dir
    }
}

pub fn create(
    root: &Path,
    name: &str,
    source: Option<&str>,
    host: &str,
    user: Option<&str>,
    port: Option<u16>,
) -> Result<Project> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Config("project name must not be empty".into()));
    }
    let slug = slugify(name);
    if slug.is_empty() {
        return Err(Error::Config(format!(
            "cannot derive a directory name from {name}"
        )));
    }

    fs::create_dir_all(root)?;
    let created_at = now();
    // Every creation gets its own repository. Names are not unique (two
    // projects may share one), so the directory is keyed by the project id.
    let mut salt = 0u32;
    let (id, dir) = loop {
        let seed = format!(
            "{slug}:{created_at}:{}:{}:{salt}",
            now_nanos(),
            std::process::id()
        );
        let id = to_hex(&hash(seed.as_bytes()))[..16].to_string();
        let dir = root.join(&id);
        match fs::create_dir(&dir) {
            Ok(()) => break (id, dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                salt = salt.wrapping_add(1);
            }
            Err(error) => return Err(error.into()),
        }
    };
    ServerRepo::open(&dir)?;
    let authority = match user.filter(|user| !user.is_empty()) {
        Some(user) => format!("{user}@{host}"),
        None => host.to_owned(),
    };
    let authority = match port {
        Some(port) => format!("{authority}:{port}"),
        None => authority,
    };
    let design_space = format!("ds://{authority}/{id}");

    let source = source
        .map(str::trim)
        .filter(|source| !source.is_empty())
        .map(str::to_owned);
    let meta = ProjectMeta {
        name: name.to_owned(),
        slug: slug.clone(),
        id,
        design_space,
        source,
        created_at,
    };
    write_meta(&dir, &meta)?;
    Ok(Project { dir, meta })
}

pub fn list(root: &Path) -> Result<Vec<Project>> {
    let Ok(entries) = fs::read_dir(root) else {
        return Ok(Vec::new());
    };
    let mut projects = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Ok(project) = open_dir(&entry.path()) {
            projects.push(project);
        }
    }
    projects.sort_by(|a, b| a.meta.name.cmp(&b.meta.name));
    Ok(projects)
}

pub fn open(root: &Path, key: &str) -> Result<Project> {
    for project in list(root)? {
        if project.meta.slug == key || project.meta.name == key || project.meta.id == key {
            return Ok(project);
        }
    }
    Err(Error::NotFound(format!("project {key}")))
}

/// Record or replace the source repository advertised to clients.
pub fn set_source(root: &Path, key: &str, source: &str) -> Result<Project> {
    let mut project = open(root, key)?;
    let source = source.trim();
    project.meta.source = if source.is_empty() {
        None
    } else {
        Some(source.to_owned())
    };
    write_meta(&project.dir, &project.meta)?;
    Ok(project)
}

pub fn find_by_id(root: &Path, id: &str) -> Result<Project> {
    list(root)?
        .into_iter()
        .find(|project| project.meta.id == id)
        .ok_or_else(|| Error::NotFound(format!("project id {id}")))
}

fn open_dir(dir: &Path) -> Result<Project> {
    let text = fs::read_to_string(dir.join("project.toml"))?;
    let meta: ProjectMeta =
        toml::from_str(&text).map_err(|error| Error::Config(error.to_string()))?;
    let dir_name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    // New projects live in `<id>/`; older ones used `<slug>/`.
    if dir_name != meta.id && dir_name != meta.slug {
        return Err(Error::Config(
            "project metadata does not match its directory".into(),
        ));
    }
    Ok(Project {
        dir: dir.to_path_buf(),
        meta,
    })
}

fn write_meta(dir: &Path, meta: &ProjectMeta) -> Result<()> {
    let text = toml::to_string_pretty(meta).map_err(|error| Error::Config(error.to_string()))?;
    let temp = dir.join("project.toml.tmp");
    fs::write(&temp, text)?;
    fs::rename(temp, dir.join("project.toml"))?;
    Ok(())
}

fn slugify(name: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_alphanumeric() {
            slug.push(ch);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_lists_and_finds_projects() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let project = create(
            &root,
            "Demo App",
            Some("git@example.com:demo.git"),
            "ds.example.com",
            Some("ds"),
            Some(2222),
        )
        .unwrap();

        assert!(project.dir.join("objects").is_dir());
        assert!(project.dir.join("refs/heads").is_dir());
        assert!(project.dir.join("project.toml").is_file());
        assert_eq!(
            project.dir.file_name().unwrap().to_str().unwrap(),
            project.meta.id
        );
        assert_eq!(project.meta.slug, "demo-app");
        assert_eq!(project.meta.id.len(), 16);
        assert!(project.meta.id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_eq!(
            project.meta.design_space,
            format!("ds://ds@ds.example.com:2222/{}", project.meta.id)
        );
        assert_eq!(
            project.meta.source.as_deref(),
            Some("git@example.com:demo.git")
        );

        let projects = list(&root).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(open(&root, "Demo App").unwrap().meta.slug, "demo-app");
        assert_eq!(
            find_by_id(&root, &project.meta.id).unwrap().dir,
            project.dir
        );
        assert!(open(&root, "missing").is_err());

        set_source(&root, "Demo App", "git@example.com:other.git").unwrap();
        assert_eq!(
            open(&root, "Demo App").unwrap().meta.source.as_deref(),
            Some("git@example.com:other.git")
        );
    }

    #[test]
    fn keeps_unicode_names_in_the_slug() {
        let temp = tempfile::tempdir().unwrap();
        let project = create(temp.path(), "软件开发平台", None, "host", None, None).unwrap();
        assert_eq!(project.meta.slug, "软件开发平台");
        assert!(project.meta.design_space.ends_with(&project.meta.id));
        assert!(!project.meta.design_space.contains('%'));
        assert!(project.dir.is_dir());
    }

    #[test]
    fn same_name_gets_a_separate_repository() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("projects");
        let first = create(&root, "软件开发平台", None, "host", None, None).unwrap();
        let second = create(&root, "软件开发平台", None, "host", None, None).unwrap();

        assert_ne!(first.meta.id, second.meta.id);
        assert_ne!(first.dir, second.dir);
        assert_eq!(first.meta.slug, second.meta.slug);
        assert_eq!(list(&root).unwrap().len(), 2);
        assert_eq!(find_by_id(&root, &second.meta.id).unwrap().dir, second.dir);
    }
}
