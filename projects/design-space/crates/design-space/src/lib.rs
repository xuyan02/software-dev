//! A git-like, versioned design space for code elements.
//!
//! The store is built from immutable, content-addressed nodes:
//!
//! - [`node::Node::Object`] — one language element;
//! - [`node::Node::Design`] — the design text of one element;
//! - [`node::Node::Index`] — a sorted index node that forms a view tree;
//! - [`node::Node::Version`] — one snapshot pointer.
//!
//! A version points at a virtual root index node. The virtual root names one
//! index tree per view (`namespaces`, `files`, `packages`, ...). Index nodes
//! hash their entries, so a change anywhere below a node changes that node and
//! every index node above it.

pub mod branch;
pub mod element;
pub mod error;
pub mod gc;
pub mod hash;
pub mod language;
pub mod node;
pub mod pack;
pub mod remote;
pub mod source;
pub mod space;
pub mod store;
pub mod types;

pub use error::{Error, Result};
