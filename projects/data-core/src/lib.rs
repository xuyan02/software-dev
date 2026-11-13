//! A self-contained, JavaScript-like data model.
//!
//! [`Value`] owns heap objects through a reference-counted raw pointer. Small
//! scalars are stored inline; strings, arrays and objects live on the heap.
//! Heap objects are immutable; cycles leak.
//!
//! [`Object`] preserves insertion order and maps `Box<str>` keys to [`Value`].
//! [`Path`] compiles dot-separated lookups once and reads values without
//! cloning the underlying data.

mod error;
mod path;
mod value;

pub use error::ValueError;
pub use path::{Path, PathSegment};
pub use value::{Object, Value, ValueType};
