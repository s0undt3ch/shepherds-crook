pub mod cli;
pub mod ctx;
pub mod error;
pub mod run;

/// `crook hook` must exit 0 whatever its arguments (ADR 0020), so `main` checks for it before
/// clap parses and can exit 2 on a usage error.
pub fn is_hook(args: &[std::ffi::OsString]) -> bool {
    args.get(1).is_some_and(|a| a == "hook")
}

pub use ctx::Ctx;
pub use error::Error;

/// Environment lookup, injected so library code never reads the process environment and tests
/// need no `set_var`.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Runs a parsed command. Every command is a stub until its task lands.
pub fn dispatch(command: &cli::Command, _ctx: &Ctx) -> Result<(), Error> {
    Err(Error::NotImplemented(command.name()))
}
