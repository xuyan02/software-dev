//! Single-file pack format.
//!
//! ```text
//! header (16)
//! records
//! index
//! trailer (64)
//! ```
//!
//! Every record is 8-byte aligned. The trailer's checksum covers the header,
//! records and index, so a pack can be verified without an external file.

use std::collections::BTreeMap;
use std::fs;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::hash::{Hash, hash, to_hex};
use crate::node::Node;

pub const PACK_MAGIC: &[u8; 4] = b"PAK1";
pub const INDEX_MAGIC: &[u8; 4] = b"PKI1";
pub const TRAILER_MAGIC: &[u8; 4] = b"TRL1";
pub const PACK_VERSION: u32 = 1;

pub const HEADER_SIZE: usize = 16;
pub const RECORD_HEADER_SIZE: usize = 16;
pub const INDEX_HEADER_SIZE: usize = 24;
pub const INDEX_ENTRY_SIZE: usize = 40;
pub const TRAILER_SIZE: usize = 64;

pub const RECORD_OBJECT: u8 = 1;
pub const RECORD_DESIGN: u8 = 2;
pub const RECORD_INDEX: u8 = 3;
pub const RECORD_VERSION: u8 = 4;

pub const CODEC_RAW: u8 = 0;

pub struct WrittenPack {
    pub bytes: Vec<u8>,
    pub checksum: Hash,
    pub count: usize,
}

pub struct Pack {
    bytes: Vec<u8>,
    checksum: Hash,
    index_offset: usize,
    entries: Vec<(Hash, u64)>,
}

impl Pack {
    pub fn open(path: &Path) -> Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < TRAILER_SIZE {
            return Err(Error::InvalidPack(
                "file is smaller than the trailer".into(),
            ));
        }

        let trailer_start = bytes.len() - TRAILER_SIZE;
        let trailer = &bytes[trailer_start..];
        if &trailer[..4] != TRAILER_MAGIC {
            return Err(Error::InvalidPack("bad trailer magic".into()));
        }
        let version = le_u32(trailer, 4)?;
        if version != PACK_VERSION {
            return Err(Error::InvalidPack(format!(
                "unsupported pack version {version}"
            )));
        }
        let pack_size = le_u64(trailer, 8)? as usize;
        let index_offset = le_u64(trailer, 16)? as usize;
        let index_size = le_u64(trailer, 24)? as usize;
        let checksum: Hash = trailer[32..64].try_into().expect("32 bytes");

        if pack_size != bytes.len() {
            return Err(Error::InvalidPack(format!(
                "trailer says {pack_size} bytes, file has {}",
                bytes.len()
            )));
        }
        let actual = hash(&bytes[..trailer_start]);
        if actual != checksum {
            return Err(Error::PackChecksum);
        }
        if index_offset
            .checked_add(index_size)
            .is_none_or(|end| end != trailer_start)
        {
            return Err(Error::InvalidPack(
                "index does not end at the trailer".into(),
            ));
        }

        let index = &bytes[index_offset..index_offset + index_size];
        let entries = parse_index(index)?;

        Ok(Self {
            bytes,
            checksum,
            index_offset,
            entries,
        })
    }

    pub fn checksum(&self) -> &Hash {
        &self.checksum
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hashes(&self) -> impl Iterator<Item = &Hash> {
        self.entries.iter().map(|(address, _)| address)
    }

    pub fn contains(&self, address: &Hash) -> bool {
        self.entries
            .binary_search_by(|(candidate, _)| candidate.cmp(address))
            .is_ok()
    }

    pub fn get(&self, address: &Hash) -> Result<Node> {
        let position = self
            .entries
            .binary_search_by(|(candidate, _)| candidate.cmp(address))
            .map_err(|_| Error::NotFound(to_hex(address)))?;
        let (_, offset) = self.entries[position];
        let offset =
            usize::try_from(offset).map_err(|_| Error::InvalidPack("offset overflow".into()))?;

        if offset < HEADER_SIZE || offset + RECORD_HEADER_SIZE > self.index_offset {
            return Err(Error::InvalidPack(format!(
                "record offset {offset} is outside the record area"
            )));
        }
        let record = &self.bytes[offset..];
        let record_size = le_u32(record, 0)? as usize;
        if offset + record_size > self.index_offset {
            return Err(Error::InvalidPack("record crosses into the index".into()));
        }
        decode_record(address, &record[..record_size])
    }
}

/// Index of one pack file. Opens the trailer and index only; node payloads are
/// read on demand.
pub struct PackIndex {
    path: PathBuf,
    checksum: Hash,
    index_offset: usize,
    entries: Vec<(Hash, u64)>,
}

impl PackIndex {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        if length < TRAILER_SIZE as u64 {
            return Err(Error::InvalidPack(
                "file is smaller than the trailer".into(),
            ));
        }
        file.seek(SeekFrom::Start(length - TRAILER_SIZE as u64))?;
        let mut trailer = [0u8; TRAILER_SIZE];
        file.read_exact(&mut trailer)?;

        if &trailer[..4] != TRAILER_MAGIC {
            return Err(Error::InvalidPack("bad trailer magic".into()));
        }
        let version = le_u32(&trailer, 4)?;
        if version != PACK_VERSION {
            return Err(Error::InvalidPack(format!(
                "unsupported pack version {version}"
            )));
        }
        let pack_size = le_u64(&trailer, 8)?;
        if pack_size != length {
            return Err(Error::InvalidPack(format!(
                "trailer says {pack_size} bytes, file has {length}"
            )));
        }
        let index_offset = le_u64(&trailer, 16)? as usize;
        let index_size = le_u64(&trailer, 24)? as usize;
        let checksum: Hash = trailer[32..64].try_into().expect("32 bytes");
        if index_offset
            .checked_add(index_size)
            .is_none_or(|end| end as u64 != length - TRAILER_SIZE as u64)
        {
            return Err(Error::InvalidPack(
                "index does not end at the trailer".into(),
            ));
        }

        file.seek(SeekFrom::Start(index_offset as u64))?;
        let mut index = vec![0u8; index_size];
        file.read_exact(&mut index)?;
        let entries = parse_index(&index)?;
        Ok(Self {
            path: path.to_path_buf(),
            checksum,
            index_offset,
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn checksum(&self) -> &Hash {
        &self.checksum
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn hashes(&self) -> impl Iterator<Item = &Hash> {
        self.entries.iter().map(|(address, _)| address)
    }

    pub fn contains(&self, address: &Hash) -> bool {
        self.entries
            .binary_search_by(|(candidate, _)| candidate.cmp(address))
            .is_ok()
    }

    pub fn get(&self, address: &Hash) -> Result<Node> {
        let position = self
            .entries
            .binary_search_by(|(candidate, _)| candidate.cmp(address))
            .map_err(|_| Error::NotFound(to_hex(address)))?;
        let (_, offset) = self.entries[position];
        let offset =
            usize::try_from(offset).map_err(|_| Error::InvalidPack("offset overflow".into()))?;
        if offset < HEADER_SIZE || offset + RECORD_HEADER_SIZE > self.index_offset {
            return Err(Error::InvalidPack(format!(
                "record offset {offset} is outside the record area"
            )));
        }

        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut header = [0u8; RECORD_HEADER_SIZE];
        file.read_exact(&mut header)?;
        let record_size = le_u32(&header, 0)? as usize;
        if record_size < RECORD_HEADER_SIZE || !record_size.is_multiple_of(8) {
            return Err(Error::InvalidPack(format!(
                "invalid record size {record_size}"
            )));
        }
        if offset + record_size > self.index_offset {
            return Err(Error::InvalidPack("record crosses into the index".into()));
        }
        let mut record = vec![0u8; record_size];
        record[..RECORD_HEADER_SIZE].copy_from_slice(&header);
        file.read_exact(&mut record[RECORD_HEADER_SIZE..])?;
        decode_record(address, &record)
    }
}

/// Multi-pack index: hash → pack, built by reading each pack's index.
pub struct MultiPackIndex {
    packs: Vec<PackIndex>,
    index: BTreeMap<Hash, usize>,
}

impl MultiPackIndex {
    pub fn open(dir: &Path) -> Result<Self> {
        let mut packs = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries {
                let entry = entry?;
                if entry.path().extension().is_some_and(|ext| ext == "pack") {
                    packs.push(PackIndex::open(&entry.path())?);
                }
            }
        }
        packs.sort_by(|a, b| a.path().cmp(b.path()));
        let mut index = BTreeMap::new();
        for (position, pack) in packs.iter().enumerate() {
            for hash in pack.hashes() {
                index.entry(*hash).or_insert(position);
            }
        }
        Ok(Self { packs, index })
    }

    pub fn get(&self, address: &Hash) -> Result<Node> {
        let position = self
            .index
            .get(address)
            .ok_or_else(|| Error::NotFound(to_hex(address)))?;
        self.packs[*position].get(address)
    }

    pub fn contains(&self, address: &Hash) -> bool {
        self.index.contains_key(address)
    }

    pub fn hashes(&self) -> impl Iterator<Item = &Hash> {
        self.index.keys()
    }

    pub fn packs(&self) -> &[PackIndex] {
        &self.packs
    }
}

fn parse_index(index: &[u8]) -> Result<Vec<(Hash, u64)>> {
    if index.len() < INDEX_HEADER_SIZE {
        return Err(Error::InvalidPack("index is truncated".into()));
    }
    if &index[..4] != INDEX_MAGIC {
        return Err(Error::InvalidPack("bad index magic".into()));
    }
    let index_version = le_u32(index, 4)?;
    if index_version != PACK_VERSION {
        return Err(Error::InvalidPack(format!(
            "unsupported index version {index_version}"
        )));
    }
    let count = le_u64(index, 8)? as usize;
    let entry_size = le_u32(index, 16)? as usize;
    if entry_size != INDEX_ENTRY_SIZE {
        return Err(Error::InvalidPack(format!(
            "unexpected index entry size {entry_size}"
        )));
    }
    let expected_size = INDEX_HEADER_SIZE
        .checked_add(
            count
                .checked_mul(INDEX_ENTRY_SIZE)
                .ok_or_else(|| Error::InvalidPack("index size overflow".into()))?,
        )
        .ok_or_else(|| Error::InvalidPack("index size overflow".into()))?;
    if expected_size != index.len() {
        return Err(Error::InvalidPack(format!(
            "index size {}, expected {expected_size}",
            index.len()
        )));
    }

    let mut entries = Vec::with_capacity(count);
    let mut cursor = INDEX_HEADER_SIZE;
    for _ in 0..count {
        let address: Hash = index[cursor..cursor + 32].try_into().expect("32 bytes");
        let offset = le_u64(index, cursor + 32)?;
        entries.push((address, offset));
        cursor += INDEX_ENTRY_SIZE;
    }
    for pair in entries.windows(2) {
        if pair[0].0 >= pair[1].0 {
            return Err(Error::InvalidPack(
                "index entries are not strictly sorted".into(),
            ));
        }
    }
    Ok(entries)
}

fn decode_record(address: &Hash, record: &[u8]) -> Result<Node> {
    let record_size = le_u32(record, 0)? as usize;
    let compressed_size = le_u32(record, 4)? as usize;
    let record_type = record[8];
    let codec = record[9];
    let original_size = le_u32(record, 12)? as usize;

    if record_size < RECORD_HEADER_SIZE || !record_size.is_multiple_of(8) {
        return Err(Error::InvalidPack(format!(
            "invalid record size {record_size}"
        )));
    }
    if record.len() < record_size {
        return Err(Error::InvalidPack("record is truncated".into()));
    }
    if RECORD_HEADER_SIZE + compressed_size > record_size {
        return Err(Error::InvalidPack(
            "compressed payload is larger than the record".into(),
        ));
    }
    if codec != CODEC_RAW {
        return Err(Error::UnsupportedCodec(codec));
    }
    if compressed_size != original_size {
        return Err(Error::InvalidPack("raw record sizes disagree".into()));
    }

    let payload = &record[RECORD_HEADER_SIZE..RECORD_HEADER_SIZE + compressed_size];
    let actual = hash(payload);
    if actual != *address {
        return Err(Error::InvalidPack(format!(
            "record {} hashes to {}",
            to_hex(address),
            to_hex(&actual)
        )));
    }

    let node = Node::decode(payload)?;
    if record_type_of(&node) != record_type {
        return Err(Error::InvalidPack(
            "record type does not match its node".into(),
        ));
    }
    Ok(node)
}

/// Serialize nodes into a pack. Nodes are sorted and deduplicated by address.
pub fn write(nodes: &[Node]) -> Result<WrittenPack> {
    let mut items = Vec::with_capacity(nodes.len());
    for node in nodes {
        node.validate()?;
        let payload = node.encode();
        items.push((hash(&payload), payload, record_type_of(node)));
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.dedup_by(|a, b| a.0 == b.0);

    let mut out = Vec::new();
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&PACK_VERSION.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());

    let mut offsets = Vec::with_capacity(items.len());
    for (address, payload, record_type) in &items {
        let offset = out.len() as u64;
        let payload_len = u32::try_from(payload.len())
            .map_err(|_| Error::InvalidPack("node is too large for a pack".into()))?;
        let raw_size = RECORD_HEADER_SIZE
            .checked_add(payload.len())
            .ok_or_else(|| Error::InvalidPack("record size overflow".into()))?;
        let padding = raw_size.next_multiple_of(8) - raw_size;
        let record_size = u32::try_from(raw_size + padding)
            .map_err(|_| Error::InvalidPack("record is too large".into()))?;

        out.extend_from_slice(&record_size.to_le_bytes());
        out.extend_from_slice(&payload_len.to_le_bytes());
        out.push(*record_type);
        out.push(CODEC_RAW);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&payload_len.to_le_bytes());
        out.extend_from_slice(payload);
        out.extend(std::iter::repeat_n(0u8, padding));
        offsets.push((*address, offset));
    }

    let index_offset = out.len() as u64;
    let mut index = Vec::new();
    index.extend_from_slice(INDEX_MAGIC);
    index.extend_from_slice(&PACK_VERSION.to_le_bytes());
    index.extend_from_slice(&(offsets.len() as u64).to_le_bytes());
    index.extend_from_slice(&(INDEX_ENTRY_SIZE as u32).to_le_bytes());
    index.extend_from_slice(&0u32.to_le_bytes());
    for (address, offset) in &offsets {
        index.extend_from_slice(address);
        index.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&index);

    let pack_size = (out.len() + TRAILER_SIZE) as u64;
    let checksum = hash(&out);
    out.extend_from_slice(TRAILER_MAGIC);
    out.extend_from_slice(&PACK_VERSION.to_le_bytes());
    out.extend_from_slice(&pack_size.to_le_bytes());
    out.extend_from_slice(&index_offset.to_le_bytes());
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&checksum);

    Ok(WrittenPack {
        bytes: out,
        checksum,
        count: offsets.len(),
    })
}

pub fn record_type_of(node: &Node) -> u8 {
    match node {
        Node::Object(_) => RECORD_OBJECT,
        Node::Design(_) => RECORD_DESIGN,
        Node::Index(_) => RECORD_INDEX,
        Node::Version(_) => RECORD_VERSION,
    }
}

fn le_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| Error::InvalidPack("truncated u32".into()))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn le_u64(data: &[u8], offset: usize) -> Result<u64> {
    let bytes = data
        .get(offset..offset + 8)
        .ok_or_else(|| Error::InvalidPack("truncated u64".into()))?;
    Ok(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{DesignNode, IndexEntry, IndexNode, ObjectNode, VersionNode};
    use crate::types::{CPP_CLASS, T_DIRECTORY, T_VIEW_ROOT};

    fn object(name: &str) -> Node {
        Node::Object(ObjectNode {
            ty: CPP_CLASS,
            commit: [1u8; 32],
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            end_line: 2,
            content_hash: [2u8; 32],
            input_hash: [3u8; 32],
            design_hash: [4u8; 32],
            name: name.into(),
            path: "src/a.cpp".into(),
        })
    }

    fn nodes() -> Vec<Node> {
        vec![
            object("acme::Foo"),
            object("acme::Bar"),
            Node::Design(DesignNode {
                text: "design".into(),
            }),
            Node::Index(
                IndexNode::leaf(
                    T_DIRECTORY,
                    vec![IndexEntry {
                        key: b"foo.cpp".to_vec(),
                        hash: [9u8; 32],
                    }],
                )
                .unwrap(),
            ),
            Node::Version(VersionNode::new([5u8; 32])),
        ]
    }

    #[test]
    fn pack_roundtrips_every_node() {
        let nodes = nodes();
        let written = write(&nodes).unwrap();
        assert_eq!(written.count, nodes.len());
        let pack = Pack::from_bytes(written.bytes.clone()).unwrap();
        assert_eq!(pack.checksum(), &written.checksum);
        assert_eq!(pack.len(), nodes.len());
        for node in &nodes {
            assert_eq!(pack.get(&node.hash()).unwrap(), *node);
        }
    }

    #[test]
    fn records_are_eight_byte_aligned() {
        let written = write(&nodes()).unwrap();
        let pack = Pack::from_bytes(written.bytes.clone()).unwrap();
        for (_, offset) in &pack.entries {
            let offset = *offset as usize;
            assert!(offset.is_multiple_of(8));
            let record = &written.bytes[offset..];
            let record_size = le_u32(record, 0).unwrap() as usize;
            assert!(record_size.is_multiple_of(8));
        }
    }

    #[test]
    fn pack_index_reads_nodes_and_multipack_finds_them() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("packs");
        fs::create_dir_all(&dir).unwrap();
        let nodes = nodes();
        let written = write(&nodes).unwrap();
        let path = dir.join("test.pack");
        fs::write(&path, &written.bytes).unwrap();

        let index = PackIndex::open(&path).unwrap();
        assert_eq!(index.len(), nodes.len());
        for node in &nodes {
            assert_eq!(index.get(&node.hash()).unwrap(), *node);
        }

        let multi = MultiPackIndex::open(&dir).unwrap();
        for node in &nodes {
            assert!(multi.contains(&node.hash()));
            assert_eq!(multi.get(&node.hash()).unwrap(), *node);
        }
    }

    #[test]
    fn corruption_is_detected_by_checksum() {
        let mut bytes = write(&nodes()).unwrap().bytes;
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xff;
        assert!(matches!(Pack::from_bytes(bytes), Err(Error::PackChecksum)));
    }

    #[test]
    fn missing_hash_is_not_found() {
        let written = write(&nodes()).unwrap();
        let pack = Pack::from_bytes(written.bytes).unwrap();
        assert!(matches!(pack.get(&[0xAAu8; 32]), Err(Error::NotFound(_))));
    }

    #[test]
    fn empty_pack_is_valid() {
        let written = write(&[]).unwrap();
        let pack = Pack::from_bytes(written.bytes).unwrap();
        assert!(pack.is_empty());
    }

    #[test]
    fn view_root_type_is_an_index_record() {
        let node = Node::Index(
            IndexNode::internal(
                T_VIEW_ROOT,
                vec![IndexEntry {
                    key: b"files".to_vec(),
                    hash: [7u8; 32],
                }],
            )
            .unwrap(),
        );
        assert_eq!(record_type_of(&node), RECORD_INDEX);
    }
}
