//! `crook stop <child>`: archives a child, stops its session and removes its worktrees.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::launch::child::{
    Launcher, archive_and_stop, describe_sessions, sessions_inside, verify_worktree,
};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew};

use super::address;

/// Stops `arg` (`<child>` or `<slug>/<child>`). It refuses, changing nothing,
/// when a recorded worktree fails `verify_worktree` or has uncommitted changes.
/// Otherwise it archives the child and stops its session, and removes each
/// worktree by its recorded path and forgets its row, but only once the session
/// is confirmed gone and `claude agents` shows no session inside any of them:
/// otherwise the worktrees stay and `stop` says why, to be run again. Another
/// session inside one (a fork, the user's own `claude`) refuses the whole stop.
pub fn run(env: Env, arg: &str, run: &dyn Launcher) -> Result<String, Error> {
    let (slug, child) = address::resolve(env, arg)?;
    let mut store = Store::open(env, &slug, Mode::ReadWrite)?;
    let (repo, rows, session_id) = store.read(|c| {
        Ok((
            crew::child_repo(c, &child)?,
            crew::worktrees(c, Some(&child))?,
            crew::resumable(c, &child)?.session_id,
        ))
    })?;

    // Every refusal comes before the first change.
    let mut checked = Vec::new();
    for r in &rows {
        let real = verify_worktree(run, &repo, &slug, &child, Path::new(&r.path))?;
        checked.push((r.path.as_str(), real));
    }
    let mut dirty = Vec::new();
    for (path, real) in &checked {
        if let Some(real) = real
            && run.worktree_dirty(real)?
        {
            dirty.push(*path);
        }
    }
    if !dirty.is_empty() {
        return Err(Error::Refused(format!(
            "child {child} has uncommitted changes in: {}; nothing was changed",
            dirty.join(", ")
        )));
    }
    let dirs: Vec<&Path> = checked
        .iter()
        .flat_map(|(path, real)| [Some(Path::new(*path)), real.as_deref()])
        .flatten()
        .collect();
    // When `claude agents` cannot say, the stop below archives only.
    if let Ok(agents) = run.agents() {
        let others: Vec<_> = sessions_inside(&agents, &dirs)
            .into_iter()
            .filter(|a| a.session_id != session_id)
            .collect();
        if !others.is_empty() {
            return Err(Error::Refused(format!(
                "other sessions run in {child}'s worktrees: {}; nothing was changed: \
                 stop them first",
                describe_sessions(&others)
            )));
        }
    }

    let stopped = archive_and_stop(&mut store, &child, run)?;
    if !stopped.session_gone() {
        return Ok(format!(
            "archived {child}, but its session may still run ({}); its {} worktrees were kept: \
             stop the session, then run crook stop {child} again",
            stopped.describe(),
            rows.len()
        ));
    }
    // A stopped session may take a moment to exit, and any other may have
    // started meanwhile: nothing is removed while one shows inside.
    let until = Instant::now() + run.exit_wait();
    let still = loop {
        let still = match run.agents() {
            Ok(agents) => describe_sessions(&sessions_inside(&agents, &dirs)),
            Err(e) => format!("claude agents cannot say: {e}"),
        };
        if still.is_empty() || Instant::now() >= until {
            break still;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    if !still.is_empty() {
        return Ok(format!(
            "archived {child} ({}), but its {} worktrees were kept while a session may run \
             in them: {still}; run crook stop {child} again once it has exited",
            stopped.describe(),
            rows.len()
        ));
    }
    let mut removed = 0;
    let mut failed = Vec::new();
    let mut skipped = Vec::new();
    for (path, real) in &checked {
        // Checked again: the disk may have changed while the session was stopping.
        let res = match real {
            Some(_) => match verify_worktree(run, &repo, &slug, &child, Path::new(path)) {
                Ok(Some(real)) => run.worktree_remove(Path::new(&repo), &real),
                // It went away meanwhile: nothing to remove.
                Ok(None) => Ok(()),
                Err(e) => {
                    skipped.push(format!("{path}: {e}"));
                    continue;
                }
            },
            // A worktree already gone from disk has nothing to remove.
            None => Ok(()),
        };
        match res {
            Ok(()) => {
                store.write(Touch::CHILDREN, |tx| crew::remove_worktree_row(tx, path))?;
                removed += 1;
            }
            Err(e) => failed.push(format!("{path}: {e}")),
        }
    }
    if !failed.is_empty() || !skipped.is_empty() {
        failed.extend(
            skipped
                .iter()
                .map(|s| format!("skipped, no longer verifies: {s}")),
        );
        return Err(Error::Refused(format!(
            "child {child} is archived ({}), but not every worktree was removed: {}",
            stopped.describe(),
            failed.join("; ")
        )));
    }
    Ok(format!(
        "stopped {child}: {}; removed {removed} worktrees",
        stopped.describe()
    ))
}

#[cfg(test)]
mod tests;
