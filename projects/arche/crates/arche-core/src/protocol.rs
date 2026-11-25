//! Wire-level helpers shared by the Arche client and server.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::hash::CommitHash;
use crate::util::list_files;

/// Every immutable record file in a repository, relative to its `.arche`
/// directory and sorted.
pub fn record_files(dir: &Path) -> Result<Vec<String>> {
    let mut file = Vec::new();
    for name in ["objects", "symbols", "commits", "packs"] {
        for path in list_files(&dir.join(name))? {
            let relative = path
                .strip_prefix(dir)
                .context("record path is outside repository")?;
            file.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    file.sort();
    Ok(file)
}

/// Only object, symbol, commit and pack records may cross the wire.
pub fn valid_record_path(relative: &str) -> bool {
    !relative.contains("..")
        && (relative.starts_with("objects/")
            || relative.starts_with("symbols/")
            || relative.starts_with("commits/")
            || relative.starts_with("packs/"))
}

pub fn write_ref_file(base: &Path, name: &str, hash: CommitHash) -> Result<()> {
    let path = base.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", hash.to_hex()))?;
    Ok(())
}

pub fn read_ref_file(base: &Path, name: &str) -> Result<CommitHash> {
    let text = fs::read_to_string(base.join(name))?;
    CommitHash::from_hex(text.trim())
}

pub fn refs_under(base: &Path, relative: &str) -> Result<Vec<(String, CommitHash)>> {
    let mut refs = Vec::new();
    let root = base.join(relative);
    if !root.is_dir() {
        return Ok(refs);
    }
    collect_refs(&root, &root, &mut refs)?;
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(refs)
}

fn collect_refs(root: &Path, dir: &Path, refs: &mut Vec<(String, CommitHash)>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect_refs(root, &path, refs)?;
            continue;
        }
        let name = path
            .strip_prefix(root)
            .context("ref path is outside refs root")?
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&path)?;
        refs.push((name, CommitHash::from_hex(text.trim())?));
    }
    Ok(())
}

pub fn local_refs(base: &Path) -> Result<Vec<(String, CommitHash)>> {
    refs_under(base, "refs/heads")
}
