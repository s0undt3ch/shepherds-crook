//! The flock's files (ADR 0024). Only crook writes the JSON ones, and only through [`atomic`].

pub mod atomic;
pub mod marks;
pub mod result;
pub mod roster;
