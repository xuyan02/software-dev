use std::ops::Deref;

use crate::hash::{ObjectHash, SymbolHash};

/// A reference to a name or qname symbol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StringRef(pub SymbolHash);

impl StringRef {
    pub const ZERO: Self = Self(SymbolHash::ZERO);

    pub const fn get(self) -> SymbolHash {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

impl From<SymbolHash> for StringRef {
    fn from(value: SymbolHash) -> Self {
        Self(value)
    }
}

impl From<StringRef> for SymbolHash {
    fn from(value: StringRef) -> Self {
        value.0
    }
}

/// A reference to another design object.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectRef(pub ObjectHash);

impl ObjectRef {
    pub const ZERO: Self = Self(ObjectHash::ZERO);

    pub const fn get(self) -> ObjectHash {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0.is_zero()
    }
}

impl From<ObjectHash> for ObjectRef {
    fn from(value: ObjectHash) -> Self {
        Self(value)
    }
}

impl From<ObjectRef> for ObjectHash {
    fn from(value: ObjectRef) -> Self {
        value.0
    }
}

/// An ordered list of name/qname symbol references.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StringRefList(pub Vec<SymbolHash>);

impl StringRefList {
    pub fn as_slice(&self) -> &[SymbolHash] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<SymbolHash> {
        self.0
    }
}

impl Deref for StringRefList {
    type Target = [SymbolHash];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a StringRefList {
    type Item = &'a SymbolHash;
    type IntoIter = std::slice::Iter<'a, SymbolHash>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// An ordered list of object references.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectRefList(pub Vec<ObjectHash>);

impl ObjectRefList {
    pub fn as_slice(&self) -> &[ObjectHash] {
        &self.0
    }

    pub fn into_vec(self) -> Vec<ObjectHash> {
        self.0
    }
}

impl Deref for ObjectRefList {
    type Target = [ObjectHash];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> IntoIterator for &'a ObjectRefList {
    type Item = &'a ObjectHash;
    type IntoIter = std::slice::Iter<'a, ObjectHash>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
