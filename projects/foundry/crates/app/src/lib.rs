//! Application layer: use cases and ports.
//!
//! The application layer orchestrates domain objects and talks to the outside
//! world only through traits declared here. It does not know whether storage is
//! Postgres or memory, whether events go to Kafka or an in-process channel.

pub mod account_service;
pub mod auth;
pub mod auth_service;
pub mod commands;
pub mod error;
pub mod issue_service;
pub mod ports;
pub mod project_service;
pub mod queries;
pub mod service;
pub mod task;
pub mod workflow;

pub use account_service::*;
pub use auth::*;
pub use auth_service::*;
pub use commands::*;
pub use error::*;
pub use issue_service::*;
pub use ports::*;
pub use project_service::*;
pub use queries::*;
pub use service::*;
pub use task::*;
pub use workflow::*;
