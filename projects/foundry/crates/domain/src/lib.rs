//! Pure domain model for Foundry.
//!
//! This crate must not depend on I/O, async runtimes, or web frameworks.
//! If a type needs a database, an HTTP request, or a clock that can fail,
//! it belongs in an adapter or the application layer.

pub mod account;
pub mod agent;
pub mod decision;
pub mod document;
pub mod event;
pub mod issue;
pub mod link;
pub mod project;
pub mod project_brief;
pub mod requirements_doc;
pub mod state_machine;
pub mod task;
pub mod task_link;
pub mod work_item;
pub mod workflow;

pub use account::*;
pub use agent::*;
pub use decision::*;
pub use document::*;
pub use event::*;
pub use issue::*;
pub use link::*;
pub use project::*;
pub use project_brief::*;
pub use requirements_doc::*;
pub use state_machine::*;
pub use task::*;
pub use task_link::*;
pub use work_item::*;
pub use workflow::*;

use serde::{Deserialize, Serialize};

/// Who performed an action. Humans and agents are both first-class actors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    Human { id: String, name: Option<String> },
    Agent { id: String, role: String },
    System,
}

impl Actor {
    pub fn human(id: impl Into<String>) -> Self {
        Self::Human {
            id: id.into(),
            name: None,
        }
    }

    pub fn agent(id: impl Into<String>, role: impl Into<String>) -> Self {
        Self::Agent {
            id: id.into(),
            role: role.into(),
        }
    }

    pub fn is_human(&self) -> bool {
        matches!(self, Self::Human { .. })
    }

    pub fn is_agent(&self) -> bool {
        matches!(self, Self::Agent { .. })
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Human { id, .. } | Self::Agent { id, .. } => id,
            Self::System => "system",
        }
    }
}
