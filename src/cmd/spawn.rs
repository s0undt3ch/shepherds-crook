//! `crook spawn <role> <child> <brief>`: write the roster entry, then launch the child under the
//! coordinator (ADR 0007, 0017, 0020, 0023, 0024, 0025; Q9, Q11, Q12, Q14, Q40).

use std::path::{Path, PathBuf};

use super::{
    Io, Setup, find_flock, is_closed, live_named, outcome_unknown, parent_terminal,
    require_context, resolve_slug,
};
use crate::files::roster::{self, Child, State};
use crate::flock::Flock;
use crate::git::{self, Worktree};
use crate::hc::Hc;
use crate::hc::preflight;
use crate::launch::{self, Session};
use crate::{Ctx, Error, names};

pub struct Args<'a> {
    pub role: &'a str,
    pub child: &'a str,
    pub brief: &'a str,
    pub repo: Option<&'a Path>,
    pub model: Option<&'a str>,
    pub flock: Option<&'a str>,
}

pub fn run(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, args: &Args) -> Result<(), Error> {
    require_context(ctx, args.flock)?;
    names::validate_child(args.child)?;
    validate_role(ctx, args.role)?;
    preflight::run(hc, &(ctx.vars)())?;
    let (slug, identity) = resolve_slug(ctx, hc, args.flock)?;

    let flock = find_flock(ctx, &slug)?;
    let on_disk = roster::load(&flock.folder)?;
    if is_closed(&on_disk) {
        return Err(Error::Flock(format!(
            "flock {slug:?} is closed; a closed flock takes no new children"
        )));
    }
    if on_disk.children.iter().any(|c| c.name == args.child) {
        return Err(Error::Flock(format!(
            "flock {slug:?} already has a child named {:?}; pick another name, or `crook resume {}` to bring it back",
            args.child, args.child
        )));
    }
    let brief_file = resolve_brief(&flock, args.brief)?;
    let setup = Setup::load(ctx)?;
    let model = args
        .model
        .filter(|m| !m.is_empty())
        .map_or_else(|| setup.config.default_child_model.clone(), str::to_string);
    let parent = parent_terminal(hc, &slug, identity.as_ref())?;

    let (folder, repo, worktree) = match args.repo {
        Some(path) => {
            let top = git::toplevel(ctx.runner, path)?;
            let tree = git::worktree_add(ctx.runner, &top, &slug, args.child)?;
            (tree.path.clone(), Some(top), Some(tree))
        }
        None => (flock.folder.clone(), None, None),
    };

    let session_id = (ctx.new_id)();
    let entry = Child {
        name: args.child.to_string(),
        role: args.role.to_string(),
        model: model.clone(),
        session_id: session_id.clone(),
        folder: folder.clone(),
        repo: repo.clone(),
        branch: worktree.as_ref().map(|w| w.branch.clone()),
        brief: format!("briefs/{brief_file}"),
        state: State::Active,
        started: false,
    };
    // The entry goes in before the launch (ADR 0007, 0017). A crash before `hc run` leaves it
    // `started: false`, so the next `resume` launches it with `--session-id` (D50).
    if let Err(e) = roster::add_child(&flock.folder, entry.clone()) {
        undo_worktree(ctx, io, repo.as_deref(), worktree.as_ref());
        return Err(e);
    }

    let notes = flock.notes_folder(&setup.config);
    let brief_path = flock.brief_path(&entry.brief);
    let result_path = flock.result_path(args.child);
    let spec = launch::child(&launch::Child {
        common: setup.common(&flock.folder, &notes),
        slug: &slug,
        name: args.child,
        role: args.role,
        model: &model,
        session: Session::New(&session_id),
        cwd: &folder,
        brief: &brief_path,
        result: &result_path,
        parent: &parent,
    });
    let launched = hc.run(&spec).map(|t| (t.address, false)).or_else(|e| {
        // A timeout is not a refusal: if the terminal is live, the child is running.
        if !outcome_unknown(&e) {
            return Err((e, true));
        }
        match live_named(hc, &spec.name) {
            Ok(Some(t)) => {
                io.warn(format!(
                    "hc did not answer the launch of {} in time, but the terminal is live (terminal {}); keeping it",
                    spec.name, t.address
                ));
                Ok((t.address, true))
            }
            Ok(None) => Err((e, true)),
            // Nothing can be undone safely when even the check fails.
            Err(check) => {
                io.warn(format!(
                    "could not tell whether {} started ({check}); the roster entry and worktree are kept, so `crook resume {}` can finish it",
                    spec.name, args.child
                ));
                Err((e, false))
            }
        }
    });
    match launched {
        Ok((address, _)) => {
            roster::set_child_started(&flock.folder, args.child, true)?;
            io.say(format!(
                "spawned {} (terminal {address}, session {session_id})",
                spec.name
            ));
            Ok(())
        }
        Err((e, undo)) => {
            if undo {
                // So the coordinator can retry with the same name (Q11, Q40).
                if let Err(remove) = roster::remove_child(&flock.folder, args.child) {
                    io.warn(format!(
                        "could not remove {:?} from the roster after the failed launch: {remove}",
                        args.child
                    ));
                }
                undo_worktree(ctx, io, repo.as_deref(), worktree.as_ref());
            }
            Err(e)
        }
    }
}

/// Removes the worktree this spawn just made, if it is still clean, and the empty branch this call made (Q40, D51).
fn undo_worktree(ctx: &Ctx, io: &mut Io, repo: Option<&Path>, worktree: Option<&Worktree>) {
    let (Some(repo), Some(worktree)) = (repo, worktree) else {
        return;
    };
    match git::remove_worktree(ctx.runner, repo, worktree) {
        Ok(git::Removal::Kept(reasons)) => io.warn(format!(
            "kept the worktree {}: {}",
            worktree.path.display(),
            reasons.join("; ")
        )),
        Err(e) => io.warn(format!(
            "could not remove the worktree {}: {e}",
            worktree.path.display()
        )),
        Ok(git::Removal::Removed) => {
            // D51: this call made the branch, so it has no commits, and `-d` refuses anything else.
            let _ = git::delete_merged_branch(ctx.runner, repo, &worktree.branch);
        }
        Ok(git::Removal::Gone) => {}
    }
}

/// The brief's file name, once it is known to be an existing file directly under the flock's
/// `briefs/` (Q12). A relative path is taken from the flock folder, as the help says. Links are
/// followed first, so a link out of `briefs/` does not pass.
fn resolve_brief(flock: &Flock, brief: &str) -> Result<String, Error> {
    let given = Path::new(brief);
    let path: PathBuf = if given.is_absolute() {
        given.to_path_buf()
    } else {
        flock.folder.join(given)
    };
    let real = path
        .canonicalize()
        .map_err(|_| Error::Flock(format!("the brief {} does not exist", path.display())))?;
    let briefs = flock
        .briefs_dir()
        .canonicalize()
        .map_err(|source| Error::File {
            path: flock.briefs_dir(),
            source,
        })?;
    if !real.is_file() || real.parent() != Some(briefs.as_path()) {
        return Err(Error::Flock(format!(
            "the brief must be a file directly under {}, not {}",
            flock.briefs_dir().display(),
            path.display()
        )));
    }
    real.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| Error::Flock(format!("the brief name of {} is not text", path.display())))
}

#[cfg(test)]
mod tests;

/// A role is a plugin agent file other than the coordinator's. Looked up in this install's plugin
/// folder, so a typo is an error here and not a Claude that starts with no agent.
fn validate_role(ctx: &Ctx, role: &str) -> Result<(), Error> {
    let plugin = crate::install::find_own(ctx)?.plugin_dir();
    let mut roles: Vec<String> = std::fs::read_dir(plugin.join("agents"))
        .map(|dir| {
            dir.filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok()?;
                    name.strip_suffix(".md").map(str::to_string)
                })
                .filter(|r| r != "coordinator")
                .collect()
        })
        .unwrap_or_default();
    roles.sort();
    if roles.iter().any(|r| r == role) {
        return Ok(());
    }
    Err(Error::Flock(format!(
        "unknown role {role:?}; roles with an agent file in {}: {}",
        plugin.join("agents").display(),
        if roles.is_empty() {
            "none".to_string()
        } else {
            roles.join(", ")
        }
    )))
}
