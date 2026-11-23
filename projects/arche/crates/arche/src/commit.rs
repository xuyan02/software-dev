use anyhow::{Result, bail};

use crate::codec::{Reader, Writer};
use crate::hash::{CommitHash, Hash32, ObjectHash, SymbolHash};

pub const COMMIT_MAGIC: &[u8; 4] = b"CMT2";
const COMMIT_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub parent: Option<CommitHash>,
    pub view: ObjectHash,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
    /// Current qname -> object hash snapshot for this commit.
    pub objects: Vec<(SymbolHash, ObjectHash)>,
}

impl Commit {
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.raw(COMMIT_MAGIC);
        writer.u8(COMMIT_VERSION);
        match self.parent {
            Some(parent) => writer.hash(parent.0),
            None => writer.hash(Hash32::ZERO),
        }
        writer.hash(self.view.0);
        writer.text(&self.message);
        writer.text(&self.author);
        writer.u64(self.timestamp as u64);
        writer.u32(self.objects.len() as u32);
        for (qname, object) in &self.objects {
            writer.symbol(*qname);
            writer.hash(object.0);
        }
        writer.finish()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        if reader.bytes(4)? != COMMIT_MAGIC {
            bail!("invalid commit magic");
        }
        let version = reader.u8()?;
        if version != COMMIT_VERSION {
            bail!("unsupported commit version {version}");
        }
        let parent_hash = reader.hash()?;
        let parent = (!parent_hash.is_zero()).then_some(CommitHash(parent_hash));
        let view = ObjectHash(reader.hash()?);
        let message = reader.text()?;
        let author = reader.text()?;
        let timestamp = reader.u64()? as i64;
        let count = reader.u32()? as usize;
        let mut objects = Vec::with_capacity(count);
        for _ in 0..count {
            objects.push((reader.symbol()?, ObjectHash(reader.hash()?)));
        }
        reader.finish()?;
        Ok(Self {
            parent,
            view,
            message,
            author,
            timestamp,
            objects,
        })
    }

    pub fn hash(&self) -> CommitHash {
        CommitHash(Hash32::sha256(&self.encode()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_round_trip() {
        let commit = Commit {
            parent: None,
            view: ObjectHash(Hash32::sha256(b"view")),
            message: "init".into(),
            author: "arche".into(),
            timestamp: 1,
            objects: vec![(SymbolHash::of("view"), ObjectHash(Hash32::sha256(b"view")))],
        };
        assert_eq!(Commit::decode(&commit.encode()).unwrap(), commit);
    }
}
