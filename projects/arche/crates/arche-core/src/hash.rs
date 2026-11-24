use std::fmt;

use sha2::{Digest, Sha256};

/// A 32-byte SHA-256 digest used throughout Arche.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash32([u8; 32]);

impl Default for Hash32 {
    fn default() -> Self {
        Self::ZERO
    }
}

impl Hash32 {
    pub const ZERO: Self = Self([0; 32]);

    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn sha256(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self(hasher.finalize().into())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0; 32]
    }

    pub fn to_hex(self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{byte:02x}");
        }
        out
    }

    pub fn from_hex(text: &str) -> anyhow::Result<Self> {
        if text.len() != 64 {
            anyhow::bail!("hash must be 64 hex characters");
        }
        let mut bytes = [0u8; 32];
        for (index, chunk) in text.as_bytes().chunks_exact(2).enumerate() {
            let chunk = std::str::from_utf8(chunk)?;
            bytes[index] =
                u8::from_str_radix(chunk, 16).map_err(|_| anyhow::anyhow!("invalid hex hash"))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolHash(pub Hash32);

impl Default for SymbolHash {
    fn default() -> Self {
        Self::ZERO
    }
}

impl SymbolHash {
    pub const ZERO: Self = Self(Hash32::ZERO);

    pub fn of(text: &str) -> Self {
        Self(Hash32::sha256(text.as_bytes()))
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    pub fn from_hex(text: &str) -> anyhow::Result<Self> {
        Ok(Self(Hash32::from_hex(text)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectHash(pub Hash32);

impl From<ObjectHash> for Hash32 {
    fn from(value: ObjectHash) -> Self {
        value.0
    }
}

impl Default for ObjectHash {
    fn default() -> Self {
        Self::ZERO
    }
}

impl ObjectHash {
    pub const ZERO: Self = Self(Hash32::ZERO);

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    pub fn from_hex(text: &str) -> anyhow::Result<Self> {
        Ok(Self(Hash32::from_hex(text)?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GitCommit(pub Hash32);

impl Default for GitCommit {
    fn default() -> Self {
        Self::ZERO
    }
}

impl GitCommit {
    pub const ZERO: Self = Self(Hash32::ZERO);

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    pub fn from_hex(text: &str) -> anyhow::Result<Self> {
        Ok(Self(Hash32::from_hex(text)?))
    }
}

impl fmt::Display for GitCommit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CommitHash(pub Hash32);

impl CommitHash {
    pub const ZERO: Self = Self(Hash32::ZERO);

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }

    pub fn to_hex(self) -> String {
        self.0.to_hex()
    }

    pub fn from_hex(text: &str) -> anyhow::Result<Self> {
        Ok(Self(Hash32::from_hex(text)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_hex_round_trip() {
        let hash = Hash32::sha256(b"arche");
        assert_eq!(Hash32::from_hex(&hash.to_hex()).unwrap(), hash);
    }

    #[test]
    fn zero_hash_is_none() {
        assert!(Hash32::ZERO.is_zero());
        assert!(!Hash32::sha256(b"").is_zero());
    }
}
