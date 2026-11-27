//! `crook stop <child>`: mark it done first, then end its terminal, then retire its worktree
//! (ADR 0020, 0023; Q6, Q44).

use super::{Io, find_flock, live_terminals, require_context, resolve_slug, retire_worktree};
use crate::files::roster::{self, State};
use crate::hc::Hc;
use crate::hc::preflight;
use crate::{Ctx, Error, names};

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    child: &str,
    flag: Option<&str>,
) -> Result<(), Error> {
    require_context(ctx, flag)?;
    names::validate_child(child)?;
    preflight::run(hc, &(ctx.vars)())?;
    let (slug, _) = resolve_slug(ctx, hc, flag)?;
    let flock = find_flock(ctx, &slug)?;
    let on_disk = roster::load(&flock.folder)?;
    let Some(entry) = on_disk.children.iter().find(|c| c.name == child) else {
        let known: Vec<&str> = on_disk.children.iter().map(|c| c.name.as_str()).collect();
        return Err(Error::Flock(format!(
            "flock {slug:?} has no child named {child:?} (children: {})",
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        )));
    };
    let was_done = entry.state == State::Done;

    // The terminal first, whatever the roster says: a kill that fails leaves the child active, so
    // nothing claims it stopped while it runs, and a retry can still end it.
    let hc_name = names::hc_child(&slug, child);
    for terminal in live_terminals(hc, &slug)? {
        if terminal.name.as_deref() == Some(hc_name.as_str())
            && let Err(e) = hc.kill(&terminal.address)
        {
            if was_done {
                io.warn(format!(
                    "kept worktree {}: its terminal {} could not be ended",
                    entry.folder.display(),
                    terminal.address
                ));
            }
            return Err(e);
        }
    }
    if !was_done {
        roster::set_child_state(&flock.folder, child, State::Done)?;
    }
    // Only now is the worktree out of use. It is tried again on a child that was already done: it
    // may be pushed by now (Q44).
    retire_worktree(ctx, io, entry);
    if !was_done {
        io.say(format!("stopped {hc_name}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
