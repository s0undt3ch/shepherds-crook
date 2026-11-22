//! `crook wipe <slug>`: removes a finished project's worktrees, state and what crook made in its home.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::launch::child::{Launcher, describe_sessions, sessions_inside, verify_worktree};
use crate::store::{Mode, Store};
use crate::{Env, Error, config, crew, paths, project};

/// Wipes project `slug` and returns what it has to say: a summary, any
/// worktree in a record repo that no row knows (never removed), warnings, and
/// the `crook/<slug>/*` branches left behind (never deleted). In the home it
/// removes only what crook made, and lists the rest (`remove_home`).
///
/// It refuses, changing nothing, while a session of the project is live (the
/// orchestrator, any child archived or not, a fork or an id a child held, or
/// any session whose cwd is in a recorded worktree or the home), when
/// `claude agents` cannot say, when a recorded worktree fails `verify_worktree`
/// or has uncommitted changes, and for `--with-diary` with a `diary_root`.
/// Removal then goes worktrees, home, state dir, so a crash part-way leaves a
/// record that still says what is left.
pub fn run(
    env: Env,
    slug: &str,
    with_diary: bool,
    run: &dyn Launcher,
) -> Result<Vec<String>, Error> {
    paths::valid_slug(slug)?;
    let cfg = config::load(env)?;
    let home = cfg.project_home(env, slug)?;
    let diary = cfg.diary(env, slug)?;
    if cfg.diary_root.is_some() {
        if with_diary {
            return Err(Error::Refused(format!(
                "--with-diary is refused for a diary under diary_root: {} is yours to remove by hand",
                diary.display()
            )));
        }
        if overlaps(&diary, &home) {
            return Err(Error::Refused(format!(
                "the diary {} overlaps the project home {}; remove what you want gone by hand",
                diary.display(),
                home.display()
            )));
        }
    }
    let state = paths::project_dir(env, slug)?;
    for dir in [&home, &state] {
        if std::fs::symlink_metadata(dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Refused(format!(
                "{} is a symlink; nothing was changed",
                dir.display()
            )));
        }
    }

    let mut store = Store::open(env, slug, Mode::ReadWrite)?;
    let (record, orchestrator, children, rows) = store.read(|c| {
        // An archived child may still run: its stop may not have confirmed it gone.
        let mut st = c.prepare(
            "SELECT child, session_id, 1 FROM children
             UNION SELECT child, session_id, 0 FROM forks
             UNION SELECT child, session_id, 0 FROM child_sessions
             ORDER BY 1, 2, 3 DESC",
        )?;
        let mut children = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? == 1,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        // The current id is also a held one: keep its `children` row only.
        children.dedup_by(|b, a| a.0 == b.0 && a.1 == b.1);
        Ok((
            project::get(c)?,
            crew::orchestrator(c)?.0,
            children,
            crew::worktrees(c, None)?,
        ))
    })?;

    // Every refusal comes before the first change.
    let agents = run.agents().map_err(|e| {
        Error::Refused(format!(
            "cannot confirm that no session of {slug} is live ({e}); nothing was changed"
        ))
    })?;
    let mut live = Vec::new();
    if let Some(sid) = &orchestrator
        && agents.iter().any(|a| &a.session_id == sid)
    {
        live.push(format!("the orchestrator (session {sid})"));
    }
    for (child, sid, current) in &children {
        if agents.iter().any(|a| &a.session_id == sid) {
            live.push(if *current {
                format!("child {child} (session {sid}): run crook stop {slug}/{child}")
            } else {
                format!("child {child}'s fork or earlier session {sid}: stop that session itself")
            });
        }
    }
    let mut dirs: Vec<&Path> = rows.iter().map(|r| Path::new(&r.path)).collect();
    dirs.push(&home);
    let inside = sessions_inside(&agents, &dirs);
    if !inside.is_empty() {
        live.push(describe_sessions(&inside));
    }
    if !live.is_empty() {
        return Err(Error::Refused(format!(
            "project {slug} has live sessions: {}; nothing was changed",
            live.join("; ")
        )));
    }

    let mut checked = Vec::new();
    for r in &rows {
        let real = verify_worktree(run, &r.repo, slug, &r.child, Path::new(&r.path))?;
        checked.push((r, real));
    }
    let mut dirty = Vec::new();
    for (r, real) in &checked {
        if let Some(real) = real
            && run.worktree_dirty(real)?
        {
            dirty.push(r.path.as_str());
        }
    }
    if !dirty.is_empty() {
        return Err(Error::Refused(format!(
            "uncommitted changes in: {}; nothing was changed",
            dirty.join(", ")
        )));
    }

    // Worktrees first, each checked again just before its removal.
    let mut removed = 0;
    let mut failed = Vec::new();
    for (r, real) in &checked {
        let res = match real {
            Some(_) => match verify_worktree(run, &r.repo, slug, &r.child, Path::new(&r.path)) {
                Ok(Some(real)) => run.worktree_remove(Path::new(&r.repo), &real),
                // It went away meanwhile: nothing to remove.
                Ok(None) => Ok(()),
                Err(e) => {
                    failed.push(format!("skipped, no longer verifies: {}: {e}", r.path));
                    continue;
                }
            },
            // Already gone from disk.
            None => Ok(()),
        };
        match res {
            Ok(()) => {
                store.write(crate::store::Touch::CHILDREN, |tx| {
                    crew::remove_worktree_row(tx, &r.path)
                })?;
                removed += 1;
            }
            Err(e) => failed.push(format!("{}: {e}", r.path)),
        }
    }
    if !failed.is_empty() {
        return Err(Error::Refused(format!(
            "not every worktree of {slug} was removed: {}; the home and state were kept, \
             run crook wipe {slug} again",
            failed.join("; ")
        )));
    }

    let mut lines = Vec::new();
    let mut repos: Vec<&str> = record.repos.iter().map(String::as_str).collect();
    repos.extend(rows.iter().map(|r| r.repo.as_str()));
    let repos: BTreeSet<&str> = repos.into_iter().collect();
    let known: BTreeSet<PathBuf> = checked
        .iter()
        .flat_map(|(r, real)| [Some(PathBuf::from(&r.path)), real.clone()])
        .flatten()
        .collect();
    for repo in &repos {
        if let Err(e) = run.worktree_prune(Path::new(repo)) {
            lines.push(format!("warning: git worktree prune in {repo}: {e}"));
        }
        match run.worktree_list(Path::new(repo)) {
            Ok(listed) => {
                for l in unrecorded(repo, slug, &listed, &known) {
                    lines.push(format!(
                        "not recorded, not removed: {} (in {repo})",
                        l.display()
                    ));
                }
            }
            Err(e) => lines.push(format!("warning: git worktree list in {repo}: {e}")),
        }
    }

    let (home_gone, left) = remove_home(&home, slug, !with_diary)?;
    drop(store);
    std::fs::remove_dir_all(&state)?;

    let kept = if with_diary {
        String::new()
    } else {
        format!("; kept the diary {}", diary.display())
    };
    let home_part = if home_gone {
        format!(", the home {}", home.display())
    } else {
        String::new()
    };
    lines.insert(
        0,
        format!("wiped {slug}: removed {removed} worktrees{home_part} and the state{kept}"),
    );
    lines.splice(1..1, left);
    let prefix = format!("crook/{slug}/");
    for repo in &repos {
        match run.branches(Path::new(repo), &prefix) {
            Ok(branches) => lines.extend(
                branches
                    .into_iter()
                    .map(|b| format!("branch kept: {b} (in {repo})")),
            ),
            Err(e) => lines.push(format!("warning: listing branches in {repo}: {e}")),
        }
    }
    Ok(lines)
}

fn resolved(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Whether either path is the other or inside it.
fn overlaps(a: &Path, b: &Path) -> bool {
    let (ra, rb) = (resolved(a), resolved(b));
    a.starts_with(b) || b.starts_with(a) || ra.starts_with(&rb) || rb.starts_with(&ra)
}

/// The listed worktrees directly under `<repo>/.worktrees/<slug>/` that no row knows.
fn unrecorded(
    repo: &str,
    slug: &str,
    listed: &[PathBuf],
    known: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    let prefixes = [
        Path::new(repo).join(".worktrees").join(slug),
        resolved(Path::new(repo)).join(".worktrees").join(slug),
    ];
    listed
        .iter()
        .filter(|l| {
            let under = |p: &Path| p.parent().is_some_and(|d| prefixes.iter().any(|x| x == d));
            under(l) || under(&resolved(l))
        })
        .filter(|l| !known.contains(*l) && !known.contains(&resolved(l)))
        .cloned()
        .collect()
}

/// Removes what crook made in `home`, and nothing else: `diary/` unless
/// `keep_diary`, then the marker and `home` itself once nothing else is in it.
/// A home without crook's marker for `slug` is left whole. Returns whether
/// `home` is gone, and a line for each thing left that crook did not make. A
/// symlinked diary is unlinked, never followed.
fn remove_home(home: &Path, slug: &str, keep_diary: bool) -> Result<(bool, Vec<String>), Error> {
    let entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((true, Vec::new())),
        Err(e) => return Err(e.into()),
    };
    if !paths::is_crook_home(home, slug) {
        return Ok((
            false,
            vec![format!(
                "left the home {} as it is: crook did not make it (no {} naming {slug})",
                home.display(),
                paths::HOME_MARKER
            )],
        ));
    }
    let mut kept = false;
    let mut theirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        if name == paths::HOME_MARKER {
            continue;
        }
        if name != "diary" {
            theirs.push(name.to_string_lossy().into_owned());
        } else if keep_diary {
            kept = true;
        } else if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    if kept || !theirs.is_empty() {
        theirs.sort();
        let left = (!theirs.is_empty()).then(|| {
            format!(
                "left in the home {}, not made by crook: {}",
                home.display(),
                theirs.join(", ")
            )
        });
        return Ok((false, left.into_iter().collect()));
    }
    std::fs::remove_file(home.join(paths::HOME_MARKER))?;
    std::fs::remove_dir(home)?;
    Ok((true, Vec::new()))
}

#[cfg(test)]
mod tests;
