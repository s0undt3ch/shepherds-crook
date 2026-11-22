//! `crook open <slug>`: starts the project's orchestrator.

use std::path::Path;

use crate::gate::{Identity, Role};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config, crew, herdr, paths, project};

const PROMPT: &str = include_str!("../../plugin/prompts/orchestrator.md");

/// The orchestrator row, read without migrating. A database no `open` has
/// migrated yet has no such table, so it has no row.
fn recorded_orchestrator(env: Env, slug: &str) -> Result<(Option<String>, Option<u32>), Error> {
    let store = Store::open(env, slug, Mode::ReadOnly)?;
    store.read(|c| {
        let v: i32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v == 0 {
            Ok((None, None))
        } else {
            crew::orchestrator(c)
        }
    })
}

/// Whether two paths are the same directory, as written or as resolved
/// (`/tmp` is `/private/tmp` on macOS).
fn same_dir(a: &str, b: &Path) -> bool {
    Path::new(a) == b
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

/// Starts the orchestrator in a tab of the project's herdr workspace, by typing
/// `crook open <slug>` there: that runs in place, and this pane is left alone.
fn move_to_workspace(
    env: Env,
    slug: &str,
    home: &Path,
    host: &Host,
    cfg: &config::Config,
) -> Result<String, Error> {
    paths::make_home(home, slug)?;
    let label = cfg.orchestrator_label();
    let tab = herdr::open_orchestrator_tab(
        env,
        slug,
        home,
        label,
        &[host.crook_bin.clone().into(), "open".into(), slug.into()],
    )?;
    Ok(format!(
        "crook: the orchestrator for {slug} starts in herdr workspace crook-{slug}, tab {tab} ({label})"
    ))
}

/// What `open` does for `slug`: attach-by-message when its orchestrator runs,
/// else the `claude` start. Nothing is written to the orchestrator row here:
/// the SessionStart hook records it once claude is up, so a `claude` that fails
/// to start leaves no session id to resume.
pub fn plan(env: Env, slug: &str, host: &Host) -> Result<Outcome, Error> {
    paths::valid_slug(slug)?;
    launch::require_project(env, slug)?;
    let cfg = config::load(env)?;
    let home = cfg.project_home(env, slug)?;
    let diary = cfg.diary(env, slug)?;

    // Everything that can refuse comes before the first migration.
    let mut notes = launch::preflight_notes(env, host)?;
    let (session_id, pid) = recorded_orchestrator(env, slug)?;
    if let Some(pid) = pid.filter(|p| (host.live)(*p)) {
        let running = format!(
            "crook: the orchestrator for {slug} is already running (pid {pid}, session {})",
            session_id.as_deref().unwrap_or("unknown")
        );
        if herdr::present(env)
            && let Some(pane) = herdr::pane_of_pid(env, pid)
        {
            return Ok(Outcome::Say(match herdr::focus(env, &pane) {
                Ok(()) => format!("{running}; focused its pane {pane}"),
                Err(e) => format!("{running}; could not focus its pane {pane}: {e}"),
            }));
        }
        return Ok(Outcome::Say(format!(
            "{running}; not starting a second one"
        )));
    }
    let listed = match (host.agents)() {
        Ok(a) => Some(a),
        Err(e) => {
            notes.push(format!(
                "crook: warning: {e}; which children have a running session was not checked"
            ));
            None
        }
    };
    let agents = listed.as_deref().unwrap_or_default();
    // The row is written by SessionStart, after claude is up (and after the
    // trust dialog, on a first open), so a second `open` in that window finds
    // no live pid: `claude agents` shows the session already in the home.
    if let Some(a) = agents
        .iter()
        .find(|a| a.kind == "interactive" && same_dir(&a.cwd, &home))
    {
        return Ok(Outcome::Say(format!(
            "crook: a session is already running in {} (session {}{}); not starting a second \
             orchestrator",
            home.display(),
            a.session_id,
            a.pid.map_or(String::new(), |p| format!(", pid {p}")),
        )));
    }
    if herdr::present(env) {
        match herdr::in_workspace(env, slug) {
            Ok(true) => {}
            Ok(false) => {
                return move_to_workspace(env, slug, &home, host, &cfg).map(Outcome::Say);
            }
            Err(e) => notes.push(format!(
                "crook: warning: could not check the herdr workspace: {e}; starting here"
            )),
        }
    }
    if session_id.is_none() {
        notes.insert(
            0,
            format!(
                "crook: Claude Code will ask you to trust {} the first time; accept it once.",
                home.display()
            ),
        );
    }

    let mut store = Store::open(env, slug, Mode::Migrate)?;
    // Read first: a clean project takes no write transaction and bumps no `gen`.
    let look = launch::Look {
        agents: listed.as_deref(),
        live: host.live,
        pane_of_pid: host.pane_of_pid,
    };
    let found = store.read(|c| launch::findings(c, slug, &look))?;
    if !found.stale_pids.is_empty() {
        store.write(Touch::CHILDREN, |tx| {
            found
                .stale_pids
                .iter()
                .try_for_each(|c| crew::clear_claude_pid(tx, c))
        })?;
    }
    notes.extend(found.lines.into_iter().map(|l| format!("crook: {l}")));
    notes.push(
        "crook: warning: MCP tools able to read other projects (a loam plot reader, say) \
         are not closed"
            .into(),
    );

    paths::make_home(&home, slug)?;
    std::fs::create_dir_all(&diary)?;
    let prep = launch::prepare(env, slug, Role::Orchestrator, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let outside = record
        .notes
        .iter()
        .filter(|n| !Path::new(n).starts_with(&home))
        .map(std::path::PathBuf::from);
    let add_dirs = launch::dedup_dirs(outside.chain(record.repos.iter().map(Into::into)));
    let session = match session_id {
        Some(id) => Session::Resume(id),
        None => Session::New((host.new_id)()),
    };
    let args = launch::argv(&LaunchOpts {
        name: format!("crook-{slug}"),
        session,
        permission_mode: prep.permission_mode.into(),
        plugin_dir: prep.plugin_dir,
        system_prompt: PROMPT.into(),
        settings: prep.settings,
        add_dirs,
    });
    let id = Identity {
        slug: slug.into(),
        role: Role::Orchestrator,
        child: None,
    };
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args,
        env: launch::env(host.base_env.iter().cloned(), &id, &host.crook_bin),
        cwd: home,
        notes,
    }))
}

#[cfg(test)]
mod tests;
