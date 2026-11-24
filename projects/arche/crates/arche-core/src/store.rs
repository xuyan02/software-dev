use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::commit::Commit;
use crate::data::{InputObject, OutputObject};
use crate::hash::{CommitHash, Hash32, ObjectHash, SymbolHash};
use crate::object::{Object, ObjectKind};

/// Magic of a loose object file. Pack readers do not add one per object.
const LOOSE_MAGIC: &[u8; 4] = b"DSO2";
use crate::util::{fanout_path, list_files, write_atomic};

/// Storage for design objects and symbols.
pub trait Store {
    fn open_object(&self, hash: ObjectHash) -> Result<Box<dyn InputObject>>;

    fn create_object(&self, version: u8, kind: ObjectKind) -> Result<Box<dyn OutputObject + '_>>;

    fn read_symbol(&self, hash: SymbolHash) -> Result<String>;

    /// Return the symbol hash, or `SymbolHash::ZERO` when the text is absent.
    fn find_symbol(&self, text: &str) -> Result<SymbolHash>;

    fn intern_symbol(&self, text: &str) -> Result<SymbolHash>;
}

struct LooseInputObject {
    bytes: Vec<u8>,
    position: usize,
    version: u8,
    kind: ObjectKind,
}

impl LooseInputObject {
    fn from_record(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 6 {
            bail!("object record is too short");
        }
        if &bytes[..4] != LOOSE_MAGIC {
            bail!("invalid loose object magic");
        }
        let version = bytes[4];
        let kind = ObjectKind::from_tag(bytes[5])?;
        Ok(Self {
            bytes,
            position: 6,
            version,
            kind,
        })
    }

    fn from_payload(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < 2 {
            bail!("packed object record is too short");
        }
        let version = bytes[0];
        let kind = ObjectKind::from_tag(bytes[1])?;
        Ok(Self {
            bytes,
            position: 2,
            version,
            kind,
        })
    }

    fn take(&mut self, len: usize) -> Result<&[u8]> {
        let end = self
            .position
            .checked_add(len)
            .context("object record length overflow")?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .context("unexpected end of object record")?;
        self.position = end;
        Ok(bytes)
    }
}

impl InputObject for LooseInputObject {
    fn version(&self) -> u8 {
        self.version
    }

    fn kind(&self) -> ObjectKind {
        self.kind
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn u128(&mut self) -> Result<u128> {
        Ok(u128::from_le_bytes(
            self.take(16)?.try_into().expect("sixteen bytes"),
        ))
    }

    fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }

    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn i128(&mut self) -> Result<i128> {
        Ok(i128::from_le_bytes(
            self.take(16)?.try_into().expect("sixteen bytes"),
        ))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn hash(&mut self) -> Result<Hash32> {
        Ok(Hash32::from_bytes(
            self.take(32)?.try_into().expect("thirty-two bytes"),
        ))
    }

    fn object(&mut self) -> Result<ObjectHash> {
        Ok(ObjectHash(self.hash()?))
    }

    fn symbol(&mut self) -> Result<SymbolHash> {
        Ok(SymbolHash(self.hash()?))
    }

    fn text(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        Ok(std::str::from_utf8(bytes)?.to_owned())
    }

    fn objects(&mut self) -> Result<Vec<ObjectHash>> {
        let count = self.u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.object()?);
        }
        Ok(values)
    }

    fn symbols(&mut self) -> Result<Vec<SymbolHash>> {
        let count = self.u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.symbol()?);
        }
        Ok(values)
    }

    fn finish(&self) -> Result<()> {
        if self.position != self.bytes.len() {
            bail!("trailing bytes in object record");
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct LooseObjectStore {
    root: PathBuf,
    tmp: PathBuf,
}

impl LooseObjectStore {
    pub fn new(root: PathBuf, tmp: PathBuf) -> Self {
        Self { root, tmp }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(&self, hash: ObjectHash) -> Result<Box<dyn InputObject>> {
        let path = fanout_path(&self.root, hash.0);
        match fs::read(&path) {
            Ok(bytes) => Ok(Box::new(LooseInputObject::from_record(bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let base = self.root.parent().context("object store has no parent")?;
                let payload = crate::pack::find(base, crate::pack::PackKind::Object, hash.0)?
                    .with_context(|| format!("object {} not found", hash.to_hex()))?;
                Ok(Box::new(LooseInputObject::from_payload(payload)?))
            }
            Err(error) => Err(error).with_context(|| format!("read object {}", hash.to_hex())),
        }
    }

    pub fn create(&self, version: u8, kind: ObjectKind) -> Box<dyn OutputObject + '_> {
        Box::new(LooseOutputObject {
            objects: self,
            version,
            kind,
            bytes: Vec::new(),
        })
    }

    pub fn all(&self, store: &dyn Store) -> Result<Vec<(ObjectHash, Object)>> {
        let mut objects = Vec::new();
        for path in list_files(&self.root)? {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let Ok(hash) = ObjectHash::from_hex(name) else {
                continue;
            };
            let object = Object::load(store, hash)
                .with_context(|| format!("decode object file {}", path.display()))?;
            objects.push((hash, object));
        }
        objects.sort_by_key(|(hash, _)| *hash);
        Ok(objects)
    }

    fn write_record(&self, hash: ObjectHash, bytes: &[u8]) -> Result<()> {
        let path = fanout_path(&self.root, hash.0);
        write_atomic(&path, bytes, &self.tmp)
    }
}

struct LooseOutputObject<'a> {
    objects: &'a LooseObjectStore,
    version: u8,
    kind: ObjectKind,
    bytes: Vec<u8>,
}

impl OutputObject for LooseOutputObject<'_> {
    fn version(&self) -> u8 {
        self.version
    }

    fn kind(&self) -> ObjectKind {
        self.kind
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u128(&mut self, value: u128) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i8(&mut self, value: i8) {
        self.u8(value as u8);
    }

    fn i16(&mut self, value: i16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i32(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i128(&mut self, value: i128) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn hash(&mut self, value: Hash32) {
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn object(&mut self, value: ObjectHash) {
        self.hash(value.0);
    }

    fn symbol(&mut self, value: SymbolHash) {
        self.hash(value.0);
    }

    fn text(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    fn objects(&mut self, values: &[ObjectHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.object(*value);
        }
    }

    fn symbols(&mut self, values: &[SymbolHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.symbol(*value);
        }
    }

    fn commit(self: Box<Self>, object_hash: ObjectHash) -> Result<()> {
        let mut record = Vec::with_capacity(6 + self.bytes.len());
        record.extend_from_slice(LOOSE_MAGIC);
        record.push(self.version);
        record.push(self.kind.tag());
        record.extend_from_slice(&self.bytes);
        self.objects.write_record(object_hash, &record)
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
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let base = self.root.parent().context("commit store has no parent")?;
                crate::pack::find(base, crate::pack::PackKind::Commit, hash.0)?
                    .with_context(|| format!("commit {} not found", hash.to_hex()))?
            }
            Err(error) => {
                return Err(error).with_context(|| format!("read commit {}", hash.to_hex()));
            }
        };
        let commit =
            Commit::decode(&bytes).with_context(|| format!("decode commit {}", hash.to_hex()))?;
        Ok(commit)
    }
}
