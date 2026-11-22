//! One integration test binary, so the shared helpers build and link once.
//! Modules mirror `src/` where a module has tests here.

mod cli;
mod common;
mod gate;
mod herdr;
mod hook;
mod modverb;
mod store;
