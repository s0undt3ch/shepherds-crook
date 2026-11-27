use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;
use crook::ctx::SystemClock;
use crook::run::RealRunner;
use crook::{Ctx, cli};

/// The only place the process environment is read (see `clippy.toml`).
#[allow(clippy::disallowed_methods)]
fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    if crook::is_hook(&args) {
        eprintln!("crook hook: not implemented");
        return ExitCode::SUCCESS;
    }
    let cli = cli::Cli::parse_from(&args);
    let ctx = Ctx {
        env: &process_env,
        runner: &RealRunner,
        clock: &SystemClock,
        current_exe: &std::env::current_exe,
    };
    match crook::dispatch(&cli.command, &ctx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
