#![recursion_limit = "512"]

//! Foundry's agent runtime.
//!
//! Foundry owns its agent definitions, prompts, tools and business integration.
//! Model messages, providers, tool execution and the LLM tool loop come from
//! `graph-agent`.

pub mod artifact;
pub mod codesummary;
pub mod context;
pub mod design_baseline;
pub mod deterministic;
pub mod error;
pub mod eval;
pub mod file;
pub mod foundry;
pub mod foundry_dispatch;
pub mod foundry_issue;
pub mod foundry_merge_request;
pub mod foundry_project;
pub mod foundry_task_tools;
pub mod graph_tools;
pub mod interaction;
pub mod mcp;
pub mod prompts;
pub mod runtime;
pub mod sandbox;
pub mod shell;
pub mod skills;
pub mod tool;
pub mod tools;
pub mod workspace;

pub use artifact::*;
pub use context::*;
pub use deterministic::*;
pub use error::*;
pub use eval::*;
pub use file::*;
pub use foundry::*;
pub use foundry_dispatch::*;
pub use foundry_issue::*;
pub use foundry_merge_request::*;
pub use foundry_project::*;
pub use foundry_task_tools::*;
pub use graph_tools::*;
pub use interaction::*;
pub use mcp::*;
pub use prompts::*;
pub use runtime::*;
pub use sandbox::*;
pub use shell::*;
pub use skills::*;
pub use tool::*;
pub use tools::*;
pub use workspace::*;
