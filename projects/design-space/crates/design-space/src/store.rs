//! Content-addressed storage for the format.
//!
//! This is the low-level disk layer. It stores:
//!
//! ```text
//! <root>/objects/<aa>/<bb>/<object-hash>
//! <root>/commits/<aa>/<bb>/<commit-hash>
//! <root>/docs/<aa>/<bb>/<doc-hash>
//! <root>/symbols/<aa>/<bb>/<symbol-hash>
//! <root>/tmp/
//! ```
//!
//! Every write goes to `tmp/` first and is published with an atomic rename on
//! the same filesystem.

use crate::format::{
    self, FormatError, commit_hash, decode_commit_checked, decode_object_checked, doc_hash,
    encode_commit, encode_object, object_hash, symbol_hash, validate_commit,
};
use crate::model::{Commit, CommitHash, DesignObject, DocHash, Hash32, ObjectHash, SymbolHash};
use crate::pack::{self, PackRecord};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("format: {0}")]
    Format(#[from] FormatError),
    #[error("zero hash is not a valid address")]
    ZeroHash,
    #[error("symbol text must be non-empty and must not contain NUL")]
    InvalidSymbol,
    #[error("pack: {0}")]
    Pack(String),
    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// Create/open a store rooted at the workspace metadata directory (`.ds`).
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        for dir in ["objects", "commits", "docs", "symbols", "packs", "tmp"] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put_object(&self, object: &DesignObject) -> Result<ObjectHash> {
        let bytes = encode_object(object)?;
        let hash = object_hash(object)?;
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        self.write_immutable("objects", hash.0, &bytes)?;
        Ok(hash)
    }

    pub fn get_object(&self, hash: ObjectHash) -> Result<DesignObject> {
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        let bytes = self.read("objects", hash.0)?;
        Ok(decode_object_checked(&bytes, hash)?)
    }

    pub fn put_commit(&self, commit: &Commit) -> Result<CommitHash> {
        validate_commit(commit)?;
        let bytes = encode_commit(commit);
        let hash = commit_hash(commit);
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        self.write_immutable("commits", hash.0, &bytes)?;
        Ok(hash)
    }

    pub fn get_commit(&self, hash: CommitHash) -> Result<Commit> {
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        let bytes = self.read("commits", hash.0)?;
        Ok(decode_commit_checked(&bytes, hash)?)
    }

    pub fn put_doc(&self, text: &str) -> Result<DocHash> {
        let hash = doc_hash(text);
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        self.write_immutable("docs", hash.0, text.as_bytes())?;
        Ok(hash)
    }

    pub fn get_doc(&self, hash: DocHash) -> Result<String> {
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        let bytes = self.read("docs", hash.0)?;
        verify_text_hash(&bytes, hash.0, "doc")?;
        String::from_utf8(bytes).map_err(|_| StoreError::Format(FormatError::InvalidUtf8("doc")))
    }

    pub fn put_symbol(&self, text: &str) -> Result<SymbolHash> {
        if text.is_empty() || text.as_bytes().contains(&0) {
            return Err(StoreError::InvalidSymbol);
        }
        let hash = symbol_hash(text);
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        self.write_immutable("symbols", hash.0, text.as_bytes())?;
        Ok(hash)
    }

    pub fn get_symbol(&self, hash: SymbolHash) -> Result<String> {
        if hash.is_zero() {
            return Err(StoreError::ZeroHash);
        }
        let bytes = self.read("symbols", hash.0)?;
        verify_text_hash(&bytes, hash.0, "symbol")?;
        String::from_utf8(bytes).map_err(|_| StoreError::Format(FormatError::InvalidUtf8("symbol")))
    }

    fn write_immutable(&self, kind: &str, hash: Hash32, bytes: &[u8]) -> Result<()> {
        let target = self.fanout(kind, hash);
        if target.is_file() {
            return Ok(());
        }
        let parent = target
            .parent()
            .expect("hashed path always has a parent directory");
        fs::create_dir_all(parent)?;

        let nonce = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = self.root.join("tmp").join(format!(
            "{}-{}-{}-{}.tmp",
            kind,
            hash.to_hex(),
            std::process::id(),
            nonce
        ));

        {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }

        match fs::rename(&temp, &target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let _ = fs::remove_file(&temp);
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(&temp);
                Err(error.into())
            }
        }
    }

    fn read(&self, kind: &str, hash: Hash32) -> Result<Vec<u8>> {
        let path = self.fanout(kind, hash);
        let mut file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if let Some(bytes) = self.read_packed(kind, hash)? {
                    return Ok(bytes);
                }
                return Err(error.into());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn read_packed(&self, kind: &str, hash: Hash32) -> Result<Option<Vec<u8>>> {
        let Some(record_type) = pack::record_type_for_dir(kind) else {
            return Ok(None);
        };
        let dir = self.root.join("packs");
        let mut packs = Vec::new();
        collect_files(&dir, &mut packs)?;
        packs.sort();
        for path in packs {
            if path.extension().and_then(|extension| extension.to_str()) != Some("pack") {
                continue;
            }
            let bytes = fs::read(&path)?;
            let found = pack::read_record(&bytes, record_type, hash)
                .map_err(|error| StoreError::Pack(format!("{error:#}")))?;
            if let Some(payload) = found {
                return Ok(Some(payload));
            }
        }
        Ok(None)
    }

    /// Expand every pack into loose files and remove the pack files.
    pub fn expand_packs(&self) -> Result<()> {
        let dir = self.root.join("packs");
        let mut packs = Vec::new();
        collect_files(&dir, &mut packs)?;
        packs.retain(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("pack")
        });
        packs.sort();
        for path in &packs {
            let bytes = fs::read(path)?;
            let records = pack::decode_pack(&bytes)
                .map_err(|error| StoreError::Pack(format!("{error:#}")))?;
            for record in records {
                let record_dir = pack::dir_for_record_type(record.record_type)
                    .expect("decoded record types are valid");
                self.write_immutable(record_dir, record.hash, &record.bytes)?;
            }
        }
        for path in packs {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Pack every loose object/doc/symbol/commit into `packs/<hash>.pack`.
    pub fn repack(&self) -> Result<Hash32> {
        self.expand_packs()?;
        let mut records = Vec::new();
        for kind in ["objects", "docs", "symbols", "commits"] {
            let record_type = pack::record_type_for_dir(kind).expect("known store kind");
            let dir = self.root.join(kind);
            let mut files = Vec::new();
            collect_files(&dir, &mut files)?;
            for path in files {
                let Some(hash) = loose_hash(&dir, &path) else {
                    continue;
                };
                let bytes = fs::read(&path)?;
                pack::validate_record(record_type, hash, &bytes)
                    .map_err(|error| StoreError::Pack(format!("{error:#}")))?;
                records.push(PackRecord {
                    record_type,
                    hash,
                    bytes,
                });
            }
        }
        if records.is_empty() {
            return Err(StoreError::Pack("store has nothing to pack".to_owned()));
        }
        let (bytes, hash) =
            pack::encode_pack(records).map_err(|error| StoreError::Pack(format!("{error:#}")))?;
        let target = self
            .root
            .join("packs")
            .join(format!("{}.pack", hash.to_hex()));
        if !target.is_file() {
            let temp = self.root.join("tmp").join(format!(
                "pack-{}-{}.tmp",
                std::process::id(),
                TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&temp, &bytes)?;
            fs::rename(&temp, &target)?;
        }
        for kind in ["objects", "docs", "symbols", "commits"] {
            let dir = self.root.join(kind);
            let mut files = Vec::new();
            collect_files(&dir, &mut files)?;
            for path in files {
                if loose_hash(&dir, &path).is_some() {
                    fs::remove_file(path)?;
                }
            }
        }
        Ok(hash)
    }

    fn fanout(&self, kind: &str, hash: Hash32) -> PathBuf {
        let hex = hash.to_hex();
        self.root
            .join(kind)
            .join(&hex[0..2])
            .join(&hex[2..4])
            .join(hex)
    }
}

fn verify_text_hash(bytes: &[u8], expected: Hash32, what: &str) -> Result<()> {
    let actual = format::hash_bytes(bytes);
    if actual != expected {
        return Err(StoreError::HashMismatch {
            path: what.into(),
            expected: expected.to_hex(),
            actual: actual.to_hex(),
        });
    }
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

fn loose_hash(dir: &Path, path: &Path) -> Option<Hash32> {
    let relative = path.strip_prefix(dir).ok()?;
    let components = relative.components().collect::<Vec<_>>();
    if components.len() != 3
        || !components
            .iter()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let (Component::Normal(aa), Component::Normal(bb), Component::Normal(name)) =
        (components[0], components[1], components[2])
    else {
        return None;
    };
    let (aa, bb, name) = (aa.to_str()?, bb.to_str()?, name.to_str()?);
    let hash = Hash32::from_hex(name)?;
    let hex = hash.to_hex();
    (aa == &hex[..2] && bb == &hex[2..4]).then_some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    use tempfile::tempdir;

    fn symbol_store(store: &Store, text: &str) -> SymbolHash {
        store.put_symbol(text).unwrap()
    }

    fn view(store: &Store) -> DesignObject {
        let files = store
            .put_object(&DesignObject::new(
                Kind::Directory,
                ObjectPayload::Directory(DirectoryPayload {
                    qualified_name: symbol_store(store, "files"),
                    parent: ParentRef::View,
                    source: None,
                    design_doc: DocHash::ZERO,
                    children: vec![],
                }),
            ))
            .unwrap();
        let namespaces = store
            .put_object(&DesignObject::new(
                Kind::Namespace,
                ObjectPayload::Namespace(NamespacePayload {
                    qualified_name: symbol_store(store, "namespaces"),
                    parent: ParentRef::View,
                    source: None,
                    content_hash: Hash32::ZERO,
                    design_doc: DocHash::ZERO,
                    children: vec![],
                }),
            ))
            .unwrap();
        DesignObject::new(
            Kind::View,
            ObjectPayload::View(ViewPayload {
                source_commit: GitOid::ZERO,
                files,
                namespaces,
                design: ObjectHash::ZERO,
            }),
        )
    }

    #[test]
    fn stores_and_verifies_objects_docs_symbols_commits() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let object = view(&store);
        let hash = store.put_object(&object).unwrap();
        assert_eq!(store.get_object(hash).unwrap(), object);

        let doc = store.put_doc("design text").unwrap();
        assert_eq!(store.get_doc(doc).unwrap(), "design text");

        let sym = store.put_symbol("namespaces/app/Auth").unwrap();
        assert_eq!(store.get_symbol(sym).unwrap(), "namespaces/app/Auth");

        let commit = Commit {
            view: hash,
            parent: CommitHash::ZERO,
            message: "initial".into(),
            author: "agent".into(),
            timestamp: 1,
        };
        let commit_hash = store.put_commit(&commit).unwrap();
        assert_eq!(store.get_commit(commit_hash).unwrap(), commit);
    }

    #[test]
    fn rejects_invalid_symbols_and_zero_hashes() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        assert!(matches!(
            store.put_symbol(""),
            Err(StoreError::InvalidSymbol)
        ));
        assert!(matches!(
            store.put_symbol("a\0b"),
            Err(StoreError::InvalidSymbol)
        ));
        assert!(matches!(
            store.get_object(ObjectHash::ZERO),
            Err(StoreError::ZeroHash)
        ));
    }

    #[test]
    fn detects_corrupted_docs() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let doc = store.put_doc("hello").unwrap();
        let path = store.fanout("docs", doc.0);
        std::fs::write(&path, b"tampered").unwrap();
        assert!(matches!(
            store.get_doc(doc),
            Err(StoreError::HashMismatch { .. })
        ));
    }

    #[test]
    fn repack_and_expand_roundtrip() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path().join(".ds")).unwrap();
        let object = view(&store);
        let object_hash = store.put_object(&object).unwrap();
        let doc = store.put_doc("design text").unwrap();
        let symbol = store.put_symbol("namespaces/app").unwrap();
        let commit = Commit {
            view: object_hash,
            parent: CommitHash::ZERO,
            message: "pack".into(),
            author: "agent".into(),
            timestamp: 1,
        };
        let commit_hash = store.put_commit(&commit).unwrap();

        let pack_hash = store.repack().unwrap();
        assert_eq!(store.get_object(object_hash).unwrap(), object);
        assert_eq!(store.get_doc(doc).unwrap(), "design text");
        assert_eq!(store.get_symbol(symbol).unwrap(), "namespaces/app");
        assert_eq!(store.get_commit(commit_hash).unwrap(), commit);
        assert!(!store.fanout("objects", object_hash.0).exists());
        let pack_path = store
            .root()
            .join("packs")
            .join(format!("{}.pack", pack_hash.to_hex()));
        assert!(pack_path.is_file());

        store.expand_packs().unwrap();
        assert!(store.fanout("objects", object_hash.0).is_file());
        assert!(!pack_path.exists());
        assert_eq!(store.get_doc(doc).unwrap(), "design text");
    }
}
