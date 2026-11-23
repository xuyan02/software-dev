use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::commit::Commit;
use crate::hash::{CommitHash, Hash32, ObjectHash};
use crate::object::Object;
use crate::util::{fanout_path, list_files, write_atomic};

#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
    tmp: PathBuf,
}

impl ObjectStore {
    pub fn new(root: PathBuf, tmp: PathBuf) -> Self {
        Self { root, tmp }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn write(&self, object: &Object) -> Result<ObjectHash> {
        let bytes = object.encode();
        let hash = ObjectHash(Hash32::sha256(&bytes));
        let path = fanout_path(&self.root, hash.0);
        write_atomic(&path, &bytes, &self.tmp)?;
        Ok(hash)
    }

    pub fn read(&self, hash: ObjectHash) -> Result<Object> {
        let path = fanout_path(&self.root, hash.0);
        let bytes = fs::read(&path).with_context(|| format!("read object {}", hash.to_hex()))?;
        let object =
            Object::decode(&bytes).with_context(|| format!("decode object {}", hash.to_hex()))?;
        let actual = ObjectHash(Hash32::sha256(&bytes));
        if actual != hash {
            anyhow::bail!(
                "object {} has content hash {}",
                hash.to_hex(),
                actual.to_hex()
            );
        }
        Ok(object)
    }

    pub fn all(&self) -> Result<Vec<(ObjectHash, Object)>> {
        let mut objects = Vec::new();
        for path in list_files(&self.root)? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let Ok(hash) = ObjectHash::from_hex(name) else {
                continue;
            };
            let bytes =
                fs::read(&path).with_context(|| format!("read object file {}", path.display()))?;
            let object = Object::decode(&bytes)
                .with_context(|| format!("decode object file {}", path.display()))?;
            objects.push((hash, object));
        }
        objects.sort_by_key(|(hash, _)| *hash);
        Ok(objects)
    }
}

#[derive(Debug, Clone)]
pub struct CommitStore {
    root: PathBuf,
    tmp: PathBuf,
}

impl CommitStore {
    pub fn new(root: PathBuf, tmp: PathBuf) -> Self {
        Self { root, tmp }
    }

    pub fn write(&self, commit: &Commit) -> Result<CommitHash> {
        let bytes = commit.encode();
        let hash = CommitHash(Hash32::sha256(&bytes));
        let path = fanout_path(&self.root, hash.0);
        write_atomic(&path, &bytes, &self.tmp)?;
        Ok(hash)
    }

    pub fn read(&self, hash: CommitHash) -> Result<Commit> {
        let path = fanout_path(&self.root, hash.0);
        let bytes = fs::read(&path).with_context(|| format!("read commit {}", hash.to_hex()))?;
        let commit =
            Commit::decode(&bytes).with_context(|| format!("decode commit {}", hash.to_hex()))?;
        Ok(commit)
    }
}
