//! `crook resume <child>`: restarts a child's current session.

use std::path::{Path, PathBuf};

use crate::gate::{Identity, Role};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew, project};

use super::address;

const PROMPT: &str = include_str!("../../plugin/prompts/child.md");

/// Refuses a child whose session runs: a live recorded pid, or a session or
/// fork of it in `claude agents`. Without `claude agents`, a note says it
/// could not look.
pub(super) fn ensure_not_running(
    store: &Store,
    child: &str,
    row: &crew::Resumable,
    host: &Host,
    notes: &mut Vec<String>,
) -> Result<(), Error> {
    if let Some(pid) = row.claude_pid.filter(|p| (host.live)(*p)) {
        return Err(Error::Refused(format!(
            "child {child} is already running (claude pid {pid})"
        )));
    }
    match (host.agents)() {
        Ok(agents) => {
            let running = store.read(|c| {
                for a in &agents {
                    if a.session_id == row.session_id || crew::is_fork(c, child, &a.session_id)? {
                        return Ok(Some(a.session_id.clone()));
                    }
                }
                Ok(None)
            })?;
            if let Some(sid) = running {
                return Err(Error::Refused(format!(
                    "child {child} is already running (session {sid})"
                )));
            }
        }
        Err(e) => notes.push(format!(
            "crook: warning: {e}; a running session of {child} would not have been noticed"
        )),
    }
    Ok(())
}

/// The `claude` start for a child's current session, with the flags, environment
/// and cwd it was spawned with. Refuses a child that is running: forgetting the
/// recorded pid would lift the SessionStart guard that protects its row.
pub fn plan(env: Env, arg: &str, host: &Host) -> Result<Outcome, Error> {
    let (slug, child) = address::resolve(env, arg)?;
    launch::require_project(env, &slug)?;
    let mut notes = launch::preflight_notes(env, host)?;
    let mut store = Store::open(env, &slug, Mode::Migrate)?;
    let row = store.read(|c| crew::resumable(c, &child))?;
    if row.archived {
        return Err(Error::Refused(format!("child {child} is archived")));
    }
    if !Path::new(&row.worktree).is_dir() {
        return Err(Error::Refused(format!(
            "child {child}: worktree {} is missing on disk",
            row.worktree
        )));
    }
    ensure_not_running(&store, &child, &row, host, &mut notes)?;
    store.write(Touch::CHILDREN, |tx| crew::clear_claude_pid(tx, &child))?;

    let prep = launch::prepare(env, &slug, Role::Child, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let add_dirs = launch::dedup_dirs(record.notes.iter().map(PathBuf::from));
    let args = launch::argv(&LaunchOpts {
        name: format!("crook-{slug}-{child}"),
        session: Session::Resume(row.session_id),
        permission_mode: prep.permission_mode.into(),
        plugin_dir: prep.plugin_dir,
        system_prompt: PROMPT.into(),
        settings: prep.settings,
        add_dirs,
    });
    let id = Identity {
        slug,
        role: Role::Child,
        child: Some(child),
    };
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args,
        env: launch::env(host.base_env.iter().cloned(), &id, &host.crook_bin),
        cwd: PathBuf::from(row.worktree),
        notes,
    }))
}

#[cfg(test)]
mod tests;
