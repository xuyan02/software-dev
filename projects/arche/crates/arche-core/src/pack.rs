use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::hash::{CommitHash, Hash32};

pub const PACK_MAGIC: &[u8; 4] = b"PAK2";
const PACK_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Object,
    Symbol,
    Commit,
    Pack,
}

impl PackKind {
    fn tag(self) -> u8 {
        match self {
            Self::Object => 0,
            Self::Symbol => 1,
            Self::Commit => 2,
            Self::Pack => 3,
        }
    }

    fn from_tag(tag: u8) -> Result<Self> {
        Ok(match tag {
            0 => Self::Object,
            1 => Self::Symbol,
            2 => Self::Commit,
            3 => Self::Pack,
            _ => bail!("unknown pack record kind {tag}"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PackRecord {
    pub kind: PackKind,
    pub hash: Hash32,
    pub payload: Vec<u8>,
}

pub fn encode(records: &[PackRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(PACK_MAGIC);
    bytes.push(PACK_VERSION);
    bytes.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for record in records {
        bytes.push(record.kind.tag());
        bytes.extend_from_slice(record.hash.as_bytes());
        bytes.extend_from_slice(&(record.payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&record.payload);
    }
    bytes
}

pub fn decode(bytes: &[u8]) -> Result<Vec<PackRecord>> {
    if bytes.len() < 9 {
        bail!("pack is too short");
    }
    if &bytes[..4] != PACK_MAGIC {
        bail!("invalid pack magic");
    }
    if bytes[4] != PACK_VERSION {
        bail!("unsupported pack version {}", bytes[4]);
    }
    let count = u32::from_le_bytes(bytes[5..9].try_into().expect("four bytes")) as usize;
    let mut position = 9;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let header = bytes
            .get(position..position + 37)
            .context("unexpected end of pack record")?;
        let kind = PackKind::from_tag(header[0])?;
        let hash = Hash32::from_bytes(header[1..33].try_into().expect("32 bytes"));
        let size = u32::from_le_bytes(header[33..37].try_into().expect("four bytes")) as usize;
        position += 37;
        let payload = bytes
            .get(position..position + size)
            .context("unexpected end of pack payload")?
            .to_vec();
        position += size;
        records.push(PackRecord {
            kind,
            hash,
            payload,
        });
    }
    if position != bytes.len() {
        bail!("pack has trailing bytes");
    }
    Ok(records)
}

#[derive(Debug, Clone)]
pub struct RefCommand {
    pub branch: String,
    pub expected: CommitHash,
    pub new: CommitHash,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct PushBundle {
    pub records: Vec<PackRecord>,
    pub refs: Vec<RefCommand>,
}

pub fn encode_bundle(bundle: &PushBundle) -> Vec<u8> {
    let pack = encode(&bundle.records);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PSH2");
    bytes.push(1);
    bytes.extend_from_slice(&(pack.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&pack);
    bytes.extend_from_slice(&(bundle.refs.len() as u32).to_le_bytes());
    for command in &bundle.refs {
        bytes.extend_from_slice(&(command.branch.len() as u32).to_le_bytes());
        bytes.extend_from_slice(command.branch.as_bytes());
        bytes.extend_from_slice(command.expected.0.as_bytes());
        bytes.extend_from_slice(command.new.0.as_bytes());
        bytes.push(u8::from(command.force));
    }
    bytes
}

pub fn decode_bundle(bytes: &[u8]) -> Result<PushBundle> {
    if bytes.len() < 9 || &bytes[..4] != b"PSH2" {
        bail!("invalid push bundle");
    }
    if bytes[4] != 1 {
        bail!("unsupported push bundle version {}", bytes[4]);
    }
    let pack_len = u32::from_le_bytes(bytes[5..9].try_into().expect("four bytes")) as usize;
    let pack_end = 9usize
        .checked_add(pack_len)
        .context("push bundle size overflow")?;
    let records = decode(bytes.get(9..pack_end).context("push bundle is truncated")?)?;
    let mut position = pack_end;
    let refs_len = read_u32(bytes, &mut position)? as usize;
    let mut refs = Vec::with_capacity(refs_len);
    for _ in 0..refs_len {
        let len = read_u32(bytes, &mut position)? as usize;
        let branch = String::from_utf8(
            bytes
                .get(position..position + len)
                .context("push bundle is truncated")?
                .to_vec(),
        )?;
        position += len;
        let expected = read_hash(bytes, &mut position)?;
        let new = read_hash(bytes, &mut position)?;
        let force = *bytes.get(position).context("push bundle is truncated")? != 0;
        position += 1;
        refs.push(RefCommand {
            branch,
            expected: CommitHash(expected),
            new: CommitHash(new),
            force,
        });
    }
    if position != bytes.len() {
        bail!("push bundle has trailing bytes");
    }
    Ok(PushBundle { records, refs })
}

fn read_u32(bytes: &[u8], position: &mut usize) -> Result<u32> {
    let value = bytes
        .get(*position..*position + 4)
        .context("push bundle is truncated")?;
    *position += 4;
    Ok(u32::from_le_bytes(value.try_into().expect("four bytes")))
}

fn read_hash(bytes: &[u8], position: &mut usize) -> Result<Hash32> {
    let value = bytes
        .get(*position..*position + 32)
        .context("push bundle is truncated")?;
    *position += 32;
    Ok(Hash32::from_bytes(value.try_into().expect("32 bytes")))
}

pub fn find(dir: &Path, kind: PackKind, hash: Hash32) -> Result<Option<Vec<u8>>> {
    let packs = dir.join("packs");
    if !packs.is_dir() {
        return Ok(None);
    }
    for entry in fs::read_dir(packs)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let bytes = fs::read(entry.path())?;
        for record in decode(&bytes)? {
            if record.kind == kind && record.hash == hash {
                return Ok(Some(record.payload));
            }
        }
    }
    Ok(None)
}
