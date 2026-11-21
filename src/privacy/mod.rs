//! The privacy check: what counts as private, and which paths a tool call names.

mod paths;
mod set;

pub use paths::{Hit, HitKind, find_paths, find_paths_by};
pub use set::{Set, names_home, resolve_path};
