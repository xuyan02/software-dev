//! Loose, content-addressed node store.
//!
//! Layout:
//!
//! ```text
//! <root>/objects/<aa>/<bb>/<64 hex>
//! <root>/tmp/
//! ```
//!
//! Nodes are written to `tmp` and renamed into place, so a reader never sees
//! a partial node.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::{Error, Result};
use crate::hash::{Hash, hash, to_hex};
use crate::node::Node;
use crate::pack::MultiPackIndex;

pub struct Store {
    root: PathBuf,
    packs: OnceLock<std::result::Result<MultiPackIndex, String>>,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            packs: OnceLock::new(),
        }
    }

    pub fn packs_dir(&self) -> PathBuf {
        self.root.join("packs")
    }

    fn pack_index(&self) -> Result<&MultiPackIndex> {
        let loaded = self.packs.get_or_init(|| {
            MultiPackIndex::open(&self.packs_dir()).map_err(|error| error.to_string())
        });
        match loaded {
            Ok(index) => Ok(index),
            Err(message) => Err(Error::InvalidPack(message.clone())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    pub fn path_for(&self, h: &Hash) -> PathBuf {
        let hex = to_hex(h);
        self.objects_dir()
            .join(&hex[..2])
            .join(&hex[2..4])
            .join(hex)
    }

    /// Validate, encode and store a node. Returns its address.
    pub fn put(&self, node: &Node) -> Result<Hash> {
        node.validate()?;
        let bytes = node.encode();
        let address = hash(&bytes);
        self.put_bytes(&address, &bytes)?;
        Ok(address)
    }

    pub fn put_bytes(&self, address: &Hash, bytes: &[u8]) -> Result<()> {
        let path = self.path_for(address);
        if path.is_file() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp_dir = self.root.join("tmp");
        fs::create_dir_all(&tmp_dir)?;
        let tmp = tmp_dir.join(format!("{}.tmp", to_hex(address)));
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn get_bytes(&self, address: &Hash) -> Result<Vec<u8>> {
        let path = self.path_for(address);
        match fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                Ok(self.pack_index()?.get(address)?.encode())
            }
            Err(error) => Err(Error::Io(error)),
        }
    }

    /// Read and verify a node. The stored bytes must hash to `address`.
    pub fn get(&self, address: &Hash) -> Result<Node> {
        let bytes = self.get_bytes(address)?;
        let actual = hash(&bytes);
        if actual != *address {
            return Err(Error::InvalidNode(format!(
                "node {} hashes to {}",
                to_hex(address),
                to_hex(&actual)
            )));
        }
        Node::decode(&bytes)
    }

    pub fn contains(&self, address: &Hash) -> bool {
        self.path_for(address).is_file()
            || self
                .pack_index()
                .map(|index| index.contains(address))
                .unwrap_or(false)
    }

    /// Every hash in loose objects and packs.
    pub fn all_hashes(&self) -> Result<BTreeMap<Hash, ()>> {
        let mut hashes = BTreeMap::new();
        for (hash, _) in self.loose_entries()? {
            hashes.insert(hash, ());
        }
        for hash in self.pack_index()?.hashes() {
            hashes.insert(*hash, ());
        }
        Ok(hashes)
    }

    /// Load loose nodes only.
    pub fn load_loose(&self) -> Result<Vec<Node>> {
        self.loose_entries()?
            .into_iter()
            .map(|(hash, _)| self.get(&hash))
            .collect()
    }

    /// Load every node from loose objects and packs, deduplicated by hash.
    pub fn load_all(&self) -> Result<Vec<Node>> {
        let mut nodes: BTreeMap<Hash, Node> = BTreeMap::new();
        for (hash, _) in self.loose_entries()? {
            nodes.insert(hash, self.get(&hash)?);
        }
        for hash in self.pack_index()?.hashes() {
            if !nodes.contains_key(hash) {
                nodes.insert(*hash, self.get(hash)?);
            }
        }
        Ok(nodes.into_values().collect())
    }

    /// Delete every loose node and the now-empty shard directories.
    /// Only safe once the nodes are all in a pack.
    pub fn remove_loose(&self) -> Result<usize> {
        let entries = self.loose_entries()?;
        for (_, path) in &entries {
            fs::remove_file(path)?;
        }
        let removed = entries.len();
        self.prune_empty_object_dirs()?;
        Ok(removed)
    }

    fn prune_empty_object_dirs(&self) -> Result<()> {
        let objects = self.objects_dir();
        let Ok(first) = fs::read_dir(&objects) else {
            return Ok(());
        };
        for first in first {
            let first = first?;
            if !first.file_type()?.is_dir() {
                continue;
            }
            let Ok(second) = fs::read_dir(first.path()) else {
                continue;
            };
            for second in second {
                let second = second?;
                if second.file_type()?.is_dir() && is_empty_dir(&second.path())? {
                    let _ = fs::remove_dir(second.path());
                }
            }
            if is_empty_dir(&first.path())? {
                let _ = fs::remove_dir(first.path());
            }
        }
        Ok(())
    }

    pub(crate) fn loose_entries(&self) -> Result<Vec<(Hash, PathBuf)>> {
        let mut entries = Vec::new();
        let objects = self.objects_dir();
        if !objects.is_dir() {
            return Ok(entries);
        }
        for first in fs::read_dir(&objects)? {
            let first = first?;
            if !first.file_type()?.is_dir() {
                continue;
            }
            for second in fs::read_dir(first.path())? {
                let second = second?;
                if !second.file_type()?.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(second.path())? {
                    let entry = entry?;
                    if !entry.file_type()?.is_file() {
                        continue;
                    }
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.len() != 64 || !name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        continue;
                    }
                    entries.push((crate::hash::from_hex(&name)?, entry.path()));
                }
            }
        }
        Ok(entries)
    }
}

fn is_empty_dir(path: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(path)?;
    Ok(entries.next().is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{DesignNode, ObjectNode};
    use crate::types::CPP_CLASS;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("store"));
        (dir, store)
    }

    fn object(name: &str) -> Node {
        Node::Object(ObjectNode {
            ty: CPP_CLASS,
            commit: [1u8; 32],
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            end_line: 2,
            content_hash: [2u8; 32],
            input_hash: [3u8; 32],
            design_hash: [4u8; 32],
            name: name.into(),
            path: "src/a.cpp".into(),
        })
    }

    #[test]
    fn put_get_roundtrip() {
        let (_dir, store) = store();
        let node = object("acme::Foo");
        let address = store.put(&node).unwrap();
        assert!(store.contains(&address));
        assert_eq!(store.get(&address).unwrap(), node);
    }

    #[test]
    fn path_is_sharded_by_hash_prefix() {
        let (_dir, store) = store();
        let address = store.put(&object("acme::Foo")).unwrap();
        let hex = to_hex(&address);
        let path = store.path_for(&address);
        assert!(path.ends_with(format!("{}/{}/{}", &hex[..2], &hex[2..4], hex)));
    }

    #[test]
    fn put_is_idempotent() {
        let (_dir, store) = store();
        let node = object("acme::Foo");
        let a = store.put(&node).unwrap();
        let b = store.put(&node).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn load_all_returns_every_node() {
        let (_dir, store) = store();
        store.put(&object("acme::Foo")).unwrap();
        store.put(&object("acme::Bar")).unwrap();
        store
            .put(&Node::Design(DesignNode {
                text: "design".into(),
            }))
            .unwrap();
        let mut nodes = store.load_all().unwrap();
        nodes.sort_by_key(|node| node.hash());
        assert_eq!(nodes.len(), 3);
    }

    #[test]
    fn corrupted_bytes_are_rejected() {
        let (_dir, store) = store();
        let address = store.put(&object("acme::Foo")).unwrap();
        let path = store.path_for(&address);
        let mut bytes = fs::read(&path).unwrap();
        bytes[10] ^= 0xff;
        fs::write(&path, bytes).unwrap();
        assert!(store.get(&address).is_err());
    }
}
