//! Building a `claude` launch: argv, environment and reconcile.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::agents::{self, Agent};
use crate::gate::{Identity, Role};
use crate::hook::sessionstart::is_claude;
use crate::{Env, Error, crew, herdr, paths, settings};

pub mod child;
pub mod plugin_dir;
pub mod preflight;
pub mod real;

/// Which conversation the session starts: a new id, or one to resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    New(String),
    Resume(String),
}

/// Everything that goes on the `claude` command line.
#[derive(Debug, Clone)]
pub struct LaunchOpts {
    pub name: String,
    pub session: Session,
    pub permission_mode: String,
    pub plugin_dir: PathBuf,
    pub system_prompt: String,
    pub settings: PathBuf,
    pub add_dirs: Vec<PathBuf>,
}

pub fn argv(o: &LaunchOpts) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["-n".into(), (&o.name).into()];
    match &o.session {
        Session::New(id) => a.extend(["--session-id".into(), id.into()]),
        Session::Resume(id) => a.extend(["--resume".into(), id.into()]),
    }
    a.extend([
        "--permission-mode".into(),
        (&o.permission_mode).into(),
        "--plugin-dir".into(),
        o.plugin_dir.clone().into(),
        "--append-system-prompt".into(),
        (&o.system_prompt).into(),
        "--settings".into(),
        o.settings.clone().into(),
    ]);
    for d in &o.add_dirs {
        a.extend(["--add-dir".into(), d.clone().into()]);
    }
    a
}

fn role_name(r: Role) -> Option<&'static str> {
    match r {
        Role::Orchestrator => Some("orchestrator"),
        Role::Child => Some("child"),
        Role::None => None,
    }
}

/// Variables a crook session never inherits: another session's identity and
/// messaging tokens, and the Claude Code marker. `env` and `claude stop` share it.
pub(crate) fn scrubbed(k: &std::ffi::OsStr) -> bool {
    let k = k.to_string_lossy();
    k.starts_with("CROOK_") || k.starts_with("CLAUDE_CODE_MESSAGING_") || k == "CLAUDECODE"
}

/// The environment a crook session starts with: `base` without any `CROOK_*`,
/// `CLAUDE_CODE_MESSAGING_*` or `CLAUDECODE`, then this session's identity.
pub fn env(
    base: impl Iterator<Item = (OsString, OsString)>,
    id: &Identity,
    crook_bin: &Path,
) -> Vec<(OsString, OsString)> {
    let mut out: Vec<(OsString, OsString)> = base.filter(|(k, _)| !scrubbed(k)).collect();
    out.push(("CROOK_PROJECT".into(), (&id.slug).into()));
    if let Some(role) = role_name(id.role) {
        out.push(("CROOK_ROLE".into(), role.into()));
    }
    if let (Role::Child, Some(child)) = (id.role, &id.child) {
        out.push(("CROOK_CHILD".into(), child.into()));
    }
    out.push(("CROOK_BIN".into(), crook_bin.into()));
    out
}

/// The running crook binary, canonicalised, for `CROOK_BIN` and the hook command.
pub fn crook_bin() -> Result<PathBuf, Error> {
    Ok(std::fs::canonicalize(std::env::current_exe()?)?)
}

/// What reconcile found, read-only: lines to report, and the children whose
/// recorded claude pid is no longer a live claude.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    pub lines: Vec<String>,
    pub stale_pids: Vec<String>,
}

/// How long `open` spends looking up herdr panes in all: each lookup has its
/// own, longer budget, and eight slow ones would hold `open` for minutes.
const HERDR_CHECK_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// What reconcile reads from the machine, live; nothing of it is stored.
pub struct Look<'a> {
    /// `claude agents`, or `None` when it could not say: then no session is
    /// taken for missing.
    pub agents: Option<&'a [Agent]>,
    pub live: &'a dyn Fn(u32) -> bool,
    /// The herdr pane holding a pid, looked up each time; `None` without herdr.
    pub pane_of_pid: Option<&'a dyn Fn(u32) -> Option<String>>,
}

/// Reads what a crash, or a launch that failed, can leave, and says what to
/// do about each:
/// - a child row whose worktree is gone from disk;
/// - a `worktrees` row whose path is gone from disk (one line per path), and
///   a worktree an archived child still has;
/// - a child with no running session (its claude never started, or exited),
///   which still holds one of the `MAX_LIVE_CHILDREN` slots;
/// - a running session in a child's worktree that no row of that child knows;
/// - with herdr, a child's interactive session that no herdr pane holds;
/// - a recorded claude pid that is not a live claude (listed, not yet cleared).
pub fn findings(c: &Connection, slug: &str, look: &Look) -> Result<Findings, Error> {
    let agents = look.agents.unwrap_or_default();
    let live = look.live;
    let mut st = c.prepare(
        "SELECT child, worktree, session_id, claude_pid FROM children
         WHERE archived = 0 ORDER BY child",
    )?;
    let kids = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = Findings::default();
    let mut reported = std::collections::BTreeSet::new();
    let herdr_started = std::time::Instant::now();
    let mut herdr_skipped = false;
    for (child, worktree, session_id, pid) in &kids {
        if !Path::new(worktree).exists() {
            reported.insert(worktree.clone());
            out.lines.push(format!(
                "child {child}: worktree {worktree} is missing on disk: \
                 crook stop {slug}/{child} forgets it"
            ));
        }
        let pid = pid.and_then(|p| u32::try_from(p).ok());
        if pid.is_some_and(|p| !live(p)) {
            out.stale_pids.push(child.clone());
        }
        let mut running = Vec::new();
        for a in agents {
            let known = a.session_id == *session_id
                || crew::is_fork(c, child, &a.session_id)?
                || crew::held(c, child, &a.session_id)?;
            if known {
                running.push(a);
            } else if agents::cwd_inside(&a.cwd, worktree) {
                out.lines.push(format!(
                    "child {child}: session {} is running in {} but no row of {child} knows it",
                    a.session_id, a.cwd
                ));
            }
        }
        if look.agents.is_some() && running.is_empty() && !pid.is_some_and(live) {
            out.lines.push(format!(
                "child {child}: no session is running (its claude never started, or exited), \
                 and it still counts toward the {}-child cap: crook resume {slug}/{child} \
                 to start it, or crook stop {slug}/{child} if it is done",
                child::MAX_LIVE_CHILDREN
            ));
        }
        if let Some(pane_of) = look.pane_of_pid {
            for a in running.iter().filter(|a| a.kind == "interactive") {
                if herdr_started.elapsed() >= HERDR_CHECK_BUDGET {
                    if !herdr_skipped {
                        herdr_skipped = true;
                        out.lines.push(
                            "herdr was slow to answer: the remaining children's panes were not checked"
                                .into(),
                        );
                    }
                    break;
                }
                if let Some(p) = a.pid
                    && pane_of(p).is_none()
                {
                    out.lines.push(format!(
                        "child {child}: session {} (pid {p}) runs in no herdr pane",
                        a.session_id
                    ));
                }
            }
        }
    }
    let archived: std::collections::BTreeSet<String> = {
        let mut st = c.prepare("SELECT child FROM children WHERE archived = 1")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for w in crew::worktrees(c, None)? {
        let child = &w.child;
        if !Path::new(&w.path).exists() {
            if reported.insert(w.path.clone()) {
                out.lines.push(format!(
                    "child {child}: worktree {} is recorded but missing on disk, and its row \
                     keeps that name taken: crook stop {slug}/{child} forgets it",
                    w.path
                ));
            }
        } else if archived.contains(child) {
            out.lines.push(format!(
                "child {child} is archived but its worktree {} was kept: \
                 run crook stop {slug}/{child} again",
                w.path
            ));
        }
    }
    Ok(out)
}

/// `findings` without herdr, then forgets the stale pids on `c` (a
/// transaction, for the caller to commit). `open` reads first and writes only
/// when this would.
pub fn reconcile_with(
    c: &Connection,
    slug: &str,
    agents: &[Agent],
    live: &dyn Fn(u32) -> bool,
) -> Result<Vec<String>, Error> {
    let look = Look {
        agents: Some(agents),
        live,
        pane_of_pid: None,
    };
    let f = findings(c, slug, &look)?;
    for child in &f.stale_pids {
        crew::clear_claude_pid(c, child)?;
    }
    Ok(f.lines)
}

/// What a launch needs from the machine, injected so tests start nothing:
/// the base environment, the crook binary, liveness, the `claude` version and
/// `claude agents`, and where managed settings live.
pub struct Host<'a> {
    pub base_env: Vec<(OsString, OsString)>,
    pub crook_bin: PathBuf,
    pub live: &'a dyn Fn(u32) -> bool,
    pub new_id: &'a dyn Fn() -> String,
    pub version: &'a dyn Fn() -> Result<String, Error>,
    pub agents: &'a dyn Fn() -> Result<Vec<Agent>, Error>,
    /// The herdr pane holding a pid, looked up live; `None` without herdr.
    pub pane_of_pid: Option<&'a dyn Fn(u32) -> Option<String>>,
    pub managed_dirs: Vec<PathBuf>,
}

/// Runs `f` against the real machine: this process's environment and binary,
/// `kill(pid, 0)` plus `ps` for liveness, `claude --version` and
/// `claude agents --json`.
pub fn with_real_host<R>(env: Env, f: impl FnOnce(&Host) -> R) -> Result<R, Error> {
    let version = || agents::claude_version(env);
    let list = || agents::list(env);
    let new_id = || uuid::Uuid::new_v4().to_string();
    let pane = |pid| herdr::pane_of_pid(env, pid);
    let host = Host {
        base_env: std::env::vars_os().collect(),
        crook_bin: crook_bin()?,
        live: &is_claude,
        new_id: &new_id,
        version: &version,
        agents: &list,
        pane_of_pid: herdr::present(env).then_some(&pane as &dyn Fn(u32) -> Option<String>),
        managed_dirs: preflight::default_dirs(),
    };
    Ok(f(&host))
}

/// A `claude` start: the lines to print first, then `exec`.
#[derive(Debug, Clone)]
pub struct Plan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
    pub notes: Vec<String>,
}

/// What a verb does: say something and stop, or start `claude`.
#[derive(Debug)]
pub enum Outcome {
    Say(String),
    Exec(Plan),
}

impl Plan {
    /// Replaces this process with `claude`, on exactly this environment: the
    /// scrub does nothing unless the inherited one is cleared first. Returns
    /// only on failure.
    pub fn exec(&self) -> std::io::Error {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(&self.program)
            .args(&self.args)
            .env_clear()
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&self.cwd)
            .exec()
    }
}

/// The paths and mode every launch of a role shares: the unpacked mod, the
/// settings file written for the role, and the permission mode.
pub struct Prepared {
    pub plugin_dir: PathBuf,
    pub settings: PathBuf,
    pub permission_mode: &'static str,
}

/// Unpacks the mod when missing and writes `<state>/<slug>/settings-<role>.json`.
pub fn prepare(env: Env, slug: &str, role: Role, crook_bin: &Path) -> Result<Prepared, Error> {
    let plugin_dir = plugin_dir::ensure(env)?;
    let settings_bytes = settings::build(&settings::Opts {
        role,
        state_dir: paths::state_dir(env),
        crook_bin: crook_bin.to_path_buf(),
    })?;
    let name = role_name(role).ok_or_else(|| Error::Refused("no role to launch".into()))?;
    let path = paths::project_dir(env, slug)?.join(format!("settings-{name}.json"));
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, settings_bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(Prepared {
        plugin_dir,
        settings: path,
        permission_mode: settings::permission_mode(&settings::user_default_mode(env)),
    })
}

/// Refuses a slug with no database: opening it in `Migrate` mode would create one.
pub fn require_project(env: Env, slug: &str) -> Result<(), Error> {
    if paths::project_dir(env, slug)?.join("crook.db").is_file() {
        Ok(())
    } else {
        Err(Error::Refused(format!(
            "no project {slug}: run crook init {slug}"
        )))
    }
}

/// `crook: warning: …` lines for the preflight checks, or a refusal.
pub fn preflight_notes(env: Env, host: &Host) -> Result<Vec<String>, Error> {
    let version = (host.version)()?;
    Ok(preflight::check_with(env, &version, &host.managed_dirs)?
        .into_iter()
        .map(|w| format!("crook: warning: {w}"))
        .collect())
}

/// `claude` as the injected `PATH` finds it.
pub fn claude_program(env: Env) -> Result<PathBuf, Error> {
    agents::find_claude(env).ok_or_else(|| Error::Refused("claude not found on PATH".into()))
}

/// Folders that go on `--add-dir`, in order, each once.
pub fn dedup_dirs(dirs: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in dirs {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod fixture;
