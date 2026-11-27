//! `crook resume [<child>] [--model <model>]`: start children again from the roster, under their
//! original hc names (ADR 0017, 0020, 0025; Q9, Q10).

use super::{
    Io, Setup, end_terminal, find_flock, is_closed, live_terminals, parent_terminal,
    require_context, resolve_slug,
};
use crate::files::roster::{self, Child, State};
use crate::flock::Flock;
use crate::hc::preflight;
use crate::hc::{Hc, Terminal};
use crate::launch::{self, Session};
use crate::{Ctx, Error, names};

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    child: Option<&str>,
    model: Option<&str>,
    flag: Option<&str>,
) -> Result<(), Error> {
    if model.is_some() && child.is_none() {
        return Err(Error::Flock(
            "--model needs a child: `crook resume <child> --model <model>`".to_string(),
        ));
    }
    if let Some(child) = child {
        names::validate_child(child)?;
    }
    require_context(ctx, flag)?;
    preflight::run(hc, &(ctx.vars)())?;
    let (slug, identity) = resolve_slug(ctx, hc, flag)?;
    let flock = find_flock(ctx, &slug)?;
    if is_closed(&roster::load(&flock.folder)?) {
        return match child {
            Some(_) => Err(Error::Flock(format!(
                "flock {slug:?} is closed; its children are not resumed"
            ))),
            None => Ok(()),
        };
    }
    let parent = parent_terminal(hc, &slug, identity.as_ref())?;
    let resumed = resume_children(ctx, hc, io, &flock, &parent, child, model)?;
    if resumed == 0 && child.is_none() {
        io.say(format!("flock {slug}: no children to resume"));
    }
    Ok(())
}

/// The library function the SessionStart hook calls directly (ADR 0020): no preflight, since a hook
/// runs inside a live core, and the parent is given. Returns how many children it started. A closed
/// flock is skipped. With `only`, a failure is an error; without it the other children are still
/// tried, and the failures are warned about and counted in the error.
pub fn resume_children(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    flock: &Flock,
    parent: &str,
    only: Option<&str>,
    model: Option<&str>,
) -> Result<usize, Error> {
    let slug = flock.slug.as_str();
    let on_disk = roster::load(&flock.folder)?;
    if is_closed(&on_disk) {
        return match only {
            Some(_) => Err(Error::Flock(format!(
                "flock {slug:?} is closed; its children are not resumed"
            ))),
            None => Ok(0),
        };
    }
    let targets: Vec<&Child> = match only {
        Some(name) => {
            let entry = on_disk.children.iter().find(|c| c.name == name);
            let entry = entry.ok_or_else(|| {
                Error::Flock(format!("flock {slug:?} has no child named {name:?}"))
            })?;
            if entry.state == State::Done {
                return Err(Error::Flock(format!(
                    "child {name:?} is done; a stopped child is not resumed"
                )));
            }
            vec![entry]
        }
        None => on_disk
            .children
            .iter()
            .filter(|c| c.state == State::Active)
            .collect(),
    };
    if targets.is_empty() {
        return Ok(0);
    }
    let setup = Setup::load(ctx)?;
    let live = live_terminals(hc, slug)?;

    let mut resumed = 0;
    let mut failed = 0;
    for child in targets {
        let hc_name = names::hc_child(slug, &child.name);
        let named = live
            .iter()
            .find(|t| t.name.as_deref() == Some(hc_name.as_str()));
        let outcome = resume_one(hc, &setup, flock, child, named, parent, model);
        match outcome {
            Ok(Some(terminal)) => {
                resumed += 1;
                io.say(format!("resumed {hc_name} (terminal {})", terminal.address));
            }
            Ok(None) => {
                if only.is_some() {
                    io.say(format!("{hc_name} is already running"));
                }
            }
            Err(e) if only.is_some() => return Err(e),
            Err(e) => {
                failed += 1;
                io.warn(format!("could not resume {hc_name}: {e}"));
            }
        }
    }
    if failed > 0 {
        return Err(Error::Flock(format!(
            "{failed} of the children could not be resumed"
        )));
    }
    Ok(resumed)
}

#[allow(clippy::too_many_arguments)]
fn resume_one(
    hc: &dyn Hc,
    setup: &Setup,
    flock: &Flock,
    child: &Child,
    named: Option<&Terminal>,
    parent: &str,
    model: Option<&str>,
) -> Result<Option<Terminal>, Error> {
    let mut chosen = child.model.clone();
    // A live terminal means the child has started, whatever the roster says (D50).
    let mut started = child.started;
    if named.is_some() && !started {
        roster::set_child_started(&flock.folder, &child.name, true)?;
        started = true;
    }
    // A bare shell under the child's name is what a lost terminal restores as (D52): not the child,
    // and in its way.
    let running = named.filter(|t| !t.is_shell());
    if let Some(shell) = named.filter(|t| t.is_shell()) {
        end_terminal(hc, &shell.address)?;
    }
    match (running, model) {
        (Some(_), None) => return Ok(None),
        (None, None) => {}
        (running, Some(model)) => {
            if let Some(live) = running {
                // Only an idle child may be moved: killing a working one throws its turn away (Q10).
                let state = hc
                    .get(&live.address)?
                    .and_then(|t| t.status)
                    .map(|s| s.state);
                if state.as_deref() == Some("working") {
                    return Err(Error::Flock(format!(
                        "child {:?} is working; wait until it is idle before changing its model",
                        child.name
                    )));
                }
            }
            // The roster first, so the new model survives a failed launch (0025).
            roster::set_child_model(&flock.folder, &child.name, model)?;
            chosen = model.to_string();
            if let Some(live) = running {
                // Ended, not marked done: it comes straight back (Q10).
                end_terminal(hc, &live.address)?;
            }
        }
    }

    let notes = flock.notes_folder(&setup.config);
    let brief = flock.brief_path(&child.brief);
    let result = flock.result_path(&child.name);
    let spec = launch::child(&launch::Child {
        common: setup.common(&flock.folder, &notes),
        slug: &flock.slug,
        name: &child.name,
        role: &child.role,
        model: &chosen,
        session: if started {
            Session::Resume(&child.session_id)
        } else {
            Session::New(&child.session_id)
        },
        cwd: &child.folder,
        brief: &brief,
        result: &result,
        parent,
    });
    let terminal = hc.run(&spec)?;
    if !started {
        roster::set_child_started(&flock.folder, &child.name, true)?;
    }
    Ok(Some(terminal))
}

#[cfg(test)]
mod tests;
