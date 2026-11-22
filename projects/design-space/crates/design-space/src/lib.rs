//! A git-like, versioned design space for conceptual design objects.
//!
//! The storage model is defined by `docs/storage-format.md`:
//!
//! - design objects are immutable and content-addressed;
//! - symbols and design documents are parallel content-addressed objects;
//! - commits point at a View root;
//! - worktree/index are mutable local state.
//!
//! [`catalog::Catalog`] reconstructs the containment graph and rebuilds a
//! View after edits.

pub mod catalog;
pub mod cli;
pub mod format;
pub mod lock;
pub mod model;
pub mod pack;
pub mod push;
pub mod remote;
pub mod server;
pub mod source;
pub mod store;
pub mod workspace;

pub use catalog::{Catalog, CatalogError};
pub use format::{FormatError, object_hash};
pub use model::*;
pub use store::{Store, StoreError};
pub use workspace::{Workspace, WorkspaceError};
