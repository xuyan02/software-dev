//! Reachability, repack and garbage collection.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hash::{Hash, from_hex, is_zero, to_hex};
use crate::node::Node;
use crate::pack::{self, PackIndex};
use crate::store::Store;

#[derive(Debug, Default)]
pub struct GcReport {
    pub reachable: usize,
    pub pack: Option<String>,
    pub pack_nodes: usize,
    pub removed_packs: usize,
    pub removed_loose: usize,
    pub removed_cache: usize,
}

#[derive(Debug, Default)]
pub struct RepackReport {
    pub nodes: usize,
    pub pack: Option<String>,
    pub removed_packs: usize,
    pub removed_loose: usize,
}

struct Reachable {
    nodes: BTreeSet<Hash>,
    versions: BTreeSet<Hash>,
}

/// Every node reachable from refs/heads and refs/remotes, including version
/// parents and design nodes.
pub fn reachable(store: &Store, dir: &Path) -> Result<BTreeSet<Hash>> {
    Ok(collect(store, dir)?.nodes)
}

fn collect(store: &Store, dir: &Path) -> Result<Reachable> {
    let mut nodes = BTreeSet::new();
    let mut versions = BTreeSet::new();
    let mut stack = ref_roots(dir)?;
    while let Some(hash) = stack.pop() {
        if !nodes.insert(hash) {
            continue;
        }
        match store.get(&hash)? {
            Node::Version(version) => {
                versions.insert(hash);
                if !is_zero(&version.parent) {
                    stack.push(version.parent);
                }
                stack.push(version.root);
            }
            Node::Index(index) => stack.extend(index.entries.iter().map(|entry| entry.hash)),
            Node::Object(object) => {
                if !is_zero(&object.design_hash) {
                    stack.push(object.design_hash);
                }
            }
            Node::Design(_) => {}
        }
    }
    Ok(Reachable { nodes, versions })
}

fn ref_roots(dir: &Path) -> Result<Vec<Hash>> {
    let mut roots = Vec::new();
    for kind in ["heads", "remotes"] {
        let root = dir.join("refs").join(kind);
        walk_refs(&root, &mut roots)?;
    }
    Ok(roots)
}

fn walk_refs(dir: &Path, roots: &mut Vec<Hash>) -> Result<()> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            walk_refs(&entry.path(), roots)?;
        } else if entry.file_type()?.is_file() {
            let text = fs::read_to_string(entry.path())?;
            roots.push(from_hex(text.trim())?);
        }
    }
    Ok(())
}

/// Prune unreachable nodes, keep reachable ones in one fresh pack.
pub fn gc(store: &Store, dir: &Path) -> Result<GcReport> {
    let _lock = Lock::acquire(dir)?;
    let reachable = collect(store, dir)?;
    let nodes: Vec<Node> = reachable
        .nodes
        .iter()
        .map(|hash| store.get(hash))
        .collect::<Result<_>>()?;

    let before = list_packs(dir)?;
    let (pack_path, checksum, count) = if nodes.is_empty() {
        (None, None, 0)
    } else {
        let written = pack::write(&nodes)?;
        let path = write_pack(dir, &written.bytes, &written.checksum)?;
        (Some(path), Some(to_hex(&written.checksum)), written.count)
    };

    if let (Some(path), Some(_)) = (&pack_path, &checksum) {
        let index = PackIndex::open(path)?;
        for hash in &reachable.nodes {
            if !index.contains(hash) {
                return Err(Error::InvalidPack(format!(
                    "new pack misses {}",
                    to_hex(hash)
                )));
            }
        }
    }

    let removed_packs = remove_old_packs(&before, pack_path.as_deref())?;
    let removed_loose = store.remove_loose()?;
    let removed_cache = prune_cache(dir, &reachable.versions)?;

    Ok(GcReport {
        reachable: reachable.nodes.len(),
        pack: checksum,
        pack_nodes: count,
        removed_packs,
        removed_loose,
        removed_cache,
    })
}

/// Pack every node currently in the store into one pack. No pruning.
pub fn repack(store: &Store, dir: &Path) -> Result<RepackReport> {
    let _lock = Lock::acquire(dir)?;
    let nodes = store.load_all()?;
    let before = list_packs(dir)?;
    let (pack_path, checksum, count) = if nodes.is_empty() {
        (None, None, 0)
    } else {
        let written = pack::write(&nodes)?;
        let path = write_pack(dir, &written.bytes, &written.checksum)?;
        let index = PackIndex::open(&path)?;
        for node in &nodes {
            let hash = node.hash();
            if !index.contains(&hash) {
                return Err(Error::InvalidPack(format!(
                    "new pack misses {}",
                    to_hex(&hash)
                )));
            }
        }
        (Some(path), Some(to_hex(&written.checksum)), written.count)
    };

    let removed_packs = remove_old_packs(&before, pack_path.as_deref())?;
    let removed_loose = store.remove_loose()?;
    Ok(RepackReport {
        nodes: count,
        pack: checksum,
        removed_packs,
        removed_loose,
    })
}

fn write_pack(dir: &Path, bytes: &[u8], checksum: &Hash) -> Result<PathBuf> {
    let packs = dir.join("packs");
    fs::create_dir_all(&packs)?;
    fs::create_dir_all(dir.join("tmp"))?;
    let path = packs.join(format!("{}.pack", to_hex(checksum)));
    let temp = dir
        .join("tmp")
        .join(format!("{}.pack.tmp", to_hex(checksum)));
    fs::write(&temp, bytes)?;
    fs::rename(temp, &path)?;
    Ok(path)
}

fn list_packs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut packs = Vec::new();
    let Ok(entries) = fs::read_dir(dir.join("packs")) else {
        return Ok(packs);
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|ext| ext == "pack")
        {
            packs.push(entry.path());
        }
    }
    Ok(packs)
}

fn remove_old_packs(before: &[PathBuf], keep: Option<&Path>) -> Result<usize> {
    let mut removed = 0;
    for path in before {
        if keep.is_some_and(|keep| keep == path) {
            continue;
        }
        fs::remove_file(path)?;
        removed += 1;
    }
    Ok(removed)
}

fn prune_cache(dir: &Path, versions: &BTreeSet<Hash>) -> Result<usize> {
    let trees = dir.join("cache").join("trees");
    let Ok(entries) = fs::read_dir(&trees) else {
        return Ok(0);
    };
    let keep: BTreeSet<String> = versions.iter().map(|hash| to_hex(hash)).collect();
    let mut removed = 0;
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if !keep.contains(stem) {
            fs::remove_file(path)?;
            removed += 1;
        }
    }
    Ok(removed)
}

struct Lock {
    path: PathBuf,
}

impl Lock {
    fn acquire(dir: &Path) -> Result<Self> {
        let path = dir.join("gc.lock");
        fs::create_dir_all(dir)?;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(Self { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Err(Error::Config(
                "another gc/repack is running (remove .ds/gc.lock if stale)".into(),
            )),
            Err(error) => Err(Error::Io(error)),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::space::{self, CommitOptions};
    use crate::types::CPP_CLASS;

    fn workspace() -> (tempfile::TempDir, Store, PathBuf, Hash) {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join(".ds");
        fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let elements = vec![Element {
            name: "acme::Foo".into(),
            ty: CPP_CLASS,
            path: "a.cpp".into(),
            commit: "00".repeat(20),
            start_byte: 0,
            end_byte: 1,
            start_line: 1,
            end_line: 1,
            own_text: "class Foo {}".into(),
            parent: None,
            children: vec![],
            design: Some("the class".into()),
            content_hash: None,
        }];
        let result = space::commit(
            &store,
            &dir,
            &elements,
            &CommitOptions {
                message: "test".into(),
                author: "test".into(),
                source_commit: [0u8; 32],
                pipeline: "test".into(),
                ref_name: "main".into(),
            },
        )
        .unwrap();
        (temp, store, dir, result.version)
    }

    #[test]
    fn gc_prunes_unreachable_nodes_and_keeps_pack_reads() {
        let (_temp, store, dir, version) = workspace();
        let orphan = store
            .put(&Node::Design(crate::node::DesignNode {
                text: "orphan".into(),
            }))
            .unwrap();
        assert!(store.contains(&orphan));

        let report = gc(&store, &dir).unwrap();
        assert!(report.reachable > 0);
        assert!(report.removed_loose > 0);
        assert!(store.loose_entries().unwrap().is_empty());

        let reopened = Store::new(&dir);
        assert_eq!(
            reopened.get(&version).unwrap(),
            store.get(&version).unwrap()
        );
        assert!(!reopened.contains(&orphan));
    }

    #[test]
    fn repack_keeps_every_node() {
        let (_temp, store, dir, version) = workspace();
        let orphan = store
            .put(&Node::Design(crate::node::DesignNode {
                text: "orphan".into(),
            }))
            .unwrap();

        let report = repack(&store, &dir).unwrap();
        assert!(report.nodes > 0);
        assert!(store.loose_entries().unwrap().is_empty());

        let reopened = Store::new(&dir);
        assert!(reopened.get(&version).is_ok());
        assert!(reopened.contains(&orphan));
    }
}
