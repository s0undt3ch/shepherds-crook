use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crook::gate::{self, Entry};
use crook::{Env, hook, modverb};

#[derive(Parser)]
#[command(name = "crook", version, about = "Orchestrates Claude Code sessions")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start or attach to a project's orchestrator
    Init { slug: String },
    /// Shell completions
    Completion {
        #[command(subcommand)]
        action: CompletionCmd,
    },
}

#[derive(Subcommand)]
enum CompletionCmd {
    /// Print a completion script
    Print { shell: Shell },
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let env = |k: &str| std::env::var(k).ok();
    let code = run(&args, &env);
    ExitCode::from(code)
}

fn run(args: &[OsString], env: Env) -> u8 {
    // `hook` and `mod` are routed before clap, which exits 2 on a bad argument.
    // With `onFailure: "block"` that would block every tool call.
    let first = args.get(1).and_then(|a| a.to_str());
    if matches!(first, Some("hook" | "mod")) {
        let rest: Vec<String> = args
            .iter()
            .skip(2)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let code = if first == Some("hook") {
            hook::main(&rest, env)
        } else {
            modverb::main(&rest, env)
        };
        return u8::try_from(code).unwrap_or(1);
    }

    let cli = Cli::parse_from(args);
    match cli.command {
        // Touches no state, so it skips the user-verb gate.
        Cmd::Completion {
            action: CompletionCmd::Print { shell },
        } => {
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "crook", &mut std::io::stdout());
            0
        }
        Cmd::Init { slug } => {
            if let Err(e) = gate::check(Entry::User, env) {
                let _ = writeln!(std::io::stderr(), "crook: {e}");
                return 2;
            }
            let _ = writeln!(std::io::stderr(), "crook: init {slug}: not implemented");
            1
        }
    }
}
