use anyhow::{Context, Result, bail};

use crate::hash::{Hash32, ObjectHash, SymbolHash};

#[derive(Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn raw(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.raw(bytes);
    }

    pub fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn hash(&mut self, value: Hash32) {
        self.bytes.extend_from_slice(value.as_bytes());
    }

    pub fn object(&mut self, value: ObjectHash) {
        self.hash(value.0);
    }

    pub fn hash_list(&mut self, values: &[ObjectHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.hash(value.0);
        }
    }

    pub fn symbol(&mut self, value: SymbolHash) {
        self.hash(value.0);
    }

    pub fn text(&mut self, value: &str) {
        self.u32(value.len() as u32);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    pub fn symbol_list(&mut self, values: &[SymbolHash]) {
        self.u32(values.len() as u32);
        for value in values {
            self.symbol(*value);
        }
    }

    pub fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    pub fn u8(&mut self) -> Result<u8> {
        let value = *self
            .bytes
            .get(self.position)
            .context("unexpected end of record")?;
        self.position += 1;
        Ok(value)
    }

    pub fn u32(&mut self) -> Result<u32> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }

    pub fn u64(&mut self) -> Result<u64> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }

    pub fn hash(&mut self) -> Result<Hash32> {
        let bytes = self.take(32)?;
        Ok(Hash32::from_bytes(
            bytes.try_into().expect("thirty-two bytes"),
        ))
    }

    pub fn bytes(&mut self, len: usize) -> Result<&'a [u8]> {
        self.take(len)
    }

    pub fn symbol(&mut self) -> Result<SymbolHash> {
        Ok(SymbolHash(self.hash()?))
    }

    pub fn object(&mut self) -> Result<ObjectHash> {
        Ok(ObjectHash(self.hash()?))
    }

    pub fn hash_list(&mut self) -> Result<Vec<ObjectHash>> {
        let count = self.u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(ObjectHash(self.hash()?));
        }
        Ok(values)
    }

    pub fn text(&mut self) -> Result<String> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        Ok(std::str::from_utf8(bytes)?.to_owned())
    }

    pub fn symbol_list(&mut self) -> Result<Vec<SymbolHash>> {
        let count = self.u32()? as usize;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.symbol()?);
        }
        Ok(values)
    }

    pub fn finish(&self) -> Result<()> {
        if self.remaining() != 0 {
            bail!("trailing bytes in record");
        }
        Ok(())
    }

    pub(crate) fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .context("record length overflow")?;
        let bytes = self
            .bytes
            .get(self.position..end)
            .context("unexpected end of record")?;
        self.position = end;
        Ok(bytes)
    }
}
