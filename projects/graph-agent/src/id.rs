use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fmt;

/// Stable name of a node inside one [`crate::Graph`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for NodeId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Index of one of a node's local transition targets.
///
/// This is not a global node index. The same value means different targets on
/// different nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TransitionIndex(pub usize);

impl TransitionIndex {
    pub const ZERO: Self = Self(0);

    pub const fn new(index: usize) -> Self {
        Self(index)
    }
}

impl From<usize> for TransitionIndex {
    fn from(value: usize) -> Self {
        Self(value)
    }
}

impl fmt::Display for TransitionIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
