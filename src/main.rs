use std::ffi::OsString;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::fd::AsFd;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crook::cli::{self, project::ProjectCmd};
use crook::gate::{self, Entry};
use crook::launch::{self, Host, Outcome};
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
    /// Start the project's orchestrator
    Open { slug: String },
    /// Restart a child's session: `<child>` or `<slug>/<child>`
    Resume { child: String },
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

/// Stands in for stdout when it cannot be duplicated (no free descriptor):
/// every write fails, so an answer that cannot be given exits 2. A closed fd 1
/// never gets here: the Rust runtime reopens it on `/dev/null` at start.
struct NoStdout;

impl Write for NoStdout {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(nix::errno::Errno::EBADF.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(nix::errno::Errno::EBADF.into())
    }
}

/// The hook's stdout as a plain file. `std::io::stdout()` reports success on
/// `EBADF`, which would turn an unwritable answer into no answer at all.
fn hook_stdout() -> Box<dyn Write> {
    match std::io::stdout().as_fd().try_clone_to_owned() {
        Ok(fd) => Box::new(BufWriter::new(File::from(fd))),
        Err(_) => Box::new(NoStdout),
    }
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
            hook::main(event, &mut std::io::stdin().lock(), env, &mut hook_stdout())
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
        Cmd::Open { slug } => launch_verb(env, |env, host| cli::open::plan(env, &slug, host)),
        Cmd::Resume { child } => launch_verb(env, |env, host| cli::resume::plan(env, &child, host)),
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

/// Runs a launch verb behind the gate. A start prints its notes on stderr, then
/// replaces this process with `claude`, so it returns only when that fails.
fn launch_verb(env: Env, f: impl FnOnce(Env, &Host) -> Result<Outcome, crook::Error>) -> u8 {
    if let Err(e) = gate::check(Entry::User, env) {
        let _ = writeln!(std::io::stderr(), "crook: {e}");
        return 2;
    }
    match launch::with_real_host(env, |host| f(env, host)).and_then(|r| r) {
        Ok(Outcome::Say(msg)) => {
            let _ = writeln!(std::io::stdout(), "{msg}");
            0
        }
        Ok(Outcome::Exec(plan)) => {
            for note in &plan.notes {
                let _ = writeln!(std::io::stderr(), "{note}");
            }
            let e = plan.exec();
            let _ = writeln!(std::io::stderr(), "crook: cannot start claude: {e}");
            1
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "crook: {e}");
            1
        }
    }
}
