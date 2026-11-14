//! One extracted language element or file object.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub name: String,
    pub ty: u32,
    pub path: String,
    /// Code commit, as hex.
    pub commit: String,
    pub start_byte: u64,
    pub end_byte: u64,
    pub start_line: u32,
    pub end_line: u32,
    /// The element's own text: children and doc comments already removed by
    /// the extractor.
    pub own_text: String,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub design: Option<String>,
    /// Precomputed content hash, set when the element comes from an unchanged
    /// file in the base version. `own_text` is then empty and no parsing
    /// happened; input hash is still derived from children designs at commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}
