//! `crook hook session-start`: records the plugin version and that the session has started (D50);
//! in the coordinator, resumes the children that lost their terminals and rescans the results
//! (ADR 0008, 0017, 0019, 0020).

use super::{Input, Reply, Who, prompt, version};
use crate::Ctx;
use crate::cmd::{Io, resume};
use crate::files::roster;
use crate::flock;
use crate::hc::Hc;

#[cfg(test)]
mod tests;

pub fn run(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, input: &Input, who: &Who) -> Reply {
    match &input.session_id {
        Some(session) => {
            // D70: only a start that loads the plugins afresh replaces the record. After `clear` or
            // `compact` the session may still run an older plugin text than the disk has, and
            // overwriting would hide that from the Stop hook's reload.
            let fresh = matches!(input.source.as_deref(), Some("startup" | "resume"));
            if !fresh {
                carry_record(ctx, io, session, who);
            }
            if let Err(e) = version::record_start(ctx, session, fresh) {
                io.warn(format!(
                    "crook hook: could not record the plugin version: {e}"
                ));
            }
        }
        None => io.warn("crook hook: no session_id in the input, so no version record"),
    }
    let flock = match flock::find(ctx.env, &who.flock) {
        Ok(f) => f,
        Err(e) => {
            io.warn(format!("crook hook: {e}"));
            return Reply::default();
        }
    };
    if let Err(e) = mark_started(&flock.folder, who) {
        io.warn(format!(
            "crook hook: could not mark the session started: {e}"
        ));
    }
    if let Err(e) = follow_new_session(&flock.folder, who, input) {
        io.warn(format!(
            "crook hook: could not record the new session id: {e}"
        ));
    }
    let mut reply = Reply::default();
    if who.child.is_some() {
        return reply;
    }
    // The coordinator's resume prints `resumed …` lines on stdout, which here are context for the
    // model; its warnings go to stderr as they are.
    let mut said = Vec::new();
    {
        let mut inner = Io {
            out: &mut said,
            err: &mut *io.err,
        };
        if let Err(e) =
            resume::resume_children(ctx, hc, &mut inner, &flock, &who.address, None, None)
        {
            inner.warn(format!("crook hook: {e}"));
        }
    }
    for line in String::from_utf8_lossy(&said).lines() {
        reply.lines.push(format!("crook: {line}"));
    }
    reply.undelivered = prompt::scan(&flock, io);
    reply
}

/// A running session proves it was launched, so a later resume must use `--resume` (D50).
fn mark_started(folder: &std::path::Path, who: &Who) -> Result<(), crate::Error> {
    let on_disk = roster::load(folder)?;
    match &who.child {
        Some(name) => {
            if on_disk
                .children
                .iter()
                .any(|c| &c.name == name && !c.started)
            {
                roster::set_child_started(folder, name, true)?;
            }
        }
        None => {
            if !on_disk.coordinator.started {
                roster::set_coordinator_started(folder, true)?;
            }
        }
    }
    Ok(())
}

/// `/clear`, or a `/resume` of another conversation from inside the session, starts a new session id
/// in the same terminal (D102). The roster follows it, so `crook resume` continues the conversation
/// the maintainer last saw (D114, D116). Every source but `startup` counts: crook launches with the
/// roster's id, so its own starts never differ, and a stray `claude` run inside the terminal starts
/// as `startup` and must not take the terminal's place.
fn follow_new_session(
    folder: &std::path::Path,
    who: &Who,
    input: &Input,
) -> Result<(), crate::Error> {
    let Some(session) = input.session_id.as_deref() else {
        return Ok(());
    };
    if matches!(input.source.as_deref(), None | Some("startup")) || session.trim().is_empty() {
        return Ok(());
    }
    let on_disk = roster::load(folder)?;
    match &who.child {
        Some(name) => {
            if on_disk
                .children
                .iter()
                .any(|c| &c.name == name && c.session_id != session)
            {
                roster::set_child_session_id(folder, name, session)?;
            }
        }
        None => {
            if on_disk.coordinator.session_id != session {
                roster::set_coordinator_session_id(folder, session)?;
            }
        }
    }
    Ok(())
}

/// `/clear` brings a new session id (D102), so D70's "keep the record" would find none and record
/// the disk's version, hiding a reload the session still needs. The new id takes over the old
/// id's record instead, read from the roster before [`follow_new_session`] moves it. A needless
/// reload is the worst case if the clear did load the new text.
fn carry_record(ctx: &Ctx, io: &mut Io, session: &str, who: &Who) {
    let Ok(flock) = flock::find(ctx.env, &who.flock) else {
        return;
    };
    let Ok(on_disk) = roster::load(&flock.folder) else {
        return;
    };
    let old = match &who.child {
        Some(name) => on_disk
            .children
            .iter()
            .find(|c| &c.name == name)
            .map(|c| c.session_id.clone()),
        None => Some(on_disk.coordinator.session_id.clone()),
    };
    let Some(old) = old.filter(|old| old != session) else {
        return;
    };
    if matches!(version::load(ctx, session), Ok(Some(_))) {
        return;
    }
    // The old skill text went with the clear, so a nudge to invoke the skill again is not owed.
    let carried = match version::load(ctx, &old) {
        Ok(Some(record)) => version::Record {
            reloaded: false,
            ..record
        },
        Ok(None) => return,
        Err(e) => {
            io.warn(format!(
                "crook hook: could not read the previous session's plugin version: {e}"
            ));
            return;
        }
    };
    if let Err(e) = version::save(ctx, session, &carried) {
        io.warn(format!(
            "crook hook: could not carry the plugin version to the new session: {e}"
        ));
    }
}
