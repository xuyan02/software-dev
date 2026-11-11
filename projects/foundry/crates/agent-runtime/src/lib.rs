#![recursion_limit = "512"]

//! Foundry's own agent runtime.
//!
//! The runtime owns the agent loop, model gateway, tool registry, context
//! budget and trajectory. It deliberately does not depend on an agent
//! framework: providers, protocols and transports are thin adapters.

pub mod budget;
pub mod context;
pub mod error;
pub mod eval;
pub mod foundry;
pub mod foundry_anchor_document;
pub mod foundry_dispatch;
pub mod foundry_issue;
pub mod foundry_project;
pub mod foundry_task_tools;
pub mod interaction;
pub mod mcp;
pub mod model;
pub mod openai;
pub mod prompts;
pub mod runtime;
pub mod sandbox;
pub mod scripted;
pub mod skills;
pub mod tool;
pub mod tools;
pub mod trajectory;
pub mod worker;

pub use budget::*;
pub use context::*;
pub use error::*;
pub use eval::*;
pub use foundry::*;
pub use foundry_anchor_document::*;
pub use foundry_dispatch::*;
pub use foundry_issue::*;
pub use foundry_project::*;
pub use foundry_task_tools::*;
pub use interaction::*;
pub use mcp::*;
pub use model::*;
pub use openai::*;
pub use prompts::*;
pub use runtime::*;
pub use sandbox::*;
pub use scripted::*;
pub use skills::*;
pub use tool::*;
pub use tools::*;
pub use trajectory::*;
pub use worker::*;
