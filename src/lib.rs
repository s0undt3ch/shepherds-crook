#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

pub mod agents;
pub mod card;
pub mod cli;
pub mod config;
pub mod crew;
pub mod error;
pub mod gate;
pub mod hook;
pub mod launch;
pub mod modverb;
pub mod paths;
pub mod project;
pub mod store;

pub use error::Error;

/// Environment lookup, injected so library code never reads the process
/// environment and tests need no `set_var`.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;
