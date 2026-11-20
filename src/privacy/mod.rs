//! The privacy check: what counts as private, and which paths a tool call names.

mod paths;
mod set;

pub use paths::{Hit, HitKind, find_paths};
pub use set::Set;
