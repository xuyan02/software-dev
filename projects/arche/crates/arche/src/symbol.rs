use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::hash::SymbolHash;
use crate::util::{fanout_path, write_atomic};

#[derive(Debug, Clone)]
pub struct SymbolStore {
    root: PathBuf,
    tmp: PathBuf,
}

impl SymbolStore {
    pub fn new(root: PathBuf, tmp: PathBuf) -> Self {
        Self { root, tmp }
    }

    pub fn hash(text: &str) -> SymbolHash {
        SymbolHash::of(text)
    }

    pub fn intern(&self, text: &str) -> Result<SymbolHash> {
        let hash = Self::hash(text);
        let path = fanout_path(&self.root, hash.0);
        write_atomic(&path, text.as_bytes(), &self.tmp)?;
        Ok(hash)
    }

    pub fn resolve(&self, hash: SymbolHash) -> Result<String> {
        let path = fanout_path(&self.root, hash.0);
        fs::read_to_string(&path).with_context(|| format!("read symbol {}", hash.to_hex()))
    }

    pub fn path(&self, hash: SymbolHash) -> PathBuf {
        fanout_path(&self.root, hash.0)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
