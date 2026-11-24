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

    pub fn find(&self, text: &str) -> Result<SymbolHash> {
        let hash = Self::hash(text);
        let path = fanout_path(&self.root, hash.0);
        if path.is_file() {
            Ok(hash)
        } else {
            Ok(SymbolHash::ZERO)
        }
    }

    pub fn intern(&self, text: &str) -> Result<SymbolHash> {
        let hash = self.find(text)?;
        if !hash.is_zero() {
            return Ok(hash);
        }
        let hash = Self::hash(text);
        let path = fanout_path(&self.root, hash.0);
        write_atomic(&path, text.as_bytes(), &self.tmp)?;
        Ok(hash)
    }

    pub fn resolve(&self, hash: SymbolHash) -> Result<String> {
        let path = fanout_path(&self.root, hash.0);
        match fs::read(&path) {
            Ok(bytes) => String::from_utf8(bytes).context("symbol is not UTF-8"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let base = self.root.parent().context("symbol store has no parent")?;
                let payload = crate::pack::find(base, crate::pack::PackKind::Symbol, hash.0)?
                    .with_context(|| format!("symbol {} not found", hash.to_hex()))?;
                String::from_utf8(payload).context("symbol is not UTF-8")
            }
            Err(error) => Err(error).with_context(|| format!("read symbol {}", hash.to_hex())),
        }
    }

    pub fn path(&self, hash: SymbolHash) -> PathBuf {
        fanout_path(&self.root, hash.0)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}
