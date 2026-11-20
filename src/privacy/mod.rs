//! The privacy check: what counts as private, and which paths a tool call names.

mod chunks;
#[cfg(test)]
mod fixture;
mod index;
mod paths;
mod set;
mod subtract;

pub use index::{ChunkHit, Index, MAX_FILES};
pub use paths::{Hit, HitKind, find_paths};
pub use set::Set;
pub use subtract::subtract;
