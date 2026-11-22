//! `crook launch <slug>/<child>`, hidden: what a herdr pane runs to start a
//! spawned child. The pane types only this command, so no environment value
//! or prompt text lands in the shell; the environment and argv come from the
//! child's rows.

use std::path::{Path, PathBuf};

use crate::gate::{Identity, Role};
use crate::launch::child::{self, Mode as Start};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store};
use crate::{Env, Error, crew, project};

use super::{address, resume};

/// The `claude` start of a child that has not run yet: the arguments a `--bg`
/// spawn gives it, without `--bg`, on its recorded session id and in its
/// worktree. A child that is running is refused; `crook resume` restarts one
/// that stopped.
pub fn plan(env: Env, arg: &str, host: &Host) -> Result<Outcome, Error> {
    let (slug, name) = address::resolve(env, arg)?;
    launch::require_project(env, &slug)?;
    let mut notes = launch::preflight_notes(env, host)?;
    let store = Store::open(env, &slug, Mode::Migrate)?;
    let row = store.read(|c| crew::resumable(c, &name))?;
    if row.archived {
        return Err(Error::Refused(format!("child {name} is archived")));
    }
    if !Path::new(&row.worktree).is_dir() {
        return Err(Error::Refused(format!(
            "child {name}: worktree {} is missing on disk",
            row.worktree
        )));
    }
    resume::ensure_not_running(&store, &name, &row, host, &mut notes)?;

    let prep = launch::prepare(env, &slug, Role::Child, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let args = child::child_args(
        &LaunchOpts {
            name: format!("crook-{slug}-{name}"),
            session: Session::New(row.session_id),
            permission_mode: prep.permission_mode.into(),
            plugin_dir: prep.plugin_dir,
            system_prompt: child::PROMPT.into(),
            settings: prep.settings,
            add_dirs: launch::dedup_dirs(record.notes.iter().map(PathBuf::from)),
        },
        Start::Herdr,
        &child::launch_prompt(&slug, &name),
    );
    let id = Identity {
        slug: slug.clone(),
        role: Role::Child,
        child: Some(name),
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
