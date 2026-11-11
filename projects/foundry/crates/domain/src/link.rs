use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkRelation {
    Implements,
    Addresses,
    DependsOn,
    DesignedBy,
    VerifiedBy,
    ReleasedIn,
    CausedBy,
    FixedBy,
    Supersedes,
    Duplicates,
    RelatesTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum LinkTarget {
    Issue(Uuid),
    Task(Uuid),
    DesignDoc(Uuid),
    Adr(Uuid),
    Contract(Uuid),
    Decision(Uuid),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub relation: LinkRelation,
    pub target: LinkTarget,
    #[serde(default)]
    pub note: Option<String>,
}

impl Link {
    pub fn new(relation: LinkRelation, target: LinkTarget) -> Self {
        Self {
            relation,
            target,
            note: None,
        }
    }
}
