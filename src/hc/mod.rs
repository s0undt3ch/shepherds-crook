//! hc, the terminal runtime crook drives (ADR 0007, 0016). [`Hc`] is what the commands and hooks
//! depend on; [`client::HcClient`] is the real one over a [`crate::run::Runner`], and the in-memory
//! fake in tests stands in for a core.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use serde::Deserialize;

use crate::Error;

pub mod client;
#[cfg(test)]
pub mod fake;
#[cfg(test)]
mod fixtures;
pub mod preflight;
pub mod scrub;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TermState {
    Live,
    Finished,
    /// A state word this crook does not know.
    #[default]
    Unknown,
}

/// What the program running in a terminal said about itself (its OSC 7501 record). `hc ls` prints
/// only the state word, so `kind` and `app` are filled in by `get`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub kind: Option<String>,
    pub app: Option<String>,
}

/// A terminal as hc reports it. `hc ls` gives a summary: no labels and a bare status word. `hc get`
/// gives the full record. Fields hc does not send are `None` or empty.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Terminal {
    /// `t7`: the terminal for good on this core.
    pub address: String,
    pub name: Option<String>,
    /// The parent's address, not its name.
    pub parent: Option<String>,
    pub state: TermState,
    pub status: Option<Status>,
    pub labels: BTreeMap<String, String>,
    pub agent: Option<String>,
    pub cwd: Option<String>,
    /// Why a finished terminal ended: `exited`, `killed`, `server_stop` and so on.
    pub reason: Option<String>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

impl Terminal {
    pub fn is_live(&self) -> bool {
        self.state == TermState::Live
    }

    /// A bare shell, which is what a lost terminal comes back as after a core crash or a reboot: same
    /// address, name and labels, none of the original program (D52). `hc ls` names the program in
    /// `agent` (`sh` for a shell, `claude` once the session reports itself), and `status.app` is
    /// set only by a program that speaks OSC 7501. A launch through `env` shows as `env`, so only
    /// a known shell name counts: an unrecognised program is never mistaken for a shell and killed.
    pub fn is_shell(&self) -> bool {
        const SHELLS: [&str; 10] = [
            "sh", "bash", "zsh", "fish", "dash", "ksh", "csh", "tcsh", "nu", "pwsh",
        ];
        let reports = self.status.as_ref().is_some_and(|s| s.app.is_some());
        !reports && self.agent.as_deref().is_some_and(|a| SHELLS.contains(&a))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parent {
    Terminal(String),
    None,
}

/// One `hc run --bg`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunSpec {
    pub name: String,
    pub cwd: PathBuf,
    pub labels: Vec<(String, String)>,
    pub parent: Parent,
    pub argv: Vec<String>,
}

/// What `hc send` did, from its exit code (spike 2): 0, 9, 11 and 3, and anything else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    /// A question is on the screen; typing would answer it.
    Blocked,
    Finished,
    NotFound,
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillOutcome {
    /// hc knew the terminal and it is finished now (killing a finished one is not an error).
    Killed,
    NotFound,
}

/// The parts of `hc schema` crook checks: which methods take which parameters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Schema {
    #[serde(default)]
    pub methods: Vec<Method>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Method {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub params: Vec<Param>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Param {
    #[serde(default)]
    pub name: String,
}

impl Schema {
    /// A schema with exactly these methods and parameters, for the fake.
    pub fn of(methods: &[(&str, &[&str])]) -> Schema {
        Schema {
            methods: methods
                .iter()
                .map(|(name, params)| Method {
                    name: (*name).to_string(),
                    params: params
                        .iter()
                        .map(|p| Param {
                            name: (*p).to_string(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn has_param(&self, method: &str, param: &str) -> bool {
        self.methods
            .iter()
            .any(|m| m.name == method && m.params.iter().any(|p| p.name == param))
    }
}

/// One method per hc call crook makes. Nothing outside [`client`] runs `hc`.
pub trait Hc {
    /// The bare version, `0.1.22`.
    fn version(&self) -> Result<String, Error>;
    fn schema(&self) -> Result<Schema, Error>;
    /// Live terminals, or every terminal with `all`. Entries carry no labels.
    fn ls(&self, all: bool) -> Result<Vec<Terminal>, Error>;
    /// As [`Hc::ls`], only the terminals carrying every one of `labels`.
    fn ls_by_label(&self, all: bool, labels: &[(String, String)]) -> Result<Vec<Terminal>, Error>;
    /// `None` when hc has no such terminal.
    fn get(&self, id: &str) -> Result<Option<Terminal>, Error>;
    fn run(&self, spec: &RunSpec) -> Result<Terminal, Error>;
    fn send(&self, to: &str, text: &str) -> Result<SendOutcome, Error>;
    /// The terminal's screen, with `lines` of history above it.
    fn read(&self, id: &str, lines: u32) -> Result<String, Error>;
    fn kill(&self, id: &str) -> Result<KillOutcome, Error>;
    /// Starts the core, or reports the running one. `env` is the whole environment of the call, and
    /// fixes the environment every later terminal gets (spike 23).
    fn server_start(&self, env: &[(OsString, OsString)]) -> Result<(), Error>;
}
