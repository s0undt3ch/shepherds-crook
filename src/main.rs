use std::ffi::OsString;
use std::process::ExitCode;

use clap::Parser;
use crook::ctx::SystemClock;
use crook::run::RealRunner;
use crook::{Ctx, cli, install};

/// The only place the process environment is read (see `clippy.toml`).
#[allow(clippy::disallowed_methods)]
fn process_env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

/// Every variable, text or not, so `hc server start` loses only what the scrub removes.
#[allow(clippy::disallowed_methods)]
fn process_vars() -> Vec<(OsString, OsString)> {
    std::env::vars_os().collect()
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let ctx = Ctx {
        env: &process_env,
        vars: &process_vars,
        runner: &RealRunner,
        clock: &SystemClock,
        current_exe: &std::env::current_exe,
    };
    // Once per run, hooks included; a problem is a warning and never stops the command.
    for warning in install::refresh(&ctx) {
        eprintln!("{warning}");
    }
    if crook::is_hook(&args) {
        eprintln!("crook hook: not implemented");
        return ExitCode::SUCCESS;
    }
    let cli = cli::Cli::parse_from(&args);
    match crook::dispatch(&cli.command, &ctx) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
