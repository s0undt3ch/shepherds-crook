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
