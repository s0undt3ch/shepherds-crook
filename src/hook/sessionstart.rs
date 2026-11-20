//! `SessionStart`: records the session and injects the project card.

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
    store.write(Touch::CHILDREN, |tx| {
        record(tx, id, session_id, source, ppid)
    })?;
    match store.read(|c| crew::status(c, id, false)) {
        Ok(v) => Ok(v.card),
        // A stopped child still gets a defined answer.
        Err(Error::Refused(m)) => Ok(m),
        Err(e) => Err(e),
    }
}

fn record(
    c: &Connection,
    id: &Identity,
    session_id: &str,
    source: &str,
    ppid: u32,
) -> Result<(), Error> {
    match (id.role, id.child.as_deref()) {
        (Role::Child, Some(child)) if source == "fork" => {
            // `/branch` keeps the claude process, so the fork is the child.
            if crew::claude_pid(c, child)? == Some(ppid) {
                crew::adopt(c, child, session_id)
            } else {
                crew::add_fork(c, child, session_id)
            }
        }
        (Role::Child, Some(child)) => crew::record_session(c, child, session_id, ppid),
        // A fork of the orchestrator must not take over its row.
        (Role::Orchestrator, _) if source != "fork" => {
            crew::record_orchestrator(c, session_id, ppid)
        }
        _ => Ok(()),
    }
}
