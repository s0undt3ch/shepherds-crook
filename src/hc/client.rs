//! The real [`Hc`]: argv for each call, and parsing of what hc prints. Parsing is lenient: unknown
//! fields are ignored and missing optional ones are `None` (ADR 0016: hc is at 0.1.x and may grow).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::Duration;

use serde::Deserialize;

use super::{Hc, KillOutcome, Parent, RunSpec, Schema, SendOutcome, Status, TermState, Terminal};
use crate::Error;
use crate::run::{Cmd, Output, Runner};

#[cfg(test)]
mod tests;

/// Every hc call gets this long. `hc send` can wait on a person's draft for longer (Q25).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

pub struct HcClient<'a> {
    runner: &'a dyn Runner,
    program: String,
    timeout: Duration,
}

impl<'a> HcClient<'a> {
    pub fn new(runner: &'a dyn Runner) -> Self {
        HcClient {
            runner,
            program: "hc".to_string(),
            timeout: CALL_TIMEOUT,
        }
    }

    /// For tests: a fake `hc` script instead of the one on `PATH`.
    pub fn with_program(mut self, program: String) -> Self {
        self.program = program;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn cmd(&self, args: Vec<String>) -> Cmd {
        Cmd::new(self.program.clone()).args(args)
    }

    fn call(&self, cmd: Cmd) -> Result<Output, Error> {
        self.runner.run(cmd, self.timeout)
    }

    /// A call whose non-zero exit is an error carrying hc's own kind and message.
    fn call_ok(&self, args: Vec<String>) -> Result<Output, Error> {
        let out = self.call(self.cmd(args.clone()))?;
        if out.is_success() {
            Ok(out)
        } else {
            Err(hc_error(&args, &out))
        }
    }
}

/// hc prints `{"error":{"kind":…,"message":…}}` on stderr; anything else is shown as it is.
fn error_parts(out: &Output) -> (Option<String>, String) {
    if let Ok(e) = serde_json::from_str::<ErrorEnvelope>(out.stderr.trim()) {
        return (e.error.kind, e.error.message);
    }
    let text = out.stderr.trim();
    if !text.is_empty() {
        return (None, text.to_string());
    }
    match out.code {
        Some(code) => (None, format!("exited with {code}")),
        None => (None, "ended by a signal".to_string()),
    }
}

fn hc_error(args: &[String], out: &Output) -> Error {
    let (kind, message) = error_parts(out);
    Error::Hc {
        args: args.join(" "),
        kind,
        message,
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    message: String,
}

fn parse_error(args: &[String], what: &str, e: impl std::fmt::Display) -> Error {
    Error::Hc {
        args: args.join(" "),
        kind: None,
        message: format!("could not read what hc printed ({what}): {e}"),
    }
}

/// The `status` of `hc ls` is a bare word; in `hc get` it is a record.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawStatus {
    Word(String),
    Record {
        #[serde(default)]
        state: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        app: Option<String>,
    },
}

#[derive(Deserialize)]
struct RawTerminal {
    address: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    status: Option<RawStatus>,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    signal: Option<i32>,
}

impl From<RawTerminal> for Terminal {
    fn from(r: RawTerminal) -> Terminal {
        Terminal {
            address: r.address,
            name: r.name,
            parent: r.parent,
            state: match r.state.as_deref() {
                Some("live") => TermState::Live,
                Some("finished") => TermState::Finished,
                _ => TermState::Unknown,
            },
            status: r.status.map(|s| match s {
                RawStatus::Word(state) => Status {
                    state,
                    kind: None,
                    app: None,
                },
                RawStatus::Record { state, kind, app } => Status { state, kind, app },
            }),
            labels: r.labels,
            agent: r.agent,
            cwd: r.cwd,
            reason: r.reason,
            exit_code: r.exit_code,
            signal: r.signal,
        }
    }
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default)]
    terminals: Vec<RawTerminal>,
}

fn parse_terminal(args: &[String], out: &Output) -> Result<Terminal, Error> {
    serde_json::from_str::<RawTerminal>(&out.stdout)
        .map(Terminal::from)
        .map_err(|e| parse_error(args, "a terminal", e))
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_string()).collect()
}

/// An id or name that starts with `-` would be read as a flag by hc.
fn check_id(args: &[String], id: &str) -> Result<(), Error> {
    if id.is_empty() || id.starts_with('-') {
        return Err(Error::Hc {
            args: args.join(" "),
            kind: None,
            message: format!("{id:?} is not a terminal name or address"),
        });
    }
    Ok(())
}

fn not_found(out: &Output) -> bool {
    out.code == Some(3)
}

impl Hc for HcClient<'_> {
    fn version(&self) -> Result<String, Error> {
        let args = strings(&["--version"]);
        let out = self.call_ok(args.clone())?;
        out.stdout
            .split_whitespace()
            .nth(1)
            .map(str::to_string)
            .ok_or_else(|| parse_error(&args, "a version", format!("{:?}", out.stdout.trim())))
    }

    fn schema(&self) -> Result<Schema, Error> {
        let args = strings(&["schema"]);
        let out = self.call_ok(args.clone())?;
        serde_json::from_str(&out.stdout).map_err(|e| parse_error(&args, "the schema", e))
    }

    fn ls(&self, all: bool) -> Result<Vec<Terminal>, Error> {
        self.ls_by_label(all, &[])
    }

    fn ls_by_label(&self, all: bool, labels: &[(String, String)]) -> Result<Vec<Terminal>, Error> {
        let mut args = strings(&["ls", "--json"]);
        if all {
            args.push("--all".to_string());
        }
        for (k, v) in labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        let out = self.call_ok(args.clone())?;
        let list: RawList = serde_json::from_str(&out.stdout)
            .map_err(|e| parse_error(&args, "a terminal list", e))?;
        Ok(list.terminals.into_iter().map(Terminal::from).collect())
    }

    fn get(&self, id: &str) -> Result<Option<Terminal>, Error> {
        let args = vec!["get".to_string(), "--json".to_string(), id.to_string()];
        check_id(&args, id)?;
        let out = self.call(self.cmd(args.clone()))?;
        if not_found(&out) {
            return Ok(None);
        }
        if !out.is_success() {
            return Err(hc_error(&args, &out));
        }
        parse_terminal(&args, &out).map(Some)
    }

    fn run(&self, spec: &RunSpec) -> Result<Terminal, Error> {
        let mut args = strings(&["run", "--bg", "--json", "--name"]);
        args.push(spec.name.clone());
        args.push("--cwd".to_string());
        args.push(spec.cwd.display().to_string());
        for (k, v) in &spec.labels {
            args.push("--label".to_string());
            args.push(format!("{k}={v}"));
        }
        match &spec.parent {
            Parent::Terminal(t) => {
                args.push("--parent".to_string());
                args.push(t.clone());
            }
            Parent::None => args.push("--no-parent".to_string()),
        }
        args.push("--".to_string());
        args.extend(spec.argv.iter().cloned());
        let out = self.call_ok(args.clone())?;
        parse_terminal(&args, &out)
    }

    fn send(&self, to: &str, text: &str) -> Result<SendOutcome, Error> {
        let args = vec![
            "send".to_string(),
            "--json".to_string(),
            "--".to_string(),
            to.to_string(),
            text.to_string(),
        ];
        check_id(&args, to)?;
        let out = self.call(self.cmd(args.clone()))?;
        Ok(match out.code {
            Some(0) => SendOutcome::Sent,
            Some(9) => SendOutcome::Blocked,
            Some(11) => SendOutcome::Finished,
            Some(3) => SendOutcome::NotFound,
            _ => SendOutcome::Failed(error_parts(&out).1),
        })
    }

    fn kill(&self, id: &str) -> Result<KillOutcome, Error> {
        let args = vec!["kill".to_string(), "--json".to_string(), id.to_string()];
        check_id(&args, id)?;
        let out = self.call(self.cmd(args.clone()))?;
        if not_found(&out) {
            return Ok(KillOutcome::NotFound);
        }
        if out.is_success() {
            Ok(KillOutcome::Killed)
        } else {
            Err(hc_error(&args, &out))
        }
    }

    fn server_start(&self, env: &[(OsString, OsString)]) -> Result<(), Error> {
        let args = strings(&["server", "start", "--json"]);
        let out = self.call(self.cmd(args.clone()).env(env.iter().cloned()))?;
        if out.is_success() {
            Ok(())
        } else {
            Err(hc_error(&args, &out))
        }
    }
}
