//! `crook close <slug>`: end the children, mark the roster done, retire the worktrees, print the
//! warnings, and only then end the coordinator (ADR 0018, 0020, 0023; Q6, Q44, D33).
//!
//! The coordinator usually runs this from a process inside its own terminal, so killing it any
//! earlier could kill `close` before the roster is marked or the warnings are printed.

use super::{Io, find_flock, is_closed, live_terminals, refuse_child_terminal, retire_worktree};
use crate::files::roster;
use crate::hc::Hc;
use crate::hc::preflight;
use crate::{Ctx, Error, names};

pub fn run(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, slug: &str) -> Result<(), Error> {
    let flock = find_flock(ctx, slug)?;
    let before = roster::load(&flock.folder)?;
    preflight::run(hc, &(ctx.vars)())?;
    refuse_child_terminal(ctx, hc)?;

    // An already closed flock goes through the same steps: a terminal that survived the first close
    // (a failed kill) is ended now, and the worktrees kept earlier are tried again (Q44).
    let closing = !is_closed(&before);
    let coordinator_name = names::hc_coordinator(slug);
    let (coordinators, children): (Vec<_>, Vec<_>) = live_terminals(hc, slug)?
        .into_iter()
        .partition(|t| t.name.as_deref() == Some(coordinator_name.as_str()));

    // 1. The children, so no worktree is in use.
    let mut still_live = Vec::new();
    for terminal in &children {
        let name = terminal.name.clone().unwrap_or_default();
        if let Err(e) = hc.kill(&terminal.address) {
            io.warn(format!("could not end {name} ({}): {e}", terminal.address));
            still_live.push(name);
        }
    }
    // 2. Closed, in one write.
    if closing {
        roster::mark_all_done(&flock.folder)?;
    }
    // 3 and 4. The worktrees, with a warning for each one kept. One whose child is still running
    // stays untouched.
    for child in &roster::load(&flock.folder)?.children {
        if still_live.contains(&names::hc_child(slug, &child.name)) {
            if child.repo.is_some() {
                io.warn(format!(
                    "kept worktree {}: its terminal is still running",
                    child.folder.display()
                ));
            }
            continue;
        }
        retire_worktree(ctx, io, child);
    }
    if closing {
        io.say(format!("closed flock {slug}"));
    }
    // 5. The coordinator, last. If this fails the flock is already closed, and running `close`
    // again ends it.
    for terminal in &coordinators {
        hc.kill(&terminal.address)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
