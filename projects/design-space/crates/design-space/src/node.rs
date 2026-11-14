//! Immutable, content-addressed nodes.

use crate::error::{Error, Result};
use crate::hash::{Hash, ZERO_HASH, hash};
use crate::types::{self, Layout};

pub const NODE_VERSION: u8 = 1;

/// Every node starts with `version u8 | reserved [u8; 3] | type u32`.
pub const HEADER_SIZE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Object(ObjectNode),
    Design(DesignNode),
    Index(IndexNode),
    Version(VersionNode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectNode {
    pub ty: u32,
    pub commit: [u8; 32],
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
    pub content_hash: Hash,
    pub input_hash: Hash,
    pub design_hash: Hash,
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignNode {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexNode {
    pub ty: u32,
    /// `true` for a leaf whose entries point at object nodes, `false` for an
    /// internal node whose entries point at other index nodes.
    pub leaf: bool,
    pub entries: Vec<IndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: Vec<u8>,
    pub hash: Hash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionNode {
    pub parent: [u8; 32],
    pub root: Hash,
    pub source_commit: [u8; 32],
    pub pipeline: String,
    pub author: String,
    pub message: String,
    pub timestamp: u64,
}

impl Node {
    pub fn ty(&self) -> u32 {
        match self {
            Self::Object(node) => node.ty,
            Self::Design(_) => types::T_DESIGN,
            Self::Index(node) => node.ty,
            Self::Version(_) => types::T_VERSION,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(NODE_VERSION);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&self.ty().to_le_bytes());

        match self {
            Self::Object(node) => {
                out.extend_from_slice(&node.commit);
                out.extend_from_slice(&node.start_byte.to_le_bytes());
                out.extend_from_slice(&node.end_byte.to_le_bytes());
                out.extend_from_slice(&node.start_line.to_le_bytes());
                out.extend_from_slice(&node.end_line.to_le_bytes());
                out.extend_from_slice(&node.content_hash);
                out.extend_from_slice(&node.input_hash);
                out.extend_from_slice(&node.design_hash);
                push_str(&mut out, &node.name);
                push_str(&mut out, &node.path);
            }
            Self::Design(node) => {
                out.extend_from_slice(&(node.text.len() as u64).to_le_bytes());
                out.extend_from_slice(node.text.as_bytes());
            }
            Self::Index(node) => {
                out.push(u8::from(node.leaf));
                out.extend_from_slice(&[0, 0, 0]);
                out.extend_from_slice(&(node.entries.len() as u32).to_le_bytes());
                for entry in &node.entries {
                    out.extend_from_slice(&(entry.key.len() as u32).to_le_bytes());
                    out.extend_from_slice(&entry.key);
                    out.extend_from_slice(&entry.hash);
                }
            }
            Self::Version(node) => {
                out.extend_from_slice(&node.parent);
                out.extend_from_slice(&node.root);
                out.extend_from_slice(&node.source_commit);
                push_str(&mut out, &node.pipeline);
                push_str(&mut out, &node.author);
                push_str(&mut out, &node.message);
                out.extend_from_slice(&node.timestamp.to_le_bytes());
            }
        }

        out
    }

    pub fn hash(&self) -> Hash {
        hash(&self.encode())
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Object(node) => {
                if types::layout_of(node.ty) != Layout::Object {
                    return Err(Error::InvalidNode(format!(
                        "type 0x{:08x} cannot be an object",
                        node.ty
                    )));
                }
            }
            Self::Design(_) => {}
            Self::Index(node) => {
                if types::layout_of(node.ty) != Layout::Index {
                    return Err(Error::InvalidNode(format!(
                        "type 0x{:08x} cannot be an index",
                        node.ty
                    )));
                }
                node.validate()?;
            }
            Self::Version(_) => {}
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let version = reader.u8()?;
        if version != NODE_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        reader.skip(3)?;
        let ty = reader.u32()?;

        let node = match types::layout_of(ty) {
            Layout::Object => Self::Object(ObjectNode {
                ty,
                commit: reader.bytes32()?,
                start_byte: reader.u64()?,
                end_byte: reader.u64()?,
                start_line: reader.u32()?,
                end_line: reader.u32()?,
                content_hash: reader.hash()?,
                input_hash: reader.hash()?,
                design_hash: reader.hash()?,
                name: reader.string()?,
                path: reader.string()?,
            }),
            Layout::Design => {
                let length = reader.u64()?;
                let text_bytes = reader.take(length as usize)?;
                let text = String::from_utf8(text_bytes.to_vec())
                    .map_err(|error| Error::InvalidNode(error.to_string()))?;
                Self::Design(DesignNode { text })
            }
            Layout::Index => {
                let leaf = reader.u8()? != 0;
                reader.skip(3)?;
                let count = reader.u32()? as usize;
                let mut entries = Vec::with_capacity(count.min(1024));
                for _ in 0..count {
                    let key_len = reader.u32()? as usize;
                    let key = reader.take(key_len)?.to_vec();
                    let hash = reader.hash()?;
                    entries.push(IndexEntry { key, hash });
                }
                Self::Index(IndexNode { ty, leaf, entries })
            }
            Layout::Version => Self::Version(VersionNode {
                parent: reader.bytes32()?,
                root: reader.hash()?,
                source_commit: reader.bytes32()?,
                pipeline: reader.string()?,
                author: reader.string()?,
                message: reader.string()?,
                timestamp: reader.u64()?,
            }),
        };

        if !reader.is_empty() {
            return Err(Error::InvalidNode(format!(
                "{} trailing bytes",
                reader.remaining()
            )));
        }

        node.validate()?;
        Ok(node)
    }
}

impl IndexNode {
    /// Build a node, ordering entries by key and rejecting duplicates.
    pub fn new(ty: u32, leaf: bool, mut entries: Vec<IndexEntry>) -> Result<Self> {
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        let node = Self { ty, leaf, entries };
        node.validate()?;
        Ok(node)
    }

    pub fn leaf(ty: u32, entries: Vec<IndexEntry>) -> Result<Self> {
        Self::new(ty, true, entries)
    }

    pub fn internal(ty: u32, entries: Vec<IndexEntry>) -> Result<Self> {
        Self::new(ty, false, entries)
    }

    pub fn get(&self, key: &[u8]) -> Option<&Hash> {
        self.entries
            .binary_search_by(|entry| entry.key.as_slice().cmp(key))
            .ok()
            .map(|index| &self.entries[index].hash)
    }

    /// Upper-bound lookup used by internal nodes: the first entry whose
    /// separator is greater than or equal to `key`.
    pub fn child_for(&self, key: &[u8]) -> Option<&Hash> {
        let index = self
            .entries
            .partition_point(|entry| entry.key.as_slice() < key);
        self.entries.get(index).map(|entry| &entry.hash)
    }

    fn validate(&self) -> Result<()> {
        for pair in self.entries.windows(2) {
            if pair[0].key >= pair[1].key {
                return Err(Error::InvalidNode(
                    "index entries must be strictly sorted by key".into(),
                ));
            }
        }
        Ok(())
    }
}

impl VersionNode {
    pub fn new(root: Hash) -> Self {
        Self {
            parent: ZERO_HASH,
            root,
            source_commit: ZERO_HASH,
            pipeline: String::new(),
            author: String::new(),
            message: String::new(),
            timestamp: 0,
        }
    }
}

fn push_str(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| Error::InvalidNode("length overflow".into()))?;
        if end > self.data.len() {
            return Err(Error::InvalidNode(format!(
                "truncated node: need {count} bytes at {}, have {}",
                self.pos,
                self.remaining()
            )));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn skip(&mut self, count: usize) -> Result<()> {
        self.take(count).map(|_| ())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
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

    fn bytes32(&mut self) -> Result<[u8; 32]> {
        let bytes = self.take(32)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(bytes);
        Ok(out)
    }

    fn hash(&mut self) -> Result<Hash> {
        self.bytes32()
    }

    fn string(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|error| Error::InvalidNode(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CPP_CLASS, T_DIRECTORY, T_VIRTUAL_ROOT};

    fn object() -> Node {
        Node::Object(ObjectNode {
            ty: CPP_CLASS,
            commit: [7u8; 32],
            start_byte: 10,
            end_byte: 120,
            start_line: 2,
            end_line: 9,
            content_hash: [1u8; 32],
            input_hash: [2u8; 32],
            design_hash: [3u8; 32],
            name: "acme::Foo".into(),
            path: "src/foo.cpp".into(),
        })
    }

    #[test]
    fn object_roundtrips() {
        let node = object();
        let encoded = node.encode();
        assert_eq!(Node::decode(&encoded).unwrap(), node);
        assert_eq!(
            encoded.len(),
            HEADER_SIZE + 32 + 8 + 8 + 4 + 4 + 96 + 4 + 9 + 4 + 11
        );
    }

    #[test]
    fn design_roundtrips() {
        let node = Node::Design(DesignNode {
            text: "返回存活连接数。".into(),
        });
        let encoded = node.encode();
        assert_eq!(Node::decode(&encoded).unwrap(), node);
    }

    #[test]
    fn index_roundtrips_and_sorts() {
        let node = IndexNode::leaf(
            T_DIRECTORY,
            vec![
                IndexEntry {
                    key: b"b".to_vec(),
                    hash: [2u8; 32],
                },
                IndexEntry {
                    key: b"a".to_vec(),
                    hash: [1u8; 32],
                },
            ],
        )
        .unwrap();
        assert_eq!(node.entries[0].key, b"a");
        let encoded = Node::Index(node.clone()).encode();
        assert_eq!(Node::decode(&encoded).unwrap(), Node::Index(node));
    }

    #[test]
    fn version_roundtrips() {
        let node = Node::Version(VersionNode {
            parent: [1u8; 32],
            root: [2u8; 32],
            source_commit: [3u8; 32],
            pipeline: "model-x/p1".into(),
            author: "agent".into(),
            message: "first".into(),
            timestamp: 42,
        });
        let encoded = node.encode();
        assert_eq!(Node::decode(&encoded).unwrap(), node);
    }

    #[test]
    fn hash_changes_with_content() {
        let a = object();
        let mut b = object();
        if let Node::Object(object) = &mut b {
            object.name = "acme::Bar".into();
        }
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn decode_rejects_truncation_and_bad_version() {
        let encoded = object().encode();
        assert!(Node::decode(&encoded[..encoded.len() - 1]).is_err());

        let mut bad = encoded.clone();
        bad[0] = 9;
        assert!(matches!(
            Node::decode(&bad),
            Err(Error::UnsupportedVersion(9))
        ));
    }

    #[test]
    fn internal_child_lookup_uses_upper_bound() {
        let node = IndexNode::internal(
            T_VIRTUAL_ROOT,
            vec![
                IndexEntry {
                    key: b"files".to_vec(),
                    hash: [1u8; 32],
                },
                IndexEntry {
                    key: b"namespaces".to_vec(),
                    hash: [2u8; 32],
                },
            ],
        )
        .unwrap();
        assert_eq!(node.child_for(b"aaa"), Some(&[1u8; 32]));
        assert_eq!(node.child_for(b"files"), Some(&[1u8; 32]));
        assert_eq!(node.child_for(b"g"), Some(&[2u8; 32]));
        assert_eq!(node.child_for(b"zzz"), None);
    }

    #[test]
    fn duplicate_index_keys_are_rejected() {
        let result = IndexNode::leaf(
            T_DIRECTORY,
            vec![
                IndexEntry {
                    key: b"a".to_vec(),
                    hash: [1u8; 32],
                },
                IndexEntry {
                    key: b"a".to_vec(),
                    hash: [2u8; 32],
                },
            ],
        );
        assert!(result.is_err());
    }
}
