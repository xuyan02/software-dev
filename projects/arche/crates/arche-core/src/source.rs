use crate::hash::SymbolHash;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourcePath {
    pub path: SymbolHash,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceSpan {
    pub path: SymbolHash,
    pub start: u32,
    pub end: u32,
}
