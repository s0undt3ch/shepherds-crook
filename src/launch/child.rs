//! Starting and stopping child sessions: the `spawn`, `worktree` and `stop` work.
//!
//! Every write to the database comes before the action it describes, so a crash
//! between the two leaves rows that `crook open` reconciles. `git` and `claude`
//! sit behind `Launcher`, so tests start neither.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use crate::agents::Agent;
use crate::gate::{Identity, Role};
use crate::store::{Store, Touch};
use crate::{Env, Error, crew, herdr, launch, paths, project};

use super::{LaunchOpts, Session};

/// The limits on a spawn, in one place.
pub const MAX_BRIEF_CHARS: usize = 4000;
pub const MAX_LIVE_CHILDREN: i64 = 8;

pub(crate) const PROMPT: &str = include_str!("../../plugin/prompts/child.md");

/// How long each command may run. The mod gives `spawn` 120 s in all, and a
/// spawn runs `git worktree add` then `claude --bg`: 90 s for the checkout of a
/// large repo and 20 s for `claude --bg` to return leave 10 s for the rest.
/// Other `git` and `claude` calls are quick.
pub const WORKTREE_ADD_TIMEOUT: Duration = Duration::from_secs(90);
pub const CLAUDE_START_TIMEOUT: Duration = Duration::from_secs(20);
pub const GIT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long each `herdr` call may run. A herdr spawn is `git worktree add`,
/// then a pane split and a pane run: 90 + 5 + 5 s stays inside the mod's 120 s.
pub const HERDR_TIMEOUT: Duration = Duration::from_secs(5);

/// How a child's `claude` starts: detached with `--bg`, or in a herdr pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Bg,
    Herdr,
}

/// A `claude` start: what the launcher runs, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCmd {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
    /// What a herdr pane types instead of `program` and `args`: only
    /// `crook launch`, which rebuilds both from the child's rows. `None` for `--bg`.
    pub pane: Option<PaneLaunch>,
}

/// The `crook launch <slug>/<child>` a herdr pane runs to start a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLaunch {
    pub slug: String,
    pub child: String,
    pub crook_bin: PathBuf,
}

/// What `spawn`, `worktree` and `stop` ask of the machine, injected so tests
/// run no `git` and start no `claude`.
pub trait Launcher {
    /// Whether this launcher can start in `mode`; asked before any row is written.
    fn check(&self, _mode: Mode) -> Result<(), Error> {
        Ok(())
    }
    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<(), Error>;
    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), Error>;
    /// The paths `git worktree list --porcelain` shows in `repo`.
    fn worktree_list(&self, repo: &Path) -> Result<Vec<PathBuf>, Error>;
    /// Whether `git -C <path> status --porcelain` prints anything.
    fn worktree_dirty(&self, path: &Path) -> Result<bool, Error>;
    /// `git -C <repo> worktree prune`.
    fn worktree_prune(&self, repo: &Path) -> Result<(), Error>;
    /// The local branches in `repo` named `<prefix>...`.
    fn branches(&self, repo: &Path, prefix: &str) -> Result<Vec<String>, Error>;
    fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error>;
    fn agents(&self) -> Result<Vec<Agent>, Error>;
    /// `claude stop <session id>`.
    fn stop_bg(&self, session_id: &str) -> Result<(), Error>;
    /// SIGTERM.
    fn terminate(&self, pid: u32) -> Result<(), Error>;
    /// Closes the herdr tab crook opened for `child`, found by the session while
    /// it still runs. A pane crook did not open (one `crook resume` ran in) is
    /// left alone, as is the lack of herdr or of any pane: none is an error.
    fn close_pane(&self, session_id: &str, child: &str) -> Result<PaneClose, Error>;
    /// How long `stop` waits for a stopped session to leave `claude agents`.
    fn exit_wait(&self) -> Duration {
        Duration::from_secs(3)
    }
}

/// What `close_pane` did with the session's pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneClose {
    #[default]
    Closed,
    /// Not one crook opened, so left open.
    NotCrooks,
    /// No herdr, or no pane runs the session.
    NoPane,
}

/// A spawn: what the orchestrator asked for, and the machine it runs on.
pub struct SpawnReq<'a> {
    pub env: Env<'a>,
    pub slug: String,
    pub child: String,
    pub repo: String,
    pub brief: String,
    pub branch: Option<String>,
    /// The session id the child starts with, chosen by the caller.
    pub session_id: String,
    pub base_env: Vec<(OsString, OsString)>,
    pub crook_bin: PathBuf,
}

/// A branch name goes to `git` as an argument, so it must not read as an
/// option or carry whitespace. `git` checks the rest.
fn valid_branch(b: &str) -> Result<(), Error> {
    if b.is_empty() || b.starts_with('-') || b.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(Error::Refused(format!("invalid branch name {b:?}")));
    }
    Ok(())
}

fn same_path(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
        || matches!(
            (std::fs::canonicalize(a), std::fs::canonicalize(b)),
            (Ok(x), Ok(y)) if x == y
        )
}

pub(crate) fn launch_prompt(slug: &str, child: &str) -> String {
    format!("You are child {child} of crook-{slug}. Your project card is above.")
}

/// The `claude` arguments for a child. The prompt follows `--`: `--add-dir`
/// takes any number of values, so a bare prompt after it would be one more folder.
pub fn child_args(o: &LaunchOpts, mode: Mode, prompt: &str) -> Vec<OsString> {
    let mut a = launch::argv(o);
    if mode == Mode::Bg {
        a.push("--bg".into());
    }
    a.extend(["--".into(), prompt.into()]);
    a
}

/// Starts a child in a new worktree of `req.repo` and returns its session id.
///
/// Checks first, then writes the rows (the child, its first session id held,
/// and its worktree), then runs `git` and starts `claude`.
pub fn spawn_child(store: &mut Store, req: &SpawnReq, run: &dyn Launcher) -> Result<String, Error> {
    let (slug, child) = (req.slug.as_str(), req.child.as_str());
    paths::valid_slug(child)
        .map_err(|_| Error::Refused(format!("invalid child name {child:?}")))?;
    if req.brief.trim().is_empty() {
        return Err(Error::Refused("empty brief".into()));
    }
    let chars = req.brief.chars().count();
    if chars > MAX_BRIEF_CHARS {
        return Err(Error::Refused(format!(
            "the brief is {chars} characters; the limit is {MAX_BRIEF_CHARS}"
        )));
    }
    let branch = match &req.branch {
        Some(b) => {
            valid_branch(b)?;
            b.clone()
        }
        None => format!("crook/{slug}/{child}"),
    };
    let record = store.read(project::get)?;
    if !record.repos.iter().any(|r| same_path(r, &req.repo)) {
        return Err(Error::Refused(format!(
            "{} is not a repo of project {slug}: add it with crook project {slug} add-repo",
            req.repo
        )));
    }
    let repo = std::fs::canonicalize(&req.repo)
        .map_err(|e| Error::Refused(format!("repo {}: {e}", req.repo)))?;
    if store.read(crew::live_count)? >= MAX_LIVE_CHILDREN {
        return Err(too_many());
    }
    let mode = if herdr::present(req.env) {
        Mode::Herdr
    } else {
        Mode::Bg
    };
    run.check(mode)?;
    let program = launch::claude_program(req.env)?;
    // Unpacks the mod when this binary was upgraded since the project was opened.
    let prep = launch::prepare(req.env, slug, Role::Child, &req.crook_bin)?;

    let worktree = repo.join(".worktrees").join(slug).join(child);
    let (repo_s, worktree_s) = (repo.display().to_string(), worktree.display().to_string());
    store.write(Touch::CHILDREN | Touch::STATUS, |tx| {
        // Counted again inside the transaction: two spawns can pass the check above.
        if crew::live_count(tx)? >= MAX_LIVE_CHILDREN {
            return Err(too_many());
        }
        crew::add_child(
            tx,
            &crew::Child {
                child: child.into(),
                repo: repo_s.clone(),
                worktree: worktree_s.clone(),
                branch: branch.clone(),
                brief: req.brief.clone(),
                session_id: req.session_id.clone(),
            },
        )?;
        crew::add_worktree(
            tx,
            &crew::Worktree {
                child: child.into(),
                repo: repo_s.clone(),
                path: worktree_s.clone(),
                branch: branch.clone(),
            },
        )
    })?;

    run.worktree_add(&repo, &worktree, &branch)?;
    let args = child_args(
        &LaunchOpts {
            name: format!("crook-{slug}-{child}"),
            session: Session::New(req.session_id.clone()),
            permission_mode: prep.permission_mode.into(),
            plugin_dir: prep.plugin_dir,
            system_prompt: PROMPT.into(),
            settings: prep.settings,
            add_dirs: launch::dedup_dirs(record.notes.iter().map(PathBuf::from)),
        },
        mode,
        &launch_prompt(slug, child),
    );
    let id = Identity {
        slug: slug.into(),
        role: Role::Child,
        child: Some(child.into()),
    };
    run.start(
        mode,
        &StartCmd {
            program,
            args,
            env: launch::env(req.base_env.iter().cloned(), &id, &req.crook_bin),
            cwd: worktree,
            pane: (mode == Mode::Herdr).then(|| PaneLaunch {
                slug: slug.into(),
                child: child.into(),
                crook_bin: req.crook_bin.clone(),
            }),
        },
    )?;
    Ok(req.session_id.clone())
}

fn too_many() -> Error {
    Error::Refused(format!(
        "{MAX_LIVE_CHILDREN} live children already: stop one first"
    ))
}

/// A child's extra worktree, from the `worktree` verb.
pub struct WorktreeReq {
    pub child: String,
    pub session_id: String,
    pub name: String,
    pub branch: Option<String>,
}

/// Makes `<repo>/.worktrees/<slug>/<child>.<name>` for the calling child and
/// returns it. The path is built here from the child's recorded repo: nothing
/// in the request names a path. The row is written first.
pub fn add_worktree(
    store: &mut Store,
    req: &WorktreeReq,
    run: &dyn Launcher,
) -> Result<PathBuf, Error> {
    paths::valid_slug(&req.name)
        .map_err(|_| Error::Refused(format!("invalid worktree name {:?}", req.name)))?;
    let slug = store.slug().to_string();
    let branch = match &req.branch {
        Some(b) => {
            valid_branch(b)?;
            b.clone()
        }
        None => format!("crook/{slug}/{}.{}", req.child, req.name),
    };
    let (repo, path) = store.write(Touch::CHILDREN, |tx| {
        crew::guard(tx, &req.child, &req.session_id, true)?;
        let repo = crew::child_repo(tx, &req.child)?;
        let path = Path::new(&repo)
            .join(".worktrees")
            .join(&slug)
            .join(format!("{}.{}", req.child, req.name));
        let path_s = path.display().to_string();
        if crew::worktrees(tx, Some(&req.child))?
            .iter()
            .any(|w| w.path == path_s)
        {
            return Err(Error::Refused(format!(
                "worktree name {:?} is already used by {}",
                req.name, req.child
            )));
        }
        crew::add_worktree(
            tx,
            &crew::Worktree {
                child: req.child.clone(),
                repo: repo.clone(),
                path: path_s,
                branch: branch.clone(),
            },
        )?;
        Ok((repo, path))
    })?;
    run.worktree_add(Path::new(&repo), &path, &branch)?;
    Ok(path)
}

/// Paths from `git worktree list --porcelain`.
pub fn parse_worktree_list(porcelain: &str) -> Vec<PathBuf> {
    porcelain
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect()
}

/// Checks a recorded worktree before anything is removed or changed, and
/// returns its canonical path, or `None` when it is already gone from disk.
/// `stop` and `wipe` share it. `path` must be:
/// - not a symlink, and directly under `<repo>/.worktrees/<slug>/` once every
///   link is resolved (so a linked `.worktrees` or `<slug>` is refused),
/// - named `<child>` or `<child>.<name>`,
/// - one of the worktrees `git worktree list` shows in `repo`.
pub fn verify_worktree(
    run: &dyn Launcher,
    repo: &str,
    slug: &str,
    child: &str,
    path: &Path,
) -> Result<Option<PathBuf>, Error> {
    let refuse = |why: &str| {
        Error::Refused(format!(
            "worktree {} is not removable ({why}); nothing was changed",
            path.display()
        ))
    };
    let prefix = Path::new(repo).join(".worktrees").join(slug);
    let lexical_ok = path.starts_with(&prefix)
        && path.parent() == Some(prefix.as_path())
        && path
            .components()
            .all(|c| !matches!(c, Component::ParentDir | Component::CurDir));
    if !lexical_ok {
        return Err(refuse(&format!("not directly under {}", prefix.display())));
    }
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(refuse(&e.to_string())),
        Ok(m) if m.file_type().is_symlink() => return Err(refuse("it is a symlink")),
        Ok(_) => {}
    }
    let real_repo = std::fs::canonicalize(repo).map_err(|e| refuse(&format!("repo: {e}")))?;
    let real_prefix = std::fs::canonicalize(&prefix)
        .map_err(|e| refuse(&format!("{}: {e}", prefix.display())))?;
    if real_prefix != real_repo.join(".worktrees").join(slug) {
        return Err(refuse(&format!(
            ".worktrees/{slug} resolves outside the repo, to {}",
            real_prefix.display()
        )));
    }
    let real = std::fs::canonicalize(path).map_err(|e| refuse(&e.to_string()))?;
    let name = real
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let named = name == child
        || name
            .strip_prefix(child)
            .and_then(|r| r.strip_prefix('.'))
            .is_some_and(|n| paths::valid_slug(n).is_ok());
    if real.parent() != Some(real_prefix.as_path()) || !named {
        return Err(refuse(&format!(
            "it is not {child} or {child}.<name> under the project"
        )));
    }
    let listed = run.worktree_list(Path::new(repo))?;
    if !listed
        .iter()
        .any(|l| std::fs::canonicalize(l).is_ok_and(|l| l == real))
    {
        return Err(refuse("git does not list it as a worktree of the repo"));
    }
    Ok(Some(real))
}

/// The `claude agents` rows whose cwd is one of `dirs` or below it: the
/// sessions that removing those directories would pull the floor from under.
pub fn sessions_inside<'a>(agents: &'a [Agent], dirs: &[&Path]) -> Vec<&'a Agent> {
    agents
        .iter()
        .filter(|a| {
            dirs.iter()
                .filter_map(|d| d.to_str())
                .any(|d| crate::agents::cwd_inside(&a.cwd, d))
        })
        .collect()
}

/// `sessions_inside` rows, for a message.
pub fn describe_sessions(found: &[&Agent]) -> String {
    found
        .iter()
        .map(|a| {
            let id = if a.session_id.is_empty() {
                "with no id"
            } else {
                a.session_id.as_str()
            };
            match a.pid {
                Some(pid) => format!("session {id} (pid {pid}) in {}", a.cwd),
                None => format!("session {id} in {}", a.cwd),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// What `stop` did to the child's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    Background,
    Pane,
    /// Signalled; `pane_left_open` when it ran in a pane crook did not open.
    Signalled {
        pane_left_open: bool,
    },
    /// `claude agents` listed no session for the child.
    NoSession,
    /// The session may still run: archived only, for this reason.
    ArchivedOnly(String),
}

impl Stopped {
    pub fn describe(&self) -> String {
        match self {
            Stopped::Background => "stopped the background session".into(),
            Stopped::Pane => "stopped the session and closed its pane".into(),
            Stopped::Signalled {
                pane_left_open: true,
            } => {
                "stopped the session; the pane it ran in is not one crook opened, so it stays open"
                    .into()
            }
            Stopped::Signalled {
                pane_left_open: false,
            } => "stopped the session".into(),
            Stopped::NoSession => "no running session".into(),
            Stopped::ArchivedOnly(why) => format!("archived only: {why}"),
        }
    }

    /// Whether the session is confirmed gone, so its worktrees are safe to remove.
    pub fn session_gone(&self) -> bool {
        !matches!(self, Stopped::ArchivedOnly(_))
    }
}

/// Archives the child, then stops its session by what `claude agents` shows.
/// A background session is stopped by id. An interactive one is signalled only
/// when its row shows the recorded pid running the child's session: a pid
/// from before a reboot or crash may be anything now.
pub fn archive_and_stop(
    store: &mut Store,
    child: &str,
    run: &dyn Launcher,
) -> Result<Stopped, Error> {
    let row = store.read(|c| crew::resumable(c, child))?;
    store.write(Touch::CHILDREN, |tx| crew::archive(tx, child))?;
    let agents = match run.agents() {
        Ok(a) => a,
        Err(e) => {
            return Ok(Stopped::ArchivedOnly(format!(
                "{e}; no session was signalled"
            )));
        }
    };
    let Some(a) = agents.iter().find(|a| a.session_id == row.session_id) else {
        return Ok(Stopped::NoSession);
    };
    match a.kind.as_str() {
        "background" => {
            run.stop_bg(&row.session_id).map_err(|e| {
                Error::Refused(format!(
                    "{child} is archived, but its session was not stopped and its worktrees were kept: {e}"
                ))
            })?;
            Ok(Stopped::Background)
        }
        "interactive" => match (row.claude_pid, a.pid) {
            (Some(recorded), Some(running)) if recorded == running => {
                // The pane is found by the session's pid, which is gone once
                // signalled, so it closes first. Closing it hangs up the
                // session; the SIGTERM covers a session that outlives that.
                // A close error does not skip the signal.
                let closed = run.close_pane(&row.session_id, child);
                run.terminate(recorded)?;
                Ok(match closed? {
                    PaneClose::Closed => Stopped::Pane,
                    PaneClose::NotCrooks => Stopped::Signalled {
                        pane_left_open: true,
                    },
                    PaneClose::NoPane => Stopped::Signalled {
                        pane_left_open: false,
                    },
                })
            }
            _ => Ok(Stopped::ArchivedOnly(
                "the recorded pid is not running the session".into(),
            )),
        },
        other => Ok(Stopped::ArchivedOnly(format!(
            "unknown session kind {other:?}"
        ))),
    }
}

#[cfg(test)]
pub(crate) mod fake;

#[cfg(test)]
mod tests;
