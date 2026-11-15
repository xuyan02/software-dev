//! Local branches.
//!
//! A local branch has an optional committed tip (`refs/heads/<name>`) and an
//! optional working state (`work/<name>/`). `HEAD` names the current branch,
//! like git's symbolic `HEAD`. The branch is created as an unborn branch by
//! ``create`` or ``checkout -b``; its ref appears on the first commit.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub const DEFAULT: &str = "main";

/// Branch names are flat: letters, digits, `.`, `_` and `-`.
pub fn validate_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.starts_with('-')
        && !name.contains("..")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid branch name: {name}")))
    }
}

pub fn current(dir: &Path) -> Result<String> {
    let text = match fs::read_to_string(dir.join("HEAD")) {
        Ok(text) => text,
        Err(_) => return Ok(DEFAULT.to_owned()),
    };
    let text = text.trim();
    if text.is_empty() {
        return Ok(DEFAULT.to_owned());
    }
    let name = text
        .strip_prefix("ref: refs/heads/")
        .ok_or_else(|| Error::Config(format!("unsupported HEAD: {text}")))?;
    validate_name(name)?;
    Ok(name.to_owned())
}

pub fn set_current(dir: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    fs::create_dir_all(dir)?;
    let temp = dir.join("HEAD.tmp");
    fs::write(&temp, format!("ref: refs/heads/{name}\n"))?;
    fs::rename(&temp, dir.join("HEAD"))?;
    Ok(())
}

/// Create an unborn branch and switch to it.
pub fn create(dir: &Path, name: &str) -> Result<()> {
    set_current(dir, name)
}

/// Create an unborn branch without switching to it.
///
/// An empty marker file is the branch's identity until its first version.
pub fn create_unborn(dir: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    if exists(dir, name) {
        return Err(Error::Config(format!("branch {name} already exists")));
    }
    let marker = marker_path(dir, name);
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(marker, "")?;
    Ok(())
}

/// Give an unborn branch a marker so it survives switching away from it.
pub fn ensure_exists(dir: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    if ref_path(dir, name).is_file()
        || marker_path(dir, name).is_file()
        || legacy_work_root(dir, name).is_dir()
    {
        return Ok(());
    }
    let marker = marker_path(dir, name);
    if let Some(parent) = marker.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(marker, "")?;
    Ok(())
}

pub fn ref_path(dir: &Path, name: &str) -> PathBuf {
    dir.join("refs").join("heads").join(name)
}

/// Marker for a branch that has no version yet.
pub fn marker_path(dir: &Path, name: &str) -> PathBuf {
    dir.join("branches").join(name)
}

fn legacy_work_root(dir: &Path, name: &str) -> PathBuf {
    dir.join("work").join(name)
}

pub fn exists(dir: &Path, name: &str) -> bool {
    validate_name(name).is_ok()
        && (ref_path(dir, name).is_file()
            || marker_path(dir, name).is_file()
            || legacy_work_root(dir, name).is_dir()
            || current(dir).is_ok_and(|current| current == name))
}

/// Every branch that has a ref, a work state, or is the current unborn branch.
pub fn list(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let heads = dir.join("refs").join("heads");
    if let Ok(entries) = fs::read_dir(&heads) {
        for entry in entries.flatten() {
            if entry.path().is_file()
                && let Ok(name) = entry.file_name().into_string()
                && validate_name(&name).is_ok()
            {
                names.push(name);
            }
        }
    }
    let markers = dir.join("branches");
    if let Ok(entries) = fs::read_dir(&markers) {
        for entry in entries.flatten() {
            if entry.path().is_file()
                && let Ok(name) = entry.file_name().into_string()
                && validate_name(&name).is_ok()
            {
                names.push(name);
            }
        }
    }
    let legacy = dir.join("work");
    if let Ok(entries) = fs::read_dir(&legacy) {
        for entry in entries.flatten() {
            if entry.path().is_dir()
                && let Ok(name) = entry.file_name().into_string()
                && validate_name(&name).is_ok()
            {
                names.push(name);
            }
        }
    }
    let current = current(dir)?;
    if !names.contains(&current) {
        names.push(current);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn delete(dir: &Path, name: &str) -> Result<()> {
    validate_name(name)?;
    if current(dir)? == name {
        return Err(Error::Config(format!(
            "cannot delete the current branch: {name}"
        )));
    }
    let reference = ref_path(dir, name);
    if reference.is_file() {
        fs::remove_file(reference)?;
    }
    let marker = marker_path(dir, name);
    if marker.is_file() {
        fs::remove_file(marker)?;
    }
    let legacy = legacy_work_root(dir, name);
    if legacy.is_dir() {
        fs::remove_dir_all(legacy)?;
    }
    Ok(())
}

/// Make sure the current branch has a working-state marker.
pub fn ensure_initialized(dir: &Path) -> Result<()> {
    if dir.join("HEAD").is_file() {
        let current = current(dir)?;
        ensure_exists(dir, &current)?;
        return Ok(());
    }
    set_current(dir, DEFAULT)?;
    ensure_exists(dir, DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn tracks_the_current_branch() {
        let dir = temp();
        let meta = dir.path();

        assert_eq!(current(meta).unwrap(), DEFAULT);
        set_current(meta, "work").unwrap();
        assert_eq!(current(meta).unwrap(), "work");
        assert!(exists(meta, "work"));
        assert!(list(meta).unwrap().contains(&"work".to_owned()));
    }

    #[test]
    fn creates_unborn_branches_without_switching() {
        let dir = temp();
        let meta = dir.path();
        set_current(meta, "main").unwrap();

        create_unborn(meta, "work").unwrap();
        assert_eq!(current(meta).unwrap(), "main");
        assert!(exists(meta, "work"));
        assert!(list(meta).unwrap().contains(&"work".to_owned()));
        assert!(create_unborn(meta, "work").is_err());

        delete(meta, "work").unwrap();
        assert!(!exists(meta, "work"));
    }

    #[test]
    fn deletes_non_current_branches() {
        let dir = temp();
        let meta = dir.path();
        set_current(meta, "main").unwrap();
        create(meta, "scratch").unwrap();
        set_current(meta, "main").unwrap();

        delete(meta, "scratch").unwrap();
        assert!(!exists(meta, "scratch"));
        assert!(delete(meta, "main").is_err());
    }

    #[test]
    fn rejects_bad_names() {
        for name in ["", "-x", ".x", "a/b", "a b", ".."] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name("feature-1_2").is_ok());
    }
}
