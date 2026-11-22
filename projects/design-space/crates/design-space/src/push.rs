//! `PUSH1` bundle: one upload carrying missing records and ref commands.

use crate::model::{CommitHash, Hash32, ObjectHash};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAGIC: &[u8] = b"PUSH1\n";
const VERSION: u8 = 1;
const MAX_COMMANDS: usize = 1024;
const MAX_RECORDS: usize = 1_000_000;
const MAX_BRANCH: usize = 1024;
const MAX_RECORD: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    Object = 0,
    Doc = 1,
    Symbol = 2,
    Commit = 3,
    Pack = 4,
}

impl RecordKind {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::Object,
            1 => Self::Doc,
            2 => Self::Symbol,
            3 => Self::Commit,
            4 => Self::Pack,
            _ => return None,
        })
    }

    pub fn dir(self) -> &'static str {
        match self {
            Self::Object => "objects",
            Self::Doc => "docs",
            Self::Symbol => "symbols",
            Self::Commit => "commits",
            Self::Pack => "packs",
        }
    }

    pub fn from_relative(path: &Path) -> Option<Self> {
        let first = path.components().next()?.as_os_str().to_str()?;
        Some(match first {
            "objects" => Self::Object,
            "docs" => Self::Doc,
            "symbols" => Self::Symbol,
            "commits" => Self::Commit,
            "packs" => Self::Pack,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct PushCommand {
    pub branch: String,
    /// `None` means the branch must not exist yet.
    pub expected_old: Option<CommitHash>,
    pub new_commit: CommitHash,
    pub force: bool,
}

#[derive(Debug, Clone)]
pub struct PushRecord {
    pub kind: RecordKind,
    pub hash: Hash32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct PushBundle {
    pub commands: Vec<PushCommand>,
    pub records: Vec<PushRecord>,
}

impl PushBundle {
    pub fn new(commands: Vec<PushCommand>, records: Vec<PushRecord>) -> Self {
        Self { commands, records }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&(self.commands.len() as u32).to_le_bytes());
        for command in &self.commands {
            out.extend_from_slice(&(command.branch.len() as u16).to_le_bytes());
            out.extend_from_slice(command.branch.as_bytes());
            match command.expected_old {
                Some(commit) => {
                    out.push(1);
                    out.extend_from_slice(&commit.0.0);
                }
                None => out.push(0),
            }
            out.extend_from_slice(&command.new_commit.0.0);
            out.push(u8::from(command.force));
        }
        out.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for record in &self.records {
            out.push(record.kind as u8);
            out.extend_from_slice(&record.hash.0);
            out.extend_from_slice(&(record.bytes.len() as u64).to_le_bytes());
            out.extend_from_slice(&record.bytes);
        }
        let checksum = Sha256::digest(&out);
        out.extend_from_slice(&checksum);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < MAGIC.len() + 1 + 4 + 4 + 32 {
            bail!("push bundle is too short");
        }
        let checksum = &bytes[bytes.len() - 32..];
        let body = &bytes[..bytes.len() - 32];
        if Sha256::digest(body).as_slice() != checksum {
            bail!("push bundle checksum mismatch");
        }
        let mut reader = Reader::new(body);
        if reader.take(MAGIC.len())? != MAGIC {
            bail!("invalid push bundle magic");
        }
        let version = reader.byte()?;
        if version != VERSION {
            bail!("unsupported push bundle version: {version}");
        }
        let command_count = reader.u32()? as usize;
        if command_count > MAX_COMMANDS {
            bail!("too many ref commands: {command_count}");
        }
        let mut commands = Vec::with_capacity(command_count);
        for _ in 0..command_count {
            let branch_len = reader.u16()? as usize;
            if branch_len == 0 || branch_len > MAX_BRANCH {
                bail!("invalid branch length: {branch_len}");
            }
            let branch = String::from_utf8(reader.take(branch_len)?.to_vec())
                .context("branch is not UTF-8")?;
            let expected_old = match reader.byte()? {
                0 => None,
                1 => Some(CommitHash(reader.hash()?)),
                value => bail!("invalid expected-old flag: {value}"),
            };
            let new_commit = CommitHash(reader.hash()?);
            let force = reader.byte()? != 0;
            commands.push(PushCommand {
                branch,
                expected_old,
                new_commit,
                force,
            });
        }
        let record_count = reader.u32()? as usize;
        if record_count > MAX_RECORDS {
            bail!("too many records: {record_count}");
        }
        let mut records = Vec::with_capacity(record_count);
        for _ in 0..record_count {
            let kind = RecordKind::from_byte(reader.byte()?).context("invalid push record kind")?;
            let hash = reader.hash()?;
            let len = reader.u64()? as usize;
            if len > MAX_RECORD {
                bail!("record is too large: {len}");
            }
            let data = reader.take(len)?.to_vec();
            records.push(PushRecord {
                kind,
                hash,
                bytes: data,
            });
        }
        if !reader.is_empty() {
            bail!("trailing bytes in push bundle");
        }
        Ok(Self { commands, records })
    }
}

pub fn record_path(kind: RecordKind, hash: Hash32) -> PathBuf {
    let hex = hash.to_hex();
    Path::new(kind.dir())
        .join(&hex[0..2])
        .join(&hex[2..4])
        .join(hex)
}

pub fn commit_hash(bytes: &[u8]) -> Result<CommitHash> {
    let hash = crate::format::hash_bytes(bytes);
    Ok(CommitHash(Hash32(hash.0)))
}

pub fn object_hash(bytes: &[u8]) -> Result<ObjectHash> {
    let hash = crate::format::hash_bytes(bytes);
    Ok(ObjectHash(Hash32(hash.0)))
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("push bundle length overflow")?;
        if end > self.bytes.len() {
            bail!("truncated push bundle");
        }
        let out = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(out)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn hash(&mut self) -> Result<Hash32> {
        let bytes = self.take(32)?;
        let mut hash = [0u8; 32];
        hash.copy_from_slice(bytes);
        Ok(Hash32(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_bundle() {
        let bundle = PushBundle::new(
            vec![PushCommand {
                branch: "main".to_owned(),
                expected_old: None,
                new_commit: CommitHash(Hash32([3; 32])),
                force: false,
            }],
            vec![PushRecord {
                kind: RecordKind::Doc,
                hash: Hash32([1; 32]),
                bytes: b"hello".to_vec(),
            }],
        );
        let decoded = PushBundle::decode(&bundle.encode()).unwrap();
        assert_eq!(decoded.commands.len(), 1);
        assert_eq!(decoded.commands[0].branch, "main");
        assert_eq!(decoded.records[0].bytes, b"hello");
    }

    #[test]
    fn rejects_tampered_bundle() {
        let bundle = PushBundle::new(Vec::new(), Vec::new());
        let mut bytes = bundle.encode();
        bytes[0] ^= 1;
        assert!(PushBundle::decode(&bytes).is_err());
    }
}
