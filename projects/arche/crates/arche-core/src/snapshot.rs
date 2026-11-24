use anyhow::{Result, bail};

use crate::codec::{Reader, Writer};
use crate::hash::{ObjectHash, SymbolHash};

pub const SNAPSHOT_MAGIC: &[u8; 4] = b"IDX2";
const SNAPSHOT_VERSION: u8 = 2;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    entries: Vec<(SymbolHash, ObjectHash)>,
}

impl Snapshot {
    pub fn from_entries(mut entries: Vec<(SymbolHash, ObjectHash)>) -> Self {
        entries.sort_by_key(|(qname, _)| *qname);
        entries.dedup_by_key(|(qname, _)| *qname);
        Self { entries }
    }

    pub fn entries(&self) -> &[(SymbolHash, ObjectHash)] {
        &self.entries
    }

    pub fn get(&self, qname: SymbolHash) -> Option<ObjectHash> {
        self.entries
            .binary_search_by_key(&qname, |(qname, _)| *qname)
            .ok()
            .map(|index| self.entries[index].1)
    }

    pub fn insert(&mut self, qname: SymbolHash, hash: ObjectHash) {
        match self
            .entries
            .binary_search_by_key(&qname, |(qname, _)| *qname)
        {
            Ok(index) => self.entries[index].1 = hash,
            Err(index) => self.entries.insert(index, (qname, hash)),
        }
    }

    pub fn remove(&mut self, qname: SymbolHash) -> Option<ObjectHash> {
        match self
            .entries
            .binary_search_by_key(&qname, |(qname, _)| *qname)
        {
            Ok(index) => Some(self.entries.remove(index).1),
            Err(_) => None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.raw(SNAPSHOT_MAGIC);
        writer.u8(SNAPSHOT_VERSION);
        writer.u32(self.entries.len() as u32);
        for (qname, hash) in &self.entries {
            writer.symbol(*qname);
            writer.hash(hash.0);
        }
        writer.finish()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        if reader.bytes(4)? != SNAPSHOT_MAGIC {
            bail!("invalid snapshot magic");
        }
        let version = reader.u8()?;
        if version != SNAPSHOT_VERSION {
            bail!("unsupported snapshot version {version}");
        }
        let count = reader.u32()? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push((reader.symbol()?, ObjectHash(reader.hash()?)));
        }
        reader.finish()?;
        Ok(Self::from_entries(entries))
    }
}

impl FromIterator<(SymbolHash, ObjectHash)> for Snapshot {
    fn from_iter<T: IntoIterator<Item = (SymbolHash, ObjectHash)>>(iter: T) -> Self {
        Self::from_entries(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_round_trip() {
        let snapshot = Snapshot::from_entries(vec![
            (
                SymbolHash::of("technical"),
                ObjectHash(crate::hash::Hash32::sha256(b"technical")),
            ),
            (
                SymbolHash::of("requirement"),
                ObjectHash(crate::hash::Hash32::sha256(b"requirement")),
            ),
        ]);
        assert_eq!(Snapshot::decode(&snapshot.encode()).unwrap(), snapshot);
    }
}
