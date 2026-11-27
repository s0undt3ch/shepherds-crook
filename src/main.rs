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
        new_id: &|| uuid::Uuid::new_v4().to_string(),
    };
    if crook::is_hook(&args) {
        // Whatever happens inside, a hook exits 0 (ADR 0020).
        crook::hook::main(
            &ctx,
            &args
                .get(2)
                .map_or_else(String::new, |a| a.to_string_lossy().into_owned()),
        );
        return ExitCode::SUCCESS;
    }
    // Once per run, as the hook does inside its own time budget; a problem is a warning and never stops the command.
    for warning in install::refresh(&ctx) {
        eprintln!("{warning}");
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
