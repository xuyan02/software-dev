//! Declarative workflow definitions.
//!
//! The AST is the single source of truth; stages, transitions and actions are
//! data interpreted by [`DefinitionWorkflow`]. Validation runs before publish
//! and the runtime keeps defensive checks.

mod ast;
mod builtin;
mod interpreter;
mod title;
mod validate;

pub use ast::*;
pub use builtin::*;
pub use interpreter::*;
pub use title::*;
pub use validate::{
    MAX_AST_BYTES, MAX_DESCRIPTION_LEN, MAX_DISPLAY_NAME_LEN, MAX_ROLES, MAX_STAGES,
    MAX_TRANSITIONS, Validation, ValidationIssue, validate,
};
