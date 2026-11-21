//! Starting and stopping child sessions: the `spawn`, `worktree` and `stop` work.
//!
//! Every write to the database comes before the action it describes, so a crash
//! between the two leaves rows that `crook open` reconciles. `git` and `claude`
//! sit behind `Launcher`, so tests start neither.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::agents::{self, Agent};
use crate::gate::{Identity, Role};
use crate::store::{Store, Touch};
use crate::{Env, Error, crew, herdr, launch, paths, project};

use super::{LaunchOpts, Session};

/// The limits on a spawn, in one place.
pub const MAX_BRIEF_CHARS: usize = 4000;
pub const MAX_LIVE_CHILDREN: i64 = 8;

const PROMPT: &str = include_str!("../../plugin/prompts/child.md");

/// How long `git` and `claude --bg` may run.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

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
    fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error>;
    fn agents(&self) -> Result<Vec<Agent>, Error>;
    /// `claude stop <session id>`.
    fn stop_bg(&self, session_id: &str) -> Result<(), Error>;
    /// SIGTERM.
    fn terminate(&self, pid: u32) -> Result<(), Error>;
    fn close_pane(&self, session_id: &str) -> Result<(), Error>;
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

fn launch_prompt(slug: &str, child: &str) -> String {
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

/// What `stop` did to the child's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    Background,
    Pane,
    ArchivedOnly(String),
}

impl Stopped {
    pub fn describe(&self) -> String {
        match self {
            Stopped::Background => "stopped the background session".into(),
            Stopped::Pane => "stopped the session and closed its pane".into(),
            Stopped::ArchivedOnly(why) => format!("archived only: {why}"),
        }
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
        return Ok(Stopped::ArchivedOnly("no running session".into()));
    };
    match a.kind.as_str() {
        "background" => {
            run.stop_bg(&row.session_id)?;
            Ok(Stopped::Background)
        }
        "interactive" => match (row.claude_pid, a.pid) {
            (Some(recorded), Some(running)) if recorded == running => {
                run.terminate(recorded)?;
                run.close_pane(&row.session_id)?;
                Ok(Stopped::Pane)
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

/// `git` and `claude` on the injected `PATH`.
pub struct RealLauncher<'a> {
    env: Env<'a>,
}

impl<'a> RealLauncher<'a> {
    pub fn new(env: Env<'a>) -> Self {
        RealLauncher { env }
    }

    fn git(&self) -> Result<PathBuf, Error> {
        find_exe(self.env, "git").ok_or_else(|| Error::Refused("git not found on PATH".into()))
    }

    fn git_out(&self, args: &[&std::ffi::OsStr]) -> Result<String, Error> {
        let mut cmd = Command::new(self.git()?);
        cmd.args(args);
        run_captured(cmd, COMMAND_TIMEOUT, "git")
    }
}

fn find_exe(env: Env, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env("PATH")?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join(name))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Runs `cmd` with no stdin and returns its stdout, or an error carrying its
/// stderr. Waits for the process, not for the pipes to close: a `claude --bg`
/// leaves a background session that may keep them open.
fn run_captured(mut cmd: Command, timeout: Duration, what: &str) -> Result<String, Error> {
    let fail = |msg: String| Error::Io(std::io::Error::other(format!("{what}: {msg}")));
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(e.to_string()))?;
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>)>();
    for (is_err, pipe) in [
        (
            false,
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
        (
            true,
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
    ] {
        if let Some(mut pipe) = pipe {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 || tx.send((is_err, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            });
        }
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(format!("timed out after {} s", timeout.as_secs())));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(fail(e.to_string())),
        }
    };
    // What the process wrote before it exited; a pipe held open by a
    // descendant ends the wait at the deadline.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let end = Instant::now() + Duration::from_millis(300);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((true, b)) => err.extend(b),
            Ok((false, b)) => out.extend(b),
            Err(_) => break,
        }
    }
    if !status.success() {
        let err = String::from_utf8_lossy(&err);
        let msg = err.trim();
        return Err(fail(if msg.is_empty() {
            format!("exited with {status}")
        } else {
            msg.chars().take(500).collect()
        }));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

impl Launcher for RealLauncher<'_> {
    fn check(&self, mode: Mode) -> Result<(), Error> {
        match mode {
            Mode::Bg => Ok(()),
            Mode::Herdr => Err(Error::Refused("herdr panes arrive with Task 20".into())),
        }
    }

    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<(), Error> {
        use std::ffi::OsStr;
        self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "add".as_ref(),
            path.as_os_str(),
            "-b".as_ref(),
            OsStr::new(branch),
        ])
        .map(drop)
    }

    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), Error> {
        self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "remove".as_ref(),
            path.as_os_str(),
        ])
        .map(drop)
    }

    fn worktree_list(&self, repo: &Path) -> Result<Vec<PathBuf>, Error> {
        let out = self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "list".as_ref(),
            "--porcelain".as_ref(),
        ])?;
        Ok(parse_worktree_list(&out))
    }

    fn worktree_dirty(&self, path: &Path) -> Result<bool, Error> {
        let out = self.git_out(&[
            "-C".as_ref(),
            path.as_os_str(),
            "status".as_ref(),
            "--porcelain".as_ref(),
        ])?;
        Ok(!out.trim().is_empty())
    }

    fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error> {
        self.check(mode)?;
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .env_clear()
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&cmd.cwd);
        run_captured(c, COMMAND_TIMEOUT, "claude").map(drop)
    }

    fn agents(&self) -> Result<Vec<Agent>, Error> {
        agents::list(self.env)
    }

    fn stop_bg(&self, session_id: &str) -> Result<(), Error> {
        let claude = launch::claude_program(self.env)?;
        let mut c = Command::new(claude);
        c.args(["stop", session_id]);
        run_captured(c, COMMAND_TIMEOUT, "claude stop").map(drop)
    }

    fn terminate(&self, pid: u32) -> Result<(), Error> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let raw = i32::try_from(pid)
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| Error::Refused(format!("not a pid: {pid}")))?;
        kill(Pid::from_raw(raw), Signal::SIGTERM)
            .map_err(|e| Error::Io(std::io::Error::other(format!("kill {pid}: {e}"))))
    }

    /// herdr panes arrive with Task 20; a `--bg` or unpaned child has none.
    fn close_pane(&self, _session_id: &str) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::BTreeSet;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Call {
        Add(PathBuf, PathBuf, String),
        Remove(PathBuf, PathBuf),
        Start(Mode, StartCmd),
        StopBg(String),
        Terminate(u32),
        ClosePane(String),
    }

    /// A launcher that records what it was asked and runs nothing.
    #[derive(Default)]
    pub struct Fake {
        pub calls: RefCell<Vec<Call>>,
        pub fail_add: bool,
        pub fail_start: bool,
        pub dirty: BTreeSet<PathBuf>,
        pub agents: Vec<Agent>,
        pub agents_fail: bool,
    }

    impl Fake {
        pub fn calls(&self) -> Vec<Call> {
            self.calls.borrow().clone()
        }
        pub fn starts(&self) -> Vec<(Mode, StartCmd)> {
            self.calls()
                .into_iter()
                .filter_map(|c| match c {
                    Call::Start(m, s) => Some((m, s)),
                    _ => None,
                })
                .collect()
        }
        fn log(&self, c: Call) {
            self.calls.borrow_mut().push(c);
        }
    }

    impl Launcher for Fake {
        fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<(), Error> {
            self.log(Call::Add(repo.into(), path.into(), branch.into()));
            if self.fail_add {
                return Err(Error::Refused("git failed".into()));
            }
            Ok(())
        }
        fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), Error> {
            self.log(Call::Remove(repo.into(), path.into()));
            Ok(())
        }
        fn worktree_list(&self, _repo: &Path) -> Result<Vec<PathBuf>, Error> {
            Ok(Vec::new())
        }
        fn worktree_dirty(&self, path: &Path) -> Result<bool, Error> {
            Ok(self.dirty.contains(path))
        }
        fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error> {
            self.log(Call::Start(mode, cmd.clone()));
            if self.fail_start {
                return Err(Error::Refused("claude failed".into()));
            }
            Ok(())
        }
        fn agents(&self) -> Result<Vec<Agent>, Error> {
            if self.agents_fail {
                return Err(Error::Agents("boom".into()));
            }
            Ok(self.agents.clone())
        }
        fn stop_bg(&self, session_id: &str) -> Result<(), Error> {
            self.log(Call::StopBg(session_id.into()));
            Ok(())
        }
        fn terminate(&self, pid: u32) -> Result<(), Error> {
            self.log(Call::Terminate(pid));
            Ok(())
        }
        fn close_pane(&self, session_id: &str) -> Result<(), Error> {
            self.log(Call::ClosePane(session_id.into()));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::fake::{Call, Fake};
    use super::*;
    use crate::launch::fixture::Fx;
    use crate::project::{self, ItemKind};
    use crate::store::Touch;
    use crate::{crew, paths};

    /// A machine with project `demo` and one real directory as its repo.
    struct World {
        fx: Fx,
        repo: PathBuf,
        extra: HashMap<String, String>,
    }

    fn world() -> World {
        let fx = Fx::new();
        fx.init("demo");
        let repo = fx.tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, ItemKind::Repo, &repo.display().to_string())?;
                project::add(tx, ItemKind::Notes, "/vault/notes")
            })
            .unwrap();
        World {
            fx,
            repo,
            extra: HashMap::new(),
        }
    }

    impl World {
        fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
            let base = self.fx.env();
            let extra = self.extra.clone();
            move |k: &str| extra.get(k).cloned().or_else(|| base(k))
        }

        fn canon(&self) -> PathBuf {
            std::fs::canonicalize(&self.repo).unwrap()
        }

        fn spawn(&self, slug: &str, child: &str, brief: &str, run: &Fake) -> Result<String, Error> {
            self.spawn_in(
                slug,
                child,
                &self.repo.display().to_string(),
                brief,
                None,
                run,
            )
        }

        fn spawn_in(
            &self,
            slug: &str,
            child: &str,
            repo: &str,
            brief: &str,
            branch: Option<&str>,
            run: &Fake,
        ) -> Result<String, Error> {
            let env = self.env();
            let mut store = self.fx.store(slug);
            let req = SpawnReq {
                env: &env,
                slug: slug.into(),
                child: child.into(),
                repo: repo.into(),
                brief: brief.into(),
                branch: branch.map(str::to_string),
                session_id: format!("sid-{slug}-{child}"),
                base_env: vec![
                    ("PATH".into(), "/bin".into()),
                    ("CROOK_PROJECT".into(), "leak".into()),
                    ("CLAUDECODE".into(), "1".into()),
                ],
                crook_bin: "/abs/crook".into(),
            };
            spawn_child(&mut store, &req, run)
        }

        fn rows(&self, slug: &str) -> (Vec<String>, Vec<crew::Worktree>) {
            let s = self.fx.store(slug);
            let kids = s
                .read(|c| {
                    let mut st = c.prepare("SELECT child FROM children ORDER BY child")?;
                    Ok(st
                        .query_map([], |r| r.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?)
                })
                .unwrap();
            (kids, s.read(|c| crew::worktrees(c, None)).unwrap())
        }
    }

    fn flag(args: &[OsString], name: &str) -> Vec<String> {
        let a: Vec<String> = args
            .iter()
            .map(|x| x.to_string_lossy().into_owned())
            .collect();
        a.windows(2)
            .filter(|w| w[0] == name)
            .map(|w| w[1].clone())
            .collect()
    }

    fn refusal(r: Result<impl std::fmt::Debug, Error>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn spawn_checks() {
        let w = world();
        let run = Fake::default();
        // A repo not in the record.
        let other = w.fx.tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let e = refusal(w.spawn_in("demo", "c1", &other.display().to_string(), "b", None, &run));
        assert!(e.contains("not a repo of project demo"), "{e}");
        // A brief over 4000 characters, counted in characters not bytes.
        let e = refusal(w.spawn("demo", "c1", &"é".repeat(MAX_BRIEF_CHARS + 1), &run));
        assert!(e.contains("4000"), "{e}");
        assert!(
            w.spawn("demo", "c1", &"é".repeat(MAX_BRIEF_CHARS), &run)
                .is_ok()
        );
        // A slug-unsafe child id.
        for bad in ["C1", "a/b", "../x", "a.b", "a b", ""] {
            let e = refusal(w.spawn("demo", bad, "b", &run));
            assert!(e.contains("invalid child name"), "{bad:?}: {e}");
        }
        // Nothing else was recorded or run: only the one good spawn.
        assert_eq!(w.rows("demo").0, ["c1"]);
        assert_eq!(run.starts().len(), 1);
    }

    #[test]
    fn spawn_refuses_a_ninth_live_child() {
        let w = world();
        let run = Fake::default();
        for i in 0..MAX_LIVE_CHILDREN {
            w.spawn("demo", &format!("c{i}"), "b", &run).unwrap();
        }
        let e = refusal(w.spawn("demo", "c9", "b", &run));
        assert!(e.contains("8 live children"), "{e}");
        assert_eq!(run.starts().len(), 8);
        // An archived child no longer counts.
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| crew::archive(tx, "c0"))
            .unwrap();
        assert!(w.spawn("demo", "c9", "b", &run).is_ok());
    }

    #[test]
    fn spawn_rows_first() {
        for (fail_add, fail_start) in [(true, false), (false, true)] {
            let w = world();
            let run = Fake {
                fail_add,
                fail_start,
                ..Fake::default()
            };
            assert!(w.spawn("demo", "c1", "b", &run).is_err());
            let (kids, wts) = w.rows("demo");
            assert_eq!(kids, ["c1"], "the rows stay for open to reconcile");
            assert_eq!(wts.len(), 1);
            let held =
                w.fx.store("demo")
                    .read(|c| crew::held(c, "c1", "sid-demo-c1"))
                    .unwrap();
            assert!(
                held,
                "the first session id is held, so the hook park-denies it"
            );
            // Rows were written before the failing call ran.
            assert_eq!(run.calls().len(), if fail_add { 1 } else { 2 });
        }
    }

    #[test]
    fn spawn_worktree() {
        let w = world();
        let run = Fake::default();
        let sid = w.spawn("demo", "c1", "the brief", &run).unwrap();
        assert_eq!(sid, "sid-demo-c1");
        let path = w.canon().join(".worktrees/demo/c1");
        assert_eq!(
            run.calls()[0],
            Call::Add(w.canon(), path.clone(), "crook/demo/c1".into())
        );
        let s = w.fx.store("demo");
        let r = s.read(|c| crew::resumable(c, "c1")).unwrap();
        assert_eq!(r.worktree, path.display().to_string());
        let (_, wts) = w.rows("demo");
        assert_eq!(
            wts,
            [crew::Worktree {
                child: "c1".into(),
                repo: w.canon().display().to_string(),
                path: path.display().to_string(),
                branch: "crook/demo/c1".into(),
            }]
        );
        assert_eq!(run.starts()[0].1.cwd, path);
        // No link is made under the home.
        let home: Vec<_> = std::fs::read_dir(w.fx.home("demo"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(home, ["diary"]);
    }

    #[test]
    fn spawn_takes_a_given_branch() {
        let w = world();
        let run = Fake::default();
        w.spawn_in(
            "demo",
            "c1",
            &w.repo.display().to_string(),
            "b",
            Some("feat/x"),
            &run,
        )
        .unwrap();
        assert!(matches!(&run.calls()[0], Call::Add(_, _, b) if b == "feat/x"));
        let (_, wts) = w.rows("demo");
        assert_eq!(wts[0].branch, "feat/x");
    }

    #[test]
    fn a_branch_that_would_be_a_git_option_is_refused() {
        let w = world();
        let run = Fake::default();
        for bad in ["--detach", "-x", "a b", "a\nb", "", "x\ty"] {
            let e = refusal(w.spawn_in(
                "demo",
                "c1",
                &w.repo.display().to_string(),
                "b",
                Some(bad),
                &run,
            ));
            assert!(e.contains("branch"), "{bad:?}: {e}");
        }
        assert!(run.calls().is_empty());
        assert!(w.rows("demo").0.is_empty());
    }

    #[test]
    fn spawn_stores_the_repo_canonicalised() {
        // A repo reached through a symlink (as /tmp is /private/tmp on macOS)
        // is stored as the real path, which the hook's parked-in-repo check compares.
        let w = world();
        let link = w.fx.tmp.path().join("link");
        std::os::unix::fs::symlink(&w.repo, &link).unwrap();
        w.fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, ItemKind::Repo, &link.display().to_string())
            })
            .unwrap();
        let run = Fake::default();
        w.spawn_in("demo", "c1", &link.display().to_string(), "b", None, &run)
            .unwrap();
        let canon = w.canon().display().to_string();
        assert_ne!(canon, link.display().to_string());
        let stored =
            w.fx.store("demo")
                .read(|c| crew::child_repo(c, "c1"))
                .unwrap();
        assert_eq!(stored, canon);
        let (_, wts) = w.rows("demo");
        assert_eq!(wts[0].repo, canon);
        assert!(wts[0].path.starts_with(&canon));
        // The hook finds the parked child by the canonical spelling.
        w.fx.store("demo")
            .write(Touch::STATUS, |tx| {
                tx.execute("UPDATE status SET state = 'parked' WHERE child = 'c1'", [])?;
                Ok(())
            })
            .unwrap();
        let found =
            w.fx.store("demo")
                .read(|c| crew::parked_in_repo(c, &canon))
                .unwrap();
        assert_eq!(found.as_deref(), Some("c1"));
    }

    #[test]
    fn spawn_two_projects() {
        let w = world();
        w.fx.init("other");
        w.fx.store("other")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, ItemKind::Repo, &w.repo.display().to_string())
            })
            .unwrap();
        let run = Fake::default();
        w.spawn("demo", "c1", "b", &run).unwrap();
        w.spawn("other", "c1", "b", &run).unwrap();
        let adds: Vec<_> = run
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Add(_, p, b) => Some((p, b)),
                _ => None,
            })
            .collect();
        assert_eq!(adds.len(), 2);
        assert_ne!(adds[0].0, adds[1].0);
        assert_ne!(adds[0].1, adds[1].1);
        assert!(adds[0].0.ends_with(".worktrees/demo/c1"));
        assert!(adds[1].0.ends_with(".worktrees/other/c1"));
        assert_eq!(adds[1].1, "crook/other/c1");
    }

    fn start_of(herdr: bool) -> (World, Mode, StartCmd) {
        let mut w = world();
        if herdr {
            w.extra.insert("HERDR_ENV".into(), "1".into());
        }
        let run = Fake::default();
        w.spawn("demo", "c1", "b", &run).unwrap();
        let (mode, cmd) = run.starts().remove(0);
        (w, mode, cmd)
    }

    #[test]
    fn spawn_argv_child() {
        let (w, mode, cmd) = start_of(false);
        assert_eq!(mode, Mode::Bg);
        let a = &cmd.args;
        assert_eq!(flag(a, "-n"), ["crook-demo-c1"]);
        assert_eq!(flag(a, "--session-id"), ["sid-demo-c1"]);
        assert_eq!(flag(a, "--permission-mode"), ["manual"]);
        assert_eq!(
            flag(a, "--append-system-prompt"),
            [include_str!("../../plugin/prompts/child.md")]
        );
        // Every notes folder: the diary `init` added, then the one the test added.
        let diary = w.fx.home("demo").join("diary").display().to_string();
        assert_eq!(flag(a, "--add-dir"), [diary.as_str(), "/vault/notes"]);
        assert!(flag(a, "--settings")[0].ends_with("settings-child.json"));
        assert!(a.iter().any(|x| x == "--bg"));
        let prompt = "You are child c1 of crook-demo. Your project card is above.";
        assert_eq!(a.last().unwrap(), prompt);
        // A variadic --add-dir would swallow a bare prompt after its last value.
        let strs: Vec<String> = a.iter().map(|x| x.to_string_lossy().into_owned()).collect();
        for (i, s) in strs.iter().enumerate() {
            if s == "--add-dir" {
                let after = strs.get(i + 2).map(String::as_str);
                assert!(after.is_none_or(|n| n.starts_with('-')), "{strs:?}");
            }
        }
        let env = |k: &str| {
            cmd.env
                .iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(env("CROOK_ROLE").as_deref(), Some("child"));
        assert_eq!(env("CROOK_CHILD").as_deref(), Some("c1"));
        assert_eq!(env("CROOK_PROJECT").as_deref(), Some("demo"));
        assert_eq!(env("CROOK_BIN").as_deref(), Some("/abs/crook"));
        assert!(env("CLAUDECODE").is_none());
    }

    #[test]
    fn spawn_argv_in_herdr_has_no_bg() {
        let (_w, mode, cmd) = start_of(true);
        assert_eq!(mode, Mode::Herdr);
        assert!(!cmd.args.iter().any(|x| x == "--bg"));
        assert_eq!(flag(&cmd.args, "-n"), ["crook-demo-c1"]);
    }

    #[test]
    fn spawn_ensures_plugin() {
        let w = world();
        let dir = paths::plugin_dir(&w.env());
        assert!(!dir.exists());
        let run = Fake::default();
        w.spawn("demo", "c1", "b", &run).unwrap();
        assert!(dir.join("prompts/child.md").is_file());
        assert_eq!(
            flag(&run.starts()[0].1.args, "--plugin-dir"),
            [dir.display().to_string()]
        );
    }

    #[test]
    fn spawn_checks_the_mode_before_writing_rows() {
        struct NoHerdr(Fake);
        impl Launcher for NoHerdr {
            fn check(&self, mode: Mode) -> Result<(), Error> {
                match mode {
                    Mode::Herdr => Err(Error::Refused("herdr panes arrive with Task 20".into())),
                    Mode::Bg => Ok(()),
                }
            }
            fn worktree_add(&self, r: &Path, p: &Path, b: &str) -> Result<(), Error> {
                self.0.worktree_add(r, p, b)
            }
            fn worktree_remove(&self, r: &Path, p: &Path) -> Result<(), Error> {
                self.0.worktree_remove(r, p)
            }
            fn worktree_list(&self, r: &Path) -> Result<Vec<PathBuf>, Error> {
                self.0.worktree_list(r)
            }
            fn worktree_dirty(&self, p: &Path) -> Result<bool, Error> {
                self.0.worktree_dirty(p)
            }
            fn start(&self, m: Mode, c: &StartCmd) -> Result<(), Error> {
                self.0.start(m, c)
            }
            fn agents(&self) -> Result<Vec<Agent>, Error> {
                self.0.agents()
            }
            fn stop_bg(&self, s: &str) -> Result<(), Error> {
                self.0.stop_bg(s)
            }
            fn terminate(&self, p: u32) -> Result<(), Error> {
                self.0.terminate(p)
            }
            fn close_pane(&self, s: &str) -> Result<(), Error> {
                self.0.close_pane(s)
            }
        }
        let mut w = world();
        w.extra.insert("HERDR_ENV".into(), "1".into());
        let run = NoHerdr(Fake::default());
        let env = w.env();
        let mut store = w.fx.store("demo");
        let req = SpawnReq {
            env: &env,
            slug: "demo".into(),
            child: "c1".into(),
            repo: w.repo.display().to_string(),
            brief: "b".into(),
            branch: None,
            session_id: "s".into(),
            base_env: Vec::new(),
            crook_bin: "/abs/crook".into(),
        };
        let e = refusal(spawn_child(&mut store, &req, &run));
        assert!(e.contains("Task 20"), "{e}");
        assert!(w.rows("demo").0.is_empty());
        assert!(run.0.calls().is_empty());
    }

    // The `worktree` verb.

    fn spawned() -> (World, Fake) {
        let w = world();
        let run = Fake::default();
        w.spawn("demo", "c1", "b", &run).unwrap();
        (w, run)
    }

    fn wt_req(name: &str, branch: Option<&str>) -> WorktreeReq {
        WorktreeReq {
            child: "c1".into(),
            session_id: "sid-demo-c1".into(),
            name: name.into(),
            branch: branch.map(str::to_string),
        }
    }

    #[test]
    fn worktree_creates_and_records() {
        let (w, _) = spawned();
        let run = Fake::default();
        let mut s = w.fx.store("demo");
        let p = add_worktree(&mut s, &wt_req("fix", None), &run).unwrap();
        let want = w.canon().join(".worktrees/demo/c1.fix");
        assert_eq!(p, want);
        assert_eq!(
            run.calls(),
            [Call::Add(
                w.canon(),
                want.clone(),
                "crook/demo/c1.fix".into()
            )]
        );
        let (_, wts) = w.rows("demo");
        assert_eq!(wts.len(), 2);
        assert_eq!(wts[1].path, want.display().to_string());
        assert_eq!(wts[1].child, "c1");
        // A given branch.
        let p2 = add_worktree(&mut s, &wt_req("docs", Some("feat/docs")), &run).unwrap();
        assert!(p2.ends_with("c1.docs"));
        assert!(matches!(&run.calls()[1], Call::Add(_, _, b) if b == "feat/docs"));
        // The same name twice is refused, without a raw constraint error.
        let e = refusal(add_worktree(&mut s, &wt_req("fix", None), &run));
        assert!(e.contains("already") && !e.contains("UNIQUE"), "{e}");
        assert_eq!(run.calls().len(), 2);
    }

    #[test]
    fn worktree_row_is_written_before_the_launcher_runs() {
        let (w, _) = spawned();
        let run = Fake {
            fail_add: true,
            ..Fake::default()
        };
        let mut s = w.fx.store("demo");
        assert!(add_worktree(&mut s, &wt_req("fix", None), &run).is_err());
        assert_eq!(
            w.rows("demo").1.len(),
            2,
            "the row stays for open to reconcile"
        );
    }

    #[test]
    fn worktree_refuses_a_session_that_is_not_the_child() {
        let (w, _) = spawned();
        let run = Fake::default();
        let mut s = w.fx.store("demo");
        let mut req = wt_req("fix", None);
        req.session_id = "a-fork".into();
        let e = refusal(add_worktree(&mut s, &req, &run));
        assert!(e.contains("fork"), "{e}");
        assert!(run.calls().is_empty());
        // An unknown child.
        let mut req = wt_req("fix", None);
        req.child = "nobody".into();
        assert!(add_worktree(&mut s, &req, &run).is_err());
    }

    #[test]
    fn worktree_rejects_bad_name() {
        let (w, _) = spawned();
        let run = Fake::default();
        let mut s = w.fx.store("demo");
        for bad in ["Fix", "a/b", "../x", "a.b", "a b", ""] {
            let e = refusal(add_worktree(&mut s, &wt_req(bad, None), &run));
            assert!(e.contains("invalid worktree name"), "{bad:?}: {e}");
        }
        for bad in ["--detach", "a b"] {
            assert!(add_worktree(&mut s, &wt_req("ok", Some(bad)), &run).is_err());
        }
        assert!(run.calls().is_empty(), "nothing is run");
        assert_eq!(w.rows("demo").1.len(), 1, "nothing is written");
    }

    #[test]
    fn parse_worktree_list_takes_the_worktree_lines() {
        let out = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\n\
                   worktree /r/.worktrees/demo/c1\nHEAD def\nbranch refs/heads/crook/demo/c1\n\n\
                   worktree /r/detached dir\nHEAD 123\ndetached\n";
        assert_eq!(
            parse_worktree_list(out),
            [
                PathBuf::from("/r"),
                PathBuf::from("/r/.worktrees/demo/c1"),
                PathBuf::from("/r/detached dir")
            ]
        );
        assert!(parse_worktree_list("").is_empty());
    }

    // stop

    fn agent(sid: &str, pid: Option<u32>, kind: &str) -> Agent {
        Agent {
            session_id: sid.into(),
            pid,
            cwd: "/x".into(),
            kind: kind.into(),
            state: None,
        }
    }

    fn set_pid(w: &World, pid: u32) {
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                tx.execute(
                    "UPDATE children SET claude_pid = ?1 WHERE child = 'c1'",
                    [pid],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn archived(w: &World) -> bool {
        w.fx.store("demo")
            .read(|c| crew::resumable(c, "c1"))
            .unwrap()
            .archived
    }

    #[test]
    fn stop_kinds() {
        // A --bg child: claude stop <session id>.
        let (w, _) = spawned();
        let run = Fake {
            agents: vec![agent("sid-demo-c1", None, "background")],
            ..Fake::default()
        };
        let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
        assert_eq!(r, Stopped::Background);
        assert_eq!(run.calls(), [Call::StopBg("sid-demo-c1".into())]);
        assert!(archived(&w));

        // A herdr child whose pid claude agents confirms: SIGTERM, then its pane.
        let (w, _) = spawned();
        set_pid(&w, 4242);
        let run = Fake {
            agents: vec![agent("sid-demo-c1", Some(4242), "interactive")],
            ..Fake::default()
        };
        let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
        assert_eq!(r, Stopped::Pane);
        assert_eq!(
            run.calls(),
            [Call::Terminate(4242), Call::ClosePane("sid-demo-c1".into())]
        );

        // A stale pid: a different pid, or none, runs the session: only archived.
        for agents in [
            vec![agent("sid-demo-c1", Some(9999), "interactive")],
            vec![agent("sid-demo-c1", None, "interactive")],
            vec![agent("someone-else", Some(4242), "interactive")],
            Vec::new(),
        ] {
            let (w, _) = spawned();
            set_pid(&w, 4242);
            let run = Fake {
                agents,
                ..Fake::default()
            };
            let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
            assert!(matches!(r, Stopped::ArchivedOnly(_)), "{r:?}");
            assert!(
                run.calls().is_empty(),
                "no signal, no stop: {:?}",
                run.calls()
            );
            assert!(archived(&w));
        }

        // claude agents failing: only archived, never a blind signal.
        let (w, _) = spawned();
        set_pid(&w, 4242);
        let run = Fake {
            agents_fail: true,
            ..Fake::default()
        };
        let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
        assert!(matches!(r, Stopped::ArchivedOnly(m) if m.contains("boom")));
        assert!(run.calls().is_empty());
        assert!(archived(&w));
    }

    mod real {
        use std::os::unix::fs::PermissionsExt;
        use std::process::Command;

        use super::super::*;

        fn path_env() -> impl Fn(&str) -> Option<String> {
            |k: &str| (k == "PATH").then(|| std::env::var("PATH").unwrap_or_default())
        }

        fn git(dir: &Path, args: &[&str]) {
            let s = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
                .args(args)
                .output()
                .unwrap();
            assert!(s.status.success(), "git {args:?}: {s:?}");
        }

        #[test]
        fn git_worktrees_add_list_dirty_remove() {
            let tmp = tempfile::tempdir().unwrap();
            let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
            std::fs::create_dir_all(&repo).unwrap();
            git(&repo, &["init", "-q", "-b", "main"]);
            git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
            let env = path_env();
            let l = RealLauncher::new(&env);
            let wt = repo.join(".worktrees/demo/c1");
            l.worktree_add(&repo, &wt, "crook/demo/c1").unwrap();
            assert!(wt.join(".git").exists());
            assert_eq!(l.worktree_list(&repo).unwrap(), [repo.clone(), wt.clone()]);
            assert!(!l.worktree_dirty(&wt).unwrap());
            std::fs::write(wt.join("new.txt"), "x").unwrap();
            assert!(l.worktree_dirty(&wt).unwrap());
            // A branch that already exists is git's refusal, with its message.
            let e = l
                .worktree_add(&repo, &repo.join(".worktrees/demo/c2"), "crook/demo/c1")
                .unwrap_err()
                .to_string();
            assert!(e.contains("git") && e.contains("already"), "{e}");
            std::fs::remove_file(wt.join("new.txt")).unwrap();
            l.worktree_remove(&repo, &wt).unwrap();
            assert!(!wt.exists());
            assert_eq!(l.worktree_list(&repo).unwrap(), [repo]);
        }

        fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
            let p = dir.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            p
        }

        #[test]
        fn start_runs_claude_on_exactly_the_given_environment_and_cwd() {
            let tmp = tempfile::tempdir().unwrap();
            let out = tmp.path().join("out");
            let claude = script(
                tmp.path(),
                "claude",
                &format!(
                    "echo \"$PWD|$CROOK_CHILD|${{LEAK-unset}}|$*\" > {}",
                    out.display()
                ),
            );
            let env = path_env();
            let cmd = StartCmd {
                program: claude,
                args: vec!["--bg".into(), "--".into(), "prompt words".into()],
                env: vec![("CROOK_CHILD".into(), "c1".into())],
                cwd: std::fs::canonicalize(tmp.path()).unwrap(),
            };
            RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap();
            let got = std::fs::read_to_string(out).unwrap();
            let want = format!("{}|c1|unset|--bg -- prompt words\n", cmd.cwd.display());
            assert_eq!(got, want);
        }

        #[test]
        fn start_does_not_wait_for_a_background_session_holding_the_pipes() {
            let tmp = tempfile::tempdir().unwrap();
            let claude = script(tmp.path(), "claude", "sleep 8 &\necho started");
            let env = path_env();
            let cmd = StartCmd {
                program: claude,
                args: Vec::new(),
                env: vec![("PATH".into(), "/bin:/usr/bin".into())],
                cwd: tmp.path().into(),
            };
            let t = Instant::now();
            RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap();
            assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
        }

        #[test]
        fn start_failure_carries_stderr() {
            let tmp = tempfile::tempdir().unwrap();
            let claude = script(
                tmp.path(),
                "claude",
                "echo 'Workspace not trusted' >&2\nexit 1",
            );
            let env = path_env();
            let cmd = StartCmd {
                program: claude,
                args: Vec::new(),
                env: Vec::new(),
                cwd: tmp.path().into(),
            };
            let e = RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap_err();
            assert!(e.to_string().contains("Workspace not trusted"), "{e}");
        }

        #[test]
        fn herdr_is_not_yet_startable() {
            let env = path_env();
            let l = RealLauncher::new(&env);
            assert!(l.check(Mode::Bg).is_ok());
            assert!(
                l.check(Mode::Herdr)
                    .unwrap_err()
                    .to_string()
                    .contains("Task 20")
            );
        }
    }
}
