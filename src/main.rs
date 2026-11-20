use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crook::cli::{self, project::ProjectCmd};
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
    /// Create a project record
    Init { slug: String },
    /// Edit a project record
    Project {
        slug: String,
        #[command(subcommand)]
        action: ProjectCmd,
    },
    /// Lift a child's park by hand: `<child>` or `<slug>/<child>`
    Unpark { child: String },
    /// One line per project
    List,
    /// Live children across projects
    Status,
    /// Pick the child that has waited longest
    Next,
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
            let event = rest.first().map_or("", String::as_str);
            hook::main(
                event,
                &mut std::io::stdin().lock(),
                env,
                &mut std::io::stdout().lock(),
            )
        } else {
            modverb::main(
                &rest,
                env,
                &mut std::io::stdin().lock(),
                &mut std::io::stdout().lock(),
            )
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
        Cmd::Init { slug } => user_verb(env, |env| cli::init::run(env, &slug)),
        Cmd::List => user_verb(env, cli::list::run),
        Cmd::Status => user_verb(env, cli::status::run),
        Cmd::Next => user_verb(env, cli::next::run),
        Cmd::Unpark { child } => user_verb(env, |env| cli::unpark::run(env, &child)),
        Cmd::Project { slug, action } => user_verb(env, |env| {
            let cwd = std::env::current_dir()?;
            cli::project::run(env, &slug, action, &cwd, &mut std::io::stdin().lock())
        }),
    }
}

/// Runs a user verb behind the gate: exit 2 when refused, 1 on any other error.
fn user_verb(env: Env, f: impl FnOnce(Env) -> Result<String, crook::Error>) -> u8 {
    if let Err(e) = gate::check(Entry::User, env) {
        let _ = writeln!(std::io::stderr(), "crook: {e}");
        return 2;
    }
    match f(env) {
        Ok(msg) => {
            let _ = writeln!(std::io::stdout(), "{msg}");
            0
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "crook: {e}");
            1
        }
    }
}
