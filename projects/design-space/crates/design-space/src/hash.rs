use crate::error::{Error, Result};
use sha2::{Digest, Sha256};

/// A SHA-256 digest. Every node in the store is addressed by one of these.
pub type Hash = [u8; 32];

pub const ZERO_HASH: Hash = [0u8; 32];

pub fn hash(bytes: &[u8]) -> Hash {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn is_zero(h: &Hash) -> bool {
    h.iter().all(|byte| *byte == 0)
}

pub fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Format a stored commit for git.
///
/// Commits are stored as 32 bytes: SHA-1 is left-padded with 12 zero bytes,
/// so it is printed as the 40-character original; SHA-256 keeps 64.
pub fn to_commit_hex(h: &Hash) -> String {
    if h[..12].iter().all(|byte| *byte == 0) {
        to_hex(&h[12..])
    } else {
        to_hex(h)
    }
}

pub fn from_hex(input: &str) -> Result<Hash> {
    if input.len() != 64 {
        return Err(Error::Hex(format!(
            "expected 64 hex characters, got {}",
            input.len()
        )));
    }

    let mut out = [0u8; 32];
    for (index, chunk) in input.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).map_err(|error| Error::Hex(error.to_string()))?;
        out[index] =
            u8::from_str_radix(text, 16).map_err(|error| Error::Hex(format!("{text}: {error}")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let h = hash(b"design");
        let text = to_hex(&h);
        assert_eq!(text.len(), 64);
        assert_eq!(from_hex(&text).unwrap(), h);
    }

    #[test]
    fn rejects_bad_hex() {
        assert!(from_hex("abc").is_err());
        assert!(from_hex(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn sha1_commits_print_without_padding() {
        let mut stored = [0u8; 32];
        for (index, byte) in stored.iter_mut().enumerate().skip(12) {
            *byte = index as u8;
        }
        assert_eq!(to_commit_hex(&stored).len(), 40);
        assert_eq!(to_commit_hex(&stored), to_hex(&stored[12..]));
        assert_eq!(to_commit_hex(&hash(b"design")).len(), 64);
    }

    #[test]
    fn zero_hash_is_recognised() {
        assert!(is_zero(&ZERO_HASH));
        assert!(!is_zero(&hash(b"x")));
    }
}
