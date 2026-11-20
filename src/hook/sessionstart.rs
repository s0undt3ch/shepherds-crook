//! `SessionStart`: records the session and injects the project card.

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::Connection;

use crate::crew;
use crate::gate::{Identity, Role};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error};

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
            if let Some(pid) = crew::claude_pid(c, child)?
                && pid != ppid
                && alive(pid)
            {
                let (_, current) = crew::is_current(c, child, session_id)?;
                return Ok(Some(format!(
                    "crook: this session is not the child; the child is session {current}"
                )));
            }
            crew::record_session(c, child, session_id, ppid)?;
        }
        // A fork of the orchestrator must not take over its row.
        (Role::Orchestrator, _) if source != "fork" => {
            crew::record_orchestrator(c, session_id, ppid)?;
        }
        _ => {}
    }
    Ok(None)
}
