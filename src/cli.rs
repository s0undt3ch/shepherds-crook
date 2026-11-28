//! The command line, as clap derive definitions. The subcommands are the table in ADR 0020 plus
//! `setup` and `doctor` (ADR 0019).

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "crook",
    version,
    about = "Orchestrates Claude Code sessions",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a flock, then open it
    New {
        slug: String,
        /// What the flock is for, passed to the coordinator
        #[arg(long)]
        task: Option<String>,
    },
    /// Start a flock's coordinator terminal
    Open { slug: String },
    /// Mark a flock closed
    Close { slug: String },
    /// Start a child session
    Spawn {
        role: String,
        child: String,
        /// The brief file, relative to the flock folder
        brief: String,
        /// Run the child in a new worktree of this repository
        #[arg(long)]
        repo: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        /// The flock, when not run from one of its terminals
        #[arg(long)]
        flock: Option<String>,
    },
    /// End a child and mark it done
    Stop {
        child: String,
        #[arg(long)]
        flock: Option<String>,
    },
    /// Type a line into a live child of the flock (never --force)
    Send {
        child: String,
        text: String,
        #[arg(long)]
        flock: Option<String>,
    },
    /// Print a live child's screen
    Read {
        child: String,
        /// Lines of history above the screen
        #[arg(long, default_value_t = 0)]
        lines: u32,
        #[arg(long)]
        flock: Option<String>,
    },
    /// Resume one child, or every active child without a live terminal
    Resume {
        child: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        flock: Option<String>,
    },
    /// Show a flock's roster, terminals and results
    Status {
        slug: Option<String>,
        #[arg(long)]
        json: bool,
        #[arg(long, conflicts_with = "slug")]
        flock: Option<String>,
    },
    /// Run a plugin hook: session-start, stop or user-prompt-submit
    Hook {
        // A string, not an enum: a clap usage error would exit 2, and a hook must exit 0 (0020).
        event: String,
    },
    /// Shell completion
    #[command(subcommand)]
    Completion(Completion),
    /// Install the herdr-app mod and print the steps to finish setup
    Setup,
    /// Check the install
    Doctor {
        #[arg(long)]
        fix: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum Completion {
    /// Print the completion script for a shell
    Print {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// What the completion scripts offer for a kind of word, one per line (not in `--help`, Q35)
    #[command(hide = true)]
    List {
        #[arg(value_enum)]
        kind: crate::cmd::completion::Kind,
        #[arg(long)]
        flock: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Command {
    /// Whether `main` points the `current` link at this install before the command runs. `doctor`
    /// reports the link and only `doctor --fix` moves it; `setup` moves it as part of its own work (D83).
    pub fn refreshes_link(&self) -> bool {
        !matches!(self, Command::Doctor { .. } | Command::Setup)
    }

    /// The name used in "not implemented" messages.
    pub fn name(&self) -> &'static str {
        match self {
            Command::New { .. } => "new",
            Command::Open { .. } => "open",
            Command::Close { .. } => "close",
            Command::Spawn { .. } => "spawn",
            Command::Stop { .. } => "stop",
            Command::Send { .. } => "send",
            Command::Read { .. } => "read",
            Command::Resume { .. } => "resume",
            Command::Status { .. } => "status",
            Command::Hook { .. } => "hook",
            Command::Completion(_) => "completion",
            Command::Setup => "setup",
            Command::Doctor { .. } => "doctor",
        }
    }
}

#[cfg(test)]
mod tests;
