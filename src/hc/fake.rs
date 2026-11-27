//! An in-memory hc core for the tests of commands and hooks: terminals with names, labels,
//! parents and states, every call recorded, and calls that can be told to fail.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;

use super::{Hc, KillOutcome, Parent, RunSpec, Schema, SendOutcome, Status, TermState, Terminal};
use crate::Error;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Version,
    Schema,
    Ls {
        all: bool,
        labels: Vec<(String, String)>,
    },
    Get(String),
    Run(RunSpec),
    Send {
        to: String,
        text: String,
    },
    Kill(String),
    ServerStart(Vec<(OsString, OsString)>),
}

struct State {
    version: String,
    schema: Schema,
    terminals: Vec<Terminal>,
    calls: Vec<Call>,
    failures: HashMap<&'static str, usize>,
    forced_send: Option<SendOutcome>,
    /// The next `run` times out; `true` means the terminal started anyway.
    timeout_run: Option<bool>,
}

pub struct FakeHc {
    state: RefCell<State>,
}

impl Default for FakeHc {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeHc {
    /// A core of the pinned version whose `terminal.run` takes `labels`, with no terminals.
    pub fn new() -> FakeHc {
        FakeHc {
            state: RefCell::new(State {
                version: "0.1.22".to_string(),
                schema: Schema::of(&[("terminal.run", &["argv", "labels", "name"])]),
                terminals: Vec::new(),
                calls: Vec::new(),
                failures: HashMap::new(),
                forced_send: None,
                timeout_run: None,
            }),
        }
    }

    pub fn with_version(self, version: &str) -> FakeHc {
        self.state.borrow_mut().version = version.to_string();
        self
    }

    pub fn without_run_labels(self) -> FakeHc {
        self.state.borrow_mut().schema = Schema::of(&[("terminal.run", &["argv", "name"])]);
        self
    }

    pub fn without_run(self) -> FakeHc {
        self.state.borrow_mut().schema = Schema::of(&[("terminal.list", &[])]);
        self
    }

    /// The next `times` calls named `call` (`version`, `schema`, `ls`, `get`, `run`, `send`,
    /// `kill` or `server_start`) fail.
    pub fn fail(&self, call: &'static str, times: usize) {
        self.state.borrow_mut().failures.insert(call, times);
    }

    /// The next `run` ends in a timeout, as when hc is too slow to answer. With `started`, the
    /// terminal exists all the same, which a caller cannot tell from the timeout alone.
    pub fn time_out_next_run(&self, started: bool) {
        self.state.borrow_mut().timeout_run = Some(started);
    }

    /// Every `send` answers with this, whatever the target.
    pub fn force_send(&self, outcome: SendOutcome) {
        self.state.borrow_mut().forced_send = Some(outcome);
    }

    /// Sets what a live terminal's program reports about itself, as a program speaking OSC 7501 does.
    pub fn set_status(&self, id: &str, state: &str, kind: Option<&str>) {
        let mut s = self.state.borrow_mut();
        if let Some(i) = find(&s.terminals, id) {
            s.terminals[i].status = Some(Status {
                state: state.to_string(),
                kind: kind.map(str::to_string),
                app: Some("claude".to_string()),
            });
        }
    }

    /// Sets the app a live terminal's program reports (`claude-code` for a real Claude Code).
    pub fn set_app(&self, id: &str, app: &str) {
        let mut s = self.state.borrow_mut();
        if let Some(i) = find(&s.terminals, id) {
            let status = s.terminals[i].status.get_or_insert_with(|| Status {
                state: "idle".to_string(),
                kind: None,
                app: None,
            });
            status.app = Some(app.to_string());
        }
    }

    /// Ends a live terminal as its program exiting with `code`.
    pub fn exit(&self, id: &str, code: i32) {
        let mut s = self.state.borrow_mut();
        if let Some(i) = find(&s.terminals, id) {
            s.terminals[i].state = TermState::Finished;
            s.terminals[i].reason = Some("exited".to_string());
            s.terminals[i].exit_code = Some(code);
            s.terminals[i].status = None;
        }
    }

    pub fn calls(&self) -> Vec<Call> {
        self.state.borrow().calls.clone()
    }

    fn record(&self, call: Call, key: &'static str) -> Result<(), Error> {
        let mut s = self.state.borrow_mut();
        s.calls.push(call);
        if let Some(left) = s.failures.get_mut(key).filter(|n| **n > 0) {
            *left -= 1;
            return Err(error(key, None, "scripted failure"));
        }
        Ok(())
    }
}

fn error(call: &str, kind: Option<&str>, message: &str) -> Error {
    Error::Hc {
        args: call.to_string(),
        kind: kind.map(str::to_string),
        message: message.to_string(),
    }
}

/// hc's rule: 1 to 32 of `a-z 0-9 - _ .`, starting with a letter or digit, and not shaped like an
/// address. Checked against hc 0.1.22: `t7`, `t10` and `t1x` are refused, `t0` and `t01` are
/// accepted, so an address is taken to be `t`, a digit 1 to 9, then letters and digits. This mirrors
/// hc, not crook's own (stricter) rule.
fn valid_name(name: &str) -> bool {
    let first = name.bytes().next();
    let ok_chars = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.'));
    let id_shaped = name.strip_prefix('t').is_some_and(|rest| {
        rest.bytes()
            .next()
            .is_some_and(|b| (b'1'..=b'9').contains(&b))
            && rest
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });
    (1..=32).contains(&name.len())
        && ok_chars
        && first.is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && !id_shaped
}

/// The terminal an address or name means: a live one before a finished one, the newest first.
fn find(terminals: &[Terminal], id: &str) -> Option<usize> {
    if let Some(i) = terminals.iter().position(|t| t.address == id) {
        return Some(i);
    }
    let named = |t: &&Terminal| t.name.as_deref() == Some(id);
    let live = terminals.iter().rposition(|t| named(&t) && t.is_live());
    live.or_else(|| terminals.iter().rposition(|t| named(&t)))
}

fn finish(terminals: &mut [Terminal], index: usize) {
    if terminals[index].is_live() {
        terminals[index].state = TermState::Finished;
        terminals[index].reason = Some("killed".to_string());
        terminals[index].signal = Some(1);
        terminals[index].status = None;
        let address = terminals[index].address.clone();
        let kids: Vec<usize> = (0..terminals.len())
            .filter(|&i| terminals[i].parent.as_deref() == Some(address.as_str()))
            .collect();
        for kid in kids {
            finish(terminals, kid);
        }
    }
}

impl Hc for FakeHc {
    fn version(&self) -> Result<String, Error> {
        self.record(Call::Version, "version")?;
        Ok(self.state.borrow().version.clone())
    }

    fn schema(&self) -> Result<Schema, Error> {
        self.record(Call::Schema, "schema")?;
        Ok(self.state.borrow().schema.clone())
    }

    fn ls(&self, all: bool) -> Result<Vec<Terminal>, Error> {
        self.ls_by_label(all, &[])
    }

    fn ls_by_label(&self, all: bool, labels: &[(String, String)]) -> Result<Vec<Terminal>, Error> {
        self.record(
            Call::Ls {
                all,
                labels: labels.to_vec(),
            },
            "ls",
        )?;
        let s = self.state.borrow();
        Ok(s.terminals
            .iter()
            .filter(|t| all || t.is_live())
            .filter(|t| labels.iter().all(|(k, v)| t.labels.get(k) == Some(v)))
            .map(|t| Terminal {
                labels: Default::default(),
                // `hc ls` gives the status word only.
                status: t.status.as_ref().map(|st| Status {
                    state: st.state.clone(),
                    kind: None,
                    app: None,
                }),
                ..t.clone()
            })
            .collect())
    }

    fn get(&self, id: &str) -> Result<Option<Terminal>, Error> {
        self.record(Call::Get(id.to_string()), "get")?;
        let s = self.state.borrow();
        Ok(find(&s.terminals, id).map(|i| s.terminals[i].clone()))
    }

    fn run(&self, spec: &RunSpec) -> Result<Terminal, Error> {
        self.record(Call::Run(spec.clone()), "run")?;
        let mut s = self.state.borrow_mut();
        if !valid_name(&spec.name) {
            return Err(error(
                "run",
                Some("invalid_params"),
                &format!("bad name {:?}", spec.name),
            ));
        }
        if s.terminals
            .iter()
            .any(|t| t.is_live() && t.name.as_deref() == Some(spec.name.as_str()))
        {
            return Err(error(
                "run",
                Some("name_taken"),
                &format!("a live terminal is already named {:?}", spec.name),
            ));
        }
        let parent = match &spec.parent {
            Parent::None => None,
            Parent::Terminal(id) => match find(&s.terminals, id) {
                // A finished parent is accepted: the child runs, with that parent (hc 0.1.22).
                Some(i) => Some(s.terminals[i].address.clone()),
                None => {
                    return Err(error(
                        "run",
                        Some("not_found"),
                        &format!("no terminal {id:?}"),
                    ));
                }
            },
        };
        let timeout = s.timeout_run.take();
        if timeout == Some(false) {
            return Err(Error::Timeout {
                program: "hc".to_string(),
                after: std::time::Duration::from_secs(10),
            });
        }
        let terminal = Terminal {
            address: format!("t{}", s.terminals.len() + 1),
            name: Some(spec.name.clone()),
            parent,
            state: TermState::Live,
            labels: spec.labels.iter().cloned().collect(),
            agent: spec.argv.first().cloned(),
            cwd: Some(spec.cwd.display().to_string()),
            ..Terminal::default()
        };
        s.terminals.push(terminal.clone());
        if timeout == Some(true) {
            return Err(Error::Timeout {
                program: "hc".to_string(),
                after: std::time::Duration::from_secs(10),
            });
        }
        // hc prints only address, agent, cwd, name, parent, state and turn: no labels.
        Ok(Terminal {
            labels: Default::default(),
            ..terminal
        })
    }

    fn send(&self, to: &str, text: &str) -> Result<SendOutcome, Error> {
        self.record(
            Call::Send {
                to: to.to_string(),
                text: text.to_string(),
            },
            "send",
        )?;
        let s = self.state.borrow();
        if let Some(forced) = &s.forced_send {
            return Ok(forced.clone());
        }
        Ok(match find(&s.terminals, to).map(|i| &s.terminals[i]) {
            None => SendOutcome::NotFound,
            Some(t) if !t.is_live() => SendOutcome::Finished,
            Some(t) if t.status.as_ref().is_some_and(|st| st.state == "blocked") => {
                SendOutcome::Blocked
            }
            Some(_) => SendOutcome::Sent,
        })
    }

    fn kill(&self, id: &str) -> Result<KillOutcome, Error> {
        self.record(Call::Kill(id.to_string()), "kill")?;
        let mut s = self.state.borrow_mut();
        match find(&s.terminals, id) {
            None => Ok(KillOutcome::NotFound),
            Some(i) => {
                finish(&mut s.terminals, i);
                Ok(KillOutcome::Killed)
            }
        }
    }

    fn server_start(&self, env: &[(OsString, OsString)]) -> Result<(), Error> {
        self.record(Call::ServerStart(env.to_vec()), "server_start")
    }
}
