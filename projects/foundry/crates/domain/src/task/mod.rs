//! Task aggregate, workflow runtime, and declarative built-in workflows.
//!
//! `model` holds persistence and view types. `definition` holds the AST,
//! validator, interpreter and embedded built-in definitions. `runtime` applies
//! one event to one aggregate and returns a mutation; it performs no I/O.

mod definition;
mod model;
mod runtime;
pub mod script;
mod workflow;

#[cfg(test)]
mod tests;

pub use definition::*;
pub use model::*;
pub use runtime::*;
pub use workflow::*;
