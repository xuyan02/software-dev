use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::hash::Hash32;

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn unique_tmp_path(tmp_dir: &Path, prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    tmp_dir.join(format!(
        "{prefix}-{}-{nanos}-{counter}.tmp",
        std::process::id()
    ))
}

pub fn write_atomic(path: &Path, bytes: &[u8], tmp_dir: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::create_dir_all(tmp_dir).with_context(|| format!("create {}", tmp_dir.display()))?;
    let temp = unique_tmp_path(tmp_dir, "write");
    {
        let mut file = fs::File::create(&temp)
            .with_context(|| format!("create temporary file {}", temp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temporary file {}", temp.display()))?;
        file.sync_all().ok();
    }
    match fs::rename(&temp, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temp);
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error).with_context(|| format!("publish {}", path.display()))
        }
    }
}

pub fn fanout_path(root: &Path, hash: Hash32) -> PathBuf {
    let hex = hash.to_hex();
    root.join(&hex[..2]).join(&hex[2..4]).join(hex)
}

pub fn list_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut file = Vec::new();
    if !root.exists() {
        return Ok(file);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in
            fs::read_dir(&path).with_context(|| format!("read directory {}", path.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else {
                file.push(path);
            }
        }
    }
    file.sort();
    Ok(file)
}
