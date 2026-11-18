//! PAK1 pack encoding.
//!
//! A pack stores design objects, commits, design documents, and symbols in one
//! file. The index is sorted by `(record_type, hash)` and maps each record to
//! its byte offset. Raw records are the only codec implemented.

use crate::format::{decode_commit_checked, decode_object_checked, hash_bytes};
use crate::model::{CommitHash, Hash32, ObjectHash};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;

pub const PACK_MAGIC: &[u8; 4] = b"PAK1";
pub const INDEX_MAGIC: &[u8; 4] = b"PKI1";
pub const TRAILER_MAGIC: &[u8; 4] = b"TRL1";
pub const VERSION: u32 = 1;
pub const INDEX_ENTRY_SIZE: u32 = 48;
pub const TRAILER_SIZE: usize = 64;

pub const TYPE_OBJECT: u8 = 1;
pub const TYPE_DOC: u8 = 2;
pub const TYPE_SYMBOL: u8 = 3;
pub const TYPE_COMMIT: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRecord {
    pub record_type: u8,
    pub hash: Hash32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexEntry {
    record_type: u8,
    hash: Hash32,
    offset: u64,
}

#[derive(Debug, Clone)]
struct Layout {
    entries: Vec<IndexEntry>,
    pack_hash: Hash32,
    index_offset: usize,
}

pub fn record_type_for_dir(dir: &str) -> Option<u8> {
    match dir {
        "objects" => Some(TYPE_OBJECT),
        "docs" => Some(TYPE_DOC),
        "symbols" => Some(TYPE_SYMBOL),
        "commits" => Some(TYPE_COMMIT),
        _ => None,
    }
}

pub fn dir_for_record_type(record_type: u8) -> Option<&'static str> {
    match record_type {
        TYPE_OBJECT => Some("objects"),
        TYPE_DOC => Some("docs"),
        TYPE_SYMBOL => Some("symbols"),
        TYPE_COMMIT => Some("commits"),
        _ => None,
    }
}

/// Encode records into a pack. Records are sorted and deduplicated.
pub fn encode_pack(mut records: Vec<PackRecord>) -> Result<(Vec<u8>, Hash32)> {
    for record in &records {
        validate_record(record.record_type, record.hash, &record.bytes)?;
    }
    records
        .sort_by(|left, right| (left.record_type, left.hash).cmp(&(right.record_type, right.hash)));
    let mut deduped: Vec<PackRecord> = Vec::with_capacity(records.len());
    for record in records {
        if let Some(previous) = deduped.last()
            && previous.record_type == record.record_type
            && previous.hash == record.hash
        {
            if previous.bytes != record.bytes {
                bail!(
                    "conflicting records for type {} hash {}",
                    record.record_type,
                    record.hash.to_hex()
                );
            }
            continue;
        }
        deduped.push(record);
    }

    let mut out = Vec::new();
    out.extend_from_slice(PACK_MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());

    let mut offsets = BTreeMap::new();
    for record in &deduped {
        let offset = out.len() as u64;
        let size = u32::try_from(16 + record.bytes.len()).context("pack record is too large")?;
        let payload_size =
            u32::try_from(record.bytes.len()).context("pack payload is too large")?;
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&payload_size.to_le_bytes());
        out.push(record.record_type);
        out.push(0);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&payload_size.to_le_bytes());
        out.extend_from_slice(&record.bytes);
        offsets.insert((record.record_type, record.hash), offset);
    }

    let index_offset = out.len();
    let count = deduped.len() as u64;
    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&INDEX_ENTRY_SIZE.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for record in &deduped {
        let offset = offsets[&(record.record_type, record.hash)];
        out.push(record.record_type);
        out.extend_from_slice(&[0u8; 7]);
        out.extend_from_slice(&record.hash.0);
        out.extend_from_slice(&offset.to_le_bytes());
    }
    let index_size = out.len() - index_offset;
    let pack_hash = hash_bytes(&out);

    let pack_size = (out.len() + TRAILER_SIZE) as u64;
    out.extend_from_slice(TRAILER_MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&pack_size.to_le_bytes());
    out.extend_from_slice(&(index_offset as u64).to_le_bytes());
    out.extend_from_slice(&(index_size as u64).to_le_bytes());
    out.extend_from_slice(&pack_hash.0);
    Ok((out, pack_hash))
}

/// Decode and fully verify a pack.
pub fn decode_pack(bytes: &[u8]) -> Result<Vec<PackRecord>> {
    let layout = parse_layout(bytes)?;
    let mut records = Vec::with_capacity(layout.entries.len());
    for entry in &layout.entries {
        let payload = read_entry(bytes, &layout, *entry)?;
        records.push(PackRecord {
            record_type: entry.record_type,
            hash: entry.hash,
            bytes: payload,
        });
    }
    Ok(records)
}

/// Verify a pack and return its pack hash.
pub fn validate_pack(bytes: &[u8]) -> Result<Hash32> {
    decode_pack(bytes)?;
    Ok(parse_layout(bytes)?.pack_hash)
}

/// Look up one record without decoding unrelated records.
pub fn read_record(bytes: &[u8], record_type: u8, hash: Hash32) -> Result<Option<Vec<u8>>> {
    let layout = parse_layout(bytes)?;
    let Some(entry) = layout
        .entries
        .iter()
        .find(|entry| entry.record_type == record_type && entry.hash == hash)
    else {
        return Ok(None);
    };
    Ok(Some(read_entry(bytes, &layout, *entry)?))
}

/// Verify a record payload against its type and hash.
pub fn validate_record(record_type: u8, hash: Hash32, bytes: &[u8]) -> Result<()> {
    if hash.is_zero() {
        bail!("zero hash is not a valid pack record");
    }
    match record_type {
        TYPE_OBJECT => {
            decode_object_checked(bytes, ObjectHash(hash)).context("invalid object record")?;
        }
        TYPE_COMMIT => {
            decode_commit_checked(bytes, CommitHash(hash)).context("invalid commit record")?;
        }
        TYPE_DOC => {
            if hash_bytes(bytes) != hash {
                bail!("doc record does not match its hash");
            }
            if std::str::from_utf8(bytes).is_err() {
                bail!("doc record is not UTF-8");
            }
        }
        TYPE_SYMBOL => {
            if hash_bytes(bytes) != hash {
                bail!("symbol record does not match its hash");
            }
            if bytes.is_empty() || bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
                bail!("invalid symbol record");
            }
        }
        _ => bail!("unknown pack record type: {record_type}"),
    }
    Ok(())
}

fn parse_layout(bytes: &[u8]) -> Result<Layout> {
    if bytes.len() < 16 + 24 + TRAILER_SIZE {
        bail!("pack is too small");
    }
    if &bytes[..4] != PACK_MAGIC {
        bail!("invalid pack magic");
    }
    let version = u32::from_le_bytes(bytes[4..8].try_into().expect("checked length"));
    if version != VERSION {
        bail!("unsupported pack version: {version}");
    }
    if u64::from_le_bytes(bytes[8..16].try_into().expect("checked length")) != 0 {
        bail!("pack reserved header is nonzero");
    }

    let trailer_offset = bytes.len() - TRAILER_SIZE;
    if &bytes[trailer_offset..trailer_offset + 4] != TRAILER_MAGIC {
        bail!("invalid pack trailer magic");
    }
    let trailer_version = u32::from_le_bytes(
        bytes[trailer_offset + 4..trailer_offset + 8]
            .try_into()
            .expect("checked length"),
    );
    if trailer_version != VERSION {
        bail!("unsupported trailer version: {trailer_version}");
    }
    let pack_size = u64::from_le_bytes(
        bytes[trailer_offset + 8..trailer_offset + 16]
            .try_into()
            .expect("checked length"),
    );
    let index_offset = u64::from_le_bytes(
        bytes[trailer_offset + 16..trailer_offset + 24]
            .try_into()
            .expect("checked length"),
    );
    let index_size = u64::from_le_bytes(
        bytes[trailer_offset + 24..trailer_offset + 32]
            .try_into()
            .expect("checked length"),
    );
    let mut pack_hash = [0u8; 32];
    pack_hash.copy_from_slice(&bytes[trailer_offset + 32..trailer_offset + 64]);

    if pack_size != bytes.len() as u64 {
        bail!("pack size does not match the trailer");
    }
    let index_offset = usize::try_from(index_offset).context("index offset is too large")?;
    let index_size = usize::try_from(index_size).context("index size is too large")?;
    if index_offset < 16 || index_size < 24 || index_offset + index_size != trailer_offset {
        bail!("invalid pack index bounds");
    }
    let computed = hash_bytes(&bytes[..trailer_offset]);
    if computed.0 != pack_hash {
        bail!("pack hash does not match the trailer");
    }

    let index = &bytes[index_offset..index_offset + index_size];
    if &index[..4] != INDEX_MAGIC {
        bail!("invalid pack index magic");
    }
    let index_version = u32::from_le_bytes(index[4..8].try_into().expect("checked length"));
    if index_version != VERSION {
        bail!("unsupported index version: {index_version}");
    }
    let count = u64::from_le_bytes(index[8..16].try_into().expect("checked length"));
    let entry_size = u32::from_le_bytes(index[16..20].try_into().expect("checked length"));
    if entry_size != INDEX_ENTRY_SIZE {
        bail!("unsupported index entry size: {entry_size}");
    }
    if u32::from_le_bytes(index[20..24].try_into().expect("checked length")) != 0 {
        bail!("index reserved field is nonzero");
    }
    let count = usize::try_from(count).context("too many index entries")?;
    let expected = 24usize
        .checked_add(
            count
                .checked_mul(INDEX_ENTRY_SIZE as usize)
                .context("index too large")?,
        )
        .context("index too large")?;
    if index_size != expected {
        bail!("index size does not match its entry count");
    }

    let mut entries = Vec::with_capacity(count);
    for entry in 0..count {
        let start = 24 + entry * INDEX_ENTRY_SIZE as usize;
        let item = &index[start..start + INDEX_ENTRY_SIZE as usize];
        let record_type = item[0];
        if !(TYPE_OBJECT..=TYPE_COMMIT).contains(&record_type) {
            bail!("unknown record type in index: {record_type}");
        }
        if item[1..8].iter().any(|byte| *byte != 0) {
            bail!("index entry reserved field is nonzero");
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&item[8..40]);
        let hash = Hash32(hash);
        if hash.is_zero() {
            bail!("zero hash in pack index");
        }
        let offset = u64::from_le_bytes(item[40..48].try_into().expect("checked length"));
        if offset < 16 || offset >= index_offset as u64 {
            bail!("pack record offset is out of bounds");
        }
        entries.push(IndexEntry {
            record_type,
            hash,
            offset,
        });
    }
    if entries
        .windows(2)
        .any(|pair| (pair[0].record_type, pair[0].hash) >= (pair[1].record_type, pair[1].hash))
    {
        bail!("pack index is not sorted and unique");
    }

    Ok(Layout {
        entries,
        pack_hash: Hash32(pack_hash),
        index_offset,
    })
}

fn read_entry(bytes: &[u8], layout: &Layout, entry: IndexEntry) -> Result<Vec<u8>> {
    let offset = usize::try_from(entry.offset).context("record offset is too large")?;
    if offset + 16 > layout.index_offset {
        bail!("pack record header is truncated");
    }
    let record_size =
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("checked")) as usize;
    let compressed_size =
        u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().expect("checked")) as usize;
    let record_type = bytes[offset + 8];
    let codec = bytes[offset + 9];
    let reserved = u16::from_le_bytes(bytes[offset + 10..offset + 12].try_into().expect("checked"));
    let original_size =
        u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().expect("checked")) as usize;
    if record_type != entry.record_type {
        bail!("pack record type does not match the index");
    }
    if codec != 0 {
        bail!("unsupported pack codec: {codec}");
    }
    if reserved != 0 {
        bail!("pack record reserved field is nonzero");
    }
    if compressed_size != original_size {
        bail!("raw pack record sizes differ");
    }
    if record_size < 16 || record_size - 16 < compressed_size {
        bail!("invalid pack record size");
    }
    let end = offset
        .checked_add(record_size)
        .context("pack record is too large")?;
    if end > layout.index_offset {
        bail!("pack record exceeds the record area");
    }
    let payload_end = offset + 16 + compressed_size;
    let payload = bytes[offset + 16..payload_end].to_vec();
    validate_record(record_type, entry.hash, &payload)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{
        commit_hash, doc_hash, encode_commit, encode_object, object_hash, symbol_hash,
    };
    use crate::model::*;

    fn object() -> (Hash32, Vec<u8>) {
        let object = DesignObject::new(
            Kind::Directory,
            ObjectPayload::Directory(DirectoryPayload {
                qualified_name: symbol_hash("files"),
                parent: ParentRef::View,
                source: None,
                document: SymbolHash::ZERO,
                children: vec![],
            }),
        );
        (
            object_hash(&object).unwrap().0,
            encode_object(&object).unwrap(),
        )
    }

    fn commit(view: Hash32) -> (Hash32, Vec<u8>) {
        let commit = Commit {
            view: ObjectHash(view),
            parent: CommitHash::ZERO,
            message: "test".into(),
            author: "agent".into(),
            timestamp: 1,
        };
        (commit_hash(&commit).0, encode_commit(&commit))
    }

    #[test]
    fn roundtrips_all_record_types() {
        let (object_hash, object_bytes) = object();
        let (commit_hash, commit_bytes) = commit(object_hash);
        let doc = b"design text".to_vec();
        let doc_hash = doc_hash("design text").0;
        let symbol = b"namespaces/app".to_vec();
        let symbol_hash = symbol_hash("namespaces/app").0;
        let records = vec![
            PackRecord {
                record_type: TYPE_OBJECT,
                hash: object_hash,
                bytes: object_bytes.clone(),
            },
            PackRecord {
                record_type: TYPE_COMMIT,
                hash: commit_hash,
                bytes: commit_bytes,
            },
            PackRecord {
                record_type: TYPE_DOC,
                hash: doc_hash,
                bytes: doc,
            },
            PackRecord {
                record_type: TYPE_SYMBOL,
                hash: symbol_hash,
                bytes: symbol,
            },
        ];
        let (pack, pack_hash) = encode_pack(records).unwrap();
        assert_eq!(validate_pack(&pack).unwrap(), pack_hash);
        let decoded = decode_pack(&pack).unwrap();
        assert_eq!(decoded.len(), 4);
        assert_eq!(
            read_record(&pack, TYPE_OBJECT, object_hash)
                .unwrap()
                .unwrap(),
            object_bytes
        );
        assert_eq!(
            read_record(&pack, TYPE_DOC, doc_hash).unwrap().unwrap(),
            b"design text"
        );
        assert_eq!(
            read_record(&pack, TYPE_SYMBOL, symbol_hash)
                .unwrap()
                .unwrap(),
            b"namespaces/app"
        );
    }

    #[test]
    fn rejects_tampering() {
        let (object_hash, object_bytes) = object();
        let (pack, _) = encode_pack(vec![PackRecord {
            record_type: TYPE_OBJECT,
            hash: object_hash,
            bytes: object_bytes,
        }])
        .unwrap();
        let mut tampered = pack.clone();
        let last = tampered.len() - TRAILER_SIZE - 1;
        tampered[last] ^= 0xff;
        assert!(validate_pack(&tampered).is_err());
    }

    #[test]
    fn rejects_wrong_record_hash() {
        let (_, object_bytes) = object();
        let result = encode_pack(vec![PackRecord {
            record_type: TYPE_OBJECT,
            hash: Hash32([9; 32]),
            bytes: object_bytes,
        }]);
        assert!(result.is_err());
    }
}
