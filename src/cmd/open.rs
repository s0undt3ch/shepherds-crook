//! `crook open <slug>`: start the flock's coordinator terminal (ADR 0015, 0018, 0020, 0021; Q7, Q8).

use super::{
    Io, Setup, coordinator_terminal, end_terminal, find_flock, is_closed, live_named,
    outcome_unknown,
};
use crate::files::roster;
use crate::hc::Hc;
use crate::hc::preflight;
use crate::launch::{self, Session};
use crate::{Ctx, Error, herdr_app, paths};

/// `task` is the first task from `crook new --task`; it only matters on a first start (Q1).
pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    slug: &str,
    task: Option<&str>,
) -> Result<(), Error> {
    let flock = find_flock(ctx, slug)?;
    let roster = roster::load(&flock.folder)?;
    if is_closed(&roster) {
        return Err(Error::Flock(format!(
            "flock {slug:?} is closed; a closed flock is not reopened (start a new one with `crook new`)"
        )));
    }
    let setup = Setup::load(ctx)?;
    let crook = paths::crook_call_path(ctx.env, setup.crook_bin.as_deref())?;
    preflight::run(hc, &(ctx.vars)())?;

    let mut found = coordinator_terminal(hc, slug)?;
    if let Some(shell) = found.take_if(|t| t.is_shell()) {
        // The core restored the terminal as a bare shell: it holds the name, and is no coordinator (D52).
        io.warn(format!(
            "terminal {} is named {slug} but runs a shell, not the coordinator; replacing it",
            shell.address
        ));
        end_terminal(hc, &shell.address)?;
    }
    if let Some(live) = found {
        // Never a second coordinator (Q7). A live one has started, whatever the roster says (D50).
        if !roster.coordinator.started {
            roster::set_coordinator_started(&flock.folder, true)?;
        }
        let renamed = rename(ctx, io, slug);
        io.say(format!(
            "flock {slug} is already open (terminal {}){}",
            live.address,
            if renamed {
                "; renamed its space and tab"
            } else {
                ""
            }
        ));
        return Ok(());
    }

    let notes = flock.notes_folder(&setup.config);
    let sid = roster.coordinator.session_id.as_str();
    // `--session-id` until a launch has succeeded, `--resume` after (Q8).
    let session = if roster.coordinator.started {
        Session::Resume(sid)
    } else {
        Session::New(sid)
    };
    let spec = launch::coordinator(&launch::Coordinator {
        common: setup.common(&flock.folder, &notes),
        slug,
        cwd: &roster.coordinator.folder,
        session,
        model: setup.config.coordinator_model.as_deref(),
        task,
        crook: &crook,
    });
    let address = match hc.run(&spec) {
        Ok(terminal) => terminal.address,
        // A timeout is not a refusal: a live terminal under this name means it started.
        Err(e) if outcome_unknown(&e) => match live_named(hc, slug) {
            Ok(Some(live)) => {
                io.warn(format!(
                    "hc did not answer the launch in time, but the coordinator is live (terminal {})",
                    live.address
                ));
                live.address
            }
            _ => return Err(e),
        },
        Err(e) => return Err(e),
    };
    roster::set_coordinator_started(&flock.folder, true)?;
    rename(ctx, io, slug);
    io.say(format!("opened flock {slug} (terminal {address})"));
    Ok(())
}

/// Whether the space and tab were renamed; a warning says why not.
fn rename(ctx: &Ctx, io: &mut Io, slug: &str) -> bool {
    match herdr_app::rename_coordinator(ctx.runner, slug) {
        Some(warning) => {
            io.warn(warning);
            false
        }
        None => true,
    }
}

#[cfg(test)]
mod tests;
