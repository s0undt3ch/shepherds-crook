//! `SessionStart`: records the session and injects the project card.

use std::process::{Command, Stdio};

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::Connection;

use crate::gate::{Identity, Role};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew};

/// The `additionalContext` text; errors are for the caller to word.
pub fn handle(
    env: Env,
    id: &Identity,
    session_id: &str,
    source: &str,
    ppid: u32,
) -> Result<String, Error> {
    let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
    let refused = store.write(Touch::CHILDREN, |tx| {
        record(tx, id, session_id, source, ppid)
    })?;
    if let Some(line) = refused {
        return Ok(line);
    }
    match store.read(|c| crew::status(c, id, false)) {
        Ok(v) => Ok(v.card),
        // A stopped child still gets a defined answer.
        Err(Error::Refused(m)) => Ok(m),
        Err(e) => Err(e),
    }
}

/// Whether a process with this pid exists. `EPERM` means it does, owned by
/// someone else. Pid 0 would name our own process group, so it is not one.
fn alive(pid: u32) -> bool {
    match i32::try_from(pid) {
        Ok(raw) if raw > 0 => !matches!(kill(Pid::from_raw(raw), None), Err(Errno::ESRCH)),
        _ => false,
    }
}

/// `ps` by absolute path: one found on `PATH` could be anything.
const PS: [&str; 2] = ["/bin/ps", "/usr/bin/ps"];

/// Whether `pid` is a running claude process, by its command name from `ps`.
/// A pid the child no longer holds may have been reused by anything. When `ps`
/// cannot run, a live pid is taken for the child: refusing is the safe side.
fn is_claude(pid: u32) -> bool {
    if !alive(pid) {
        return false;
    }
    for ps in PS {
        let out = Command::new(ps)
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if let Ok(o) = out {
            return o.status.success() && claude_name(String::from_utf8_lossy(&o.stdout).trim());
        }
    }
    true
}

/// The command's basename is `claude`, alone or followed by a character that
/// is not a letter or digit (`claude-code`, not `claudette`).
fn claude_name(comm: &str) -> bool {
    let name = comm.rsplit('/').next().unwrap_or_default();
    name.strip_prefix("claude")
        .is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()))
}

/// Records the session. `Some` is the context to answer with instead, when a
/// session claims a child whose claude process is another one, still running:
/// taking its row would lift a park.
fn record(
    c: &Connection,
    id: &Identity,
    session_id: &str,
    source: &str,
    ppid: u32,
) -> Result<Option<String>, Error> {
    match (id.role, id.child.as_deref()) {
        (Role::Child, Some(child)) if source == "fork" => {
            // `/branch` keeps the claude process, so the fork is the child.
            if crew::claude_pid(c, child)? == Some(ppid) {
                crew::adopt(c, child, session_id)?;
            } else {
                crew::add_fork(c, child, session_id)?;
            }
        }
        (Role::Child, Some(child)) => {
            let (same, current) = crew::is_current(c, child, session_id)?;
            // Another live claude holds the row: the caller is not the child.
            let held = crew::claude_pid(c, child)?.is_some_and(|p| p != ppid && is_claude(p));
            if held && !same {
                return Ok(Some(format!(
                    "crook: this session is not the child; the child is session {current}"
                )));
            }
            // With the child's own id, it only gets the card: its pid stays.
            if !held {
                crew::record_session(c, child, session_id, ppid)?;
            }
        }
        // A fork of the orchestrator must not take over its row.
        (Role::Orchestrator, _) if source != "fork" => {
            crew::record_orchestrator(c, session_id, ppid)?;
        }
        _ => {}
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::claude_name;

    #[test]
    fn claude_name_is_exact() {
        for yes in [
            "claude",
            "/usr/local/bin/claude",
            "claude-code",
            "claude.exe",
        ] {
            assert!(claude_name(yes), "{yes}");
        }
        for no in ["claudette", "notclaude", "zsh", "", "claude2"] {
            assert!(!claude_name(no), "{no}");
        }
    }
}
