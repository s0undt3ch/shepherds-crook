//! Building a `claude` launch: argv, environment and reconcile.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::agents::{self, Agent};
use crate::gate::{Identity, Role};
use crate::hook::sessionstart::is_claude;
use crate::{Env, Error, crew, paths, settings};

pub mod plugin_dir;
pub mod preflight;

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

/// The environment a crook session starts with: `base` without any `CROOK_*`,
/// `CLAUDE_CODE_MESSAGING_*` or `CLAUDECODE`, then this session's identity.
pub fn env(
    base: impl Iterator<Item = (OsString, OsString)>,
    id: &Identity,
    crook_bin: &Path,
) -> Vec<(OsString, OsString)> {
    let dropped = |k: &OsString| {
        let k = k.to_string_lossy();
        k.starts_with("CROOK_") || k.starts_with("CLAUDE_CODE_MESSAGING_") || k == "CLAUDECODE"
    };
    let mut out: Vec<(OsString, OsString)> = base.filter(|(k, _)| !dropped(k)).collect();
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

/// `reconcile_with`, with the live check `crook hook SessionStart` uses.
pub fn reconcile(c: &Connection, agents: &[Agent]) -> Result<Vec<String>, Error> {
    reconcile_with(c, agents, &is_claude)
}

/// What reconcile found, read-only: lines to report, and the children whose
/// recorded claude pid is no longer a live claude.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Findings {
    pub lines: Vec<String>,
    pub stale_pids: Vec<String>,
}

/// Reads what a crash between a spawn's rows and its launch can leave:
/// - a child row whose worktree is gone from disk;
/// - a running session in a child's worktree that no row of that child knows;
/// - a recorded claude pid that is not a live claude (listed, not yet cleared).
pub fn findings(
    c: &Connection,
    agents: &[Agent],
    live: &dyn Fn(u32) -> bool,
) -> Result<Findings, Error> {
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
    for (child, worktree, session_id, pid) in &kids {
        if !Path::new(worktree).exists() {
            out.lines.push(format!(
                "child {child}: worktree {worktree} is missing on disk"
            ));
        }
        let stale = pid
            .and_then(|p| u32::try_from(p).ok())
            .is_some_and(|p| !live(p));
        if stale {
            out.stale_pids.push(child.clone());
        }
        for a in agents {
            let known = a.session_id == *session_id
                || crew::is_fork(c, child, &a.session_id)?
                || crew::held(c, child, &a.session_id)?;
            if !known && agents::cwd_inside(&a.cwd, worktree) {
                out.lines.push(format!(
                    "child {child}: session {} is running in {} but no row of {child} knows it",
                    a.session_id, a.cwd
                ));
            }
        }
    }
    Ok(out)
}

/// `findings`, then forgets the stale pids on `c` (a transaction, for the
/// caller to commit). `open` reads first and writes only when this would.
pub fn reconcile_with(
    c: &Connection,
    agents: &[Agent],
    live: &dyn Fn(u32) -> bool,
) -> Result<Vec<String>, Error> {
    let f = findings(c, agents, live)?;
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
    pub managed_dirs: Vec<PathBuf>,
}

/// Runs `f` against the real machine: this process's environment and binary,
/// `kill(pid, 0)` plus `ps` for liveness, `claude --version` and
/// `claude agents --json`.
pub fn with_real_host<R>(env: Env, f: impl FnOnce(&Host) -> R) -> Result<R, Error> {
    let version = || agents::claude_version(env);
    let list = || agents::list(env);
    let new_id = || uuid::Uuid::new_v4().to_string();
    let host = Host {
        base_env: std::env::vars_os().collect(),
        crook_bin: crook_bin()?,
        live: &is_claude,
        new_id: &new_id,
        version: &version,
        agents: &list,
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
mod tests {
    use super::*;
    use crate::gate::Role;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn opts(session: Session) -> LaunchOpts {
        LaunchOpts {
            name: "crook-demo".into(),
            session,
            permission_mode: "manual".into(),
            plugin_dir: "/x/crook-plugin/0.1.0-abc".into(),
            system_prompt: "PROMPT".into(),
            settings: "/s/crook/demo/settings-orchestrator.json".into(),
            add_dirs: vec!["/notes".into(), "/repo".into()],
        }
    }

    #[test]
    fn argv_orchestrator() {
        assert_eq!(
            argv(&opts(Session::New("uuid-1".into()))),
            [
                "-n",
                "crook-demo",
                "--session-id",
                "uuid-1",
                "--permission-mode",
                "manual",
                "--plugin-dir",
                "/x/crook-plugin/0.1.0-abc",
                "--append-system-prompt",
                "PROMPT",
                "--settings",
                "/s/crook/demo/settings-orchestrator.json",
                "--add-dir",
                "/notes",
                "--add-dir",
                "/repo",
            ]
            .map(os)
        );
    }

    #[test]
    fn argv_resume_swaps_only_the_session_flag() {
        let a = argv(&opts(Session::Resume("old".into())));
        let b = argv(&opts(Session::New("uuid-1".into())));
        assert_eq!(a[2], os("--resume"));
        assert_eq!(a[3], os("old"));
        assert_eq!(a[4..], b[4..]);
    }

    fn base() -> Vec<(OsString, OsString)> {
        [
            ("PATH", "/bin"),
            ("HOME", "/h"),
            ("CROOK_CHILD", "c9"),
            ("CROOK_PROJECT", "other"),
            ("CROOK_ROLE", "child"),
            ("CROOK_BIN", "/old/crook"),
            ("CROOK_ANYTHING", "1"),
            ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/s"),
            ("CLAUDE_CODE_MESSAGING_TOKEN", "t"),
            ("CLAUDECODE", "1"),
            ("ANTHROPIC_MODEL", "haiku"),
        ]
        .map(|(k, v)| (os(k), os(v)))
        .to_vec()
    }

    fn id(role: Role, child: Option<&str>) -> Identity {
        Identity {
            slug: "demo".into(),
            role,
            child: child.map(str::to_string),
        }
    }

    fn get<'a>(v: &'a [(OsString, OsString)], k: &str) -> Option<&'a OsString> {
        v.iter().find(|(a, _)| a == k).map(|(_, b)| b)
    }

    #[test]
    fn env_scrub() {
        let out = env(
            base().into_iter(),
            &id(Role::Orchestrator, None),
            Path::new("/new/crook"),
        );
        assert_eq!(get(&out, "CROOK_PROJECT"), Some(&os("demo")));
        assert_eq!(get(&out, "CROOK_ROLE"), Some(&os("orchestrator")));
        assert_eq!(get(&out, "CROOK_BIN"), Some(&os("/new/crook")));
        for gone in [
            "CROOK_CHILD",
            "CROOK_ANYTHING",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN",
            "CLAUDECODE",
        ] {
            assert!(get(&out, gone).is_none(), "{gone}");
        }
        // Everything else passes through, and no key is set twice.
        assert_eq!(get(&out, "ANTHROPIC_MODEL"), Some(&os("haiku")));
        assert_eq!(get(&out, "PATH"), Some(&os("/bin")));
        let mut keys: Vec<_> = out.iter().map(|(k, _)| k.clone()).collect();
        keys.sort();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }

    #[test]
    fn env_child_carries_its_id() {
        let out = env(
            base().into_iter(),
            &id(Role::Child, Some("c1")),
            Path::new("/new/crook"),
        );
        assert_eq!(get(&out, "CROOK_ROLE"), Some(&os("child")));
        assert_eq!(get(&out, "CROOK_CHILD"), Some(&os("c1")));
    }

    mod reconcile {
        use super::super::*;
        use crate::store::{Mode, Store, Touch};

        fn store() -> (tempfile::TempDir, Store) {
            let tmp = tempfile::tempdir().unwrap();
            let x = tmp.path().display().to_string();
            let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
            crate::project::init(&env, "p").unwrap();
            let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
            (tmp, s)
        }

        fn child(s: &mut Store, name: &str, worktree: &str, sid: &str, pid: Option<i64>) {
            s.write(Touch::CHILDREN, |tx| {
                tx.execute(
                    "INSERT INTO children(child, repo, worktree, branch, brief, session_id,
                                          claude_pid, created_at)
                     VALUES (?1, '/r', ?2, 'b', 'brief', ?3, ?4, 0)",
                    (name, worktree, sid, pid),
                )?;
                Ok(())
            })
            .unwrap();
        }

        fn agent(sid: &str, cwd: &str) -> Agent {
            Agent {
                session_id: sid.into(),
                pid: Some(1),
                cwd: cwd.into(),
                kind: "interactive".into(),
                state: None,
            }
        }

        #[test]
        fn missing_worktree_is_reported() {
            let (tmp, mut s) = store();
            let here = tmp.path().display().to_string();
            child(&mut s, "ok", &here, "s1", None);
            child(&mut s, "gone", "/no/such/worktree", "s2", None);
            let lines = s
                .write(Touch::CHILDREN, |tx| reconcile_with(tx, &[], &|_| false))
                .unwrap();
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].contains("gone") && lines[0].contains("/no/such/worktree"));
        }

        #[test]
        fn archived_child_is_not_reported() {
            let (_t, mut s) = store();
            child(&mut s, "gone", "/no/such/worktree", "s2", None);
            s.write(Touch::CHILDREN, |tx| crate::crew::archive(tx, "gone"))
                .unwrap();
            let lines = s
                .write(Touch::CHILDREN, |tx| reconcile_with(tx, &[], &|_| false))
                .unwrap();
            assert!(lines.is_empty(), "{lines:?}");
        }

        #[test]
        fn unrecorded_running_session_is_flagged() {
            let (tmp, mut s) = store();
            let wt = tmp.path().display().to_string();
            child(&mut s, "c1", &wt, "known", None);
            let agents = [
                agent("known", &wt),
                agent("stranger", &format!("{wt}/sub")),
                agent("elsewhere", "/somewhere/else"),
            ];
            let lines = s
                .write(Touch::CHILDREN, |tx| {
                    reconcile_with(tx, &agents, &|_| false)
                })
                .unwrap();
            assert_eq!(lines.len(), 1, "{lines:?}");
            assert!(lines[0].contains("stranger") && lines[0].contains("c1"));
        }

        #[test]
        fn a_stale_pid_is_cleared_and_a_live_one_kept() {
            let (tmp, mut s) = store();
            let wt = tmp.path().display().to_string();
            child(&mut s, "dead", &wt, "s1", Some(111));
            child(&mut s, "alive", &wt, "s2", Some(222));
            s.write(Touch::CHILDREN, |tx| reconcile_with(tx, &[], &|p| p == 222))
                .unwrap();
            let pid = |c: &str| s.read(|conn| crate::crew::claude_pid(conn, c)).unwrap();
            assert_eq!(pid("dead"), None);
            assert_eq!(pid("alive"), Some(222));
        }
    }
}

#[cfg(test)]
pub(crate) mod fixture {
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::store::{Mode, Store, Touch};

    /// A temp machine: XDG dirs, a fake `claude` on `PATH`, one initialised project.
    pub struct Fx {
        pub tmp: tempfile::TempDir,
        vars: HashMap<String, String>,
    }

    impl Fx {
        pub fn new() -> Fx {
            let tmp = tempfile::tempdir().unwrap();
            let d = tmp.path().display().to_string();
            let bin = tmp.path().join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let claude = bin.join("claude");
            std::fs::write(&claude, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
            let vars = HashMap::from([
                ("HOME".to_string(), format!("{d}/h")),
                ("XDG_DATA_HOME".to_string(), format!("{d}/data")),
                ("XDG_STATE_HOME".to_string(), format!("{d}/state")),
                ("XDG_CONFIG_HOME".to_string(), format!("{d}/cfg")),
                ("CLAUDE_CONFIG_DIR".to_string(), format!("{d}/claude")),
                ("PATH".to_string(), bin.display().to_string()),
            ]);
            Fx { tmp, vars }
        }

        pub fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
            let v = self.vars.clone();
            move |k: &str| v.get(k).cloned()
        }

        pub fn claude(&self) -> PathBuf {
            self.tmp.path().join("bin/claude")
        }

        pub fn init(&self, slug: &str) {
            crate::cli::init::run(&self.env(), slug).unwrap();
        }

        pub fn home(&self, slug: &str) -> PathBuf {
            self.tmp.path().join("data/crook").join(slug)
        }

        pub fn store(&self, slug: &str) -> Store {
            Store::open(&self.env(), slug, Mode::ReadWrite).unwrap()
        }

        pub fn add_child(
            &self,
            slug: &str,
            child: &str,
            worktree: &Path,
            sid: &str,
            pid: Option<u32>,
        ) {
            self.store(slug)
                .write(Touch::CHILDREN, |tx| {
                    crew::add_child(
                        tx,
                        &crew::Child {
                            child: child.into(),
                            repo: "/r".into(),
                            worktree: worktree.display().to_string(),
                            branch: "b".into(),
                            brief: "x".into(),
                            session_id: sid.into(),
                        },
                    )?;
                    if let Some(p) = pid {
                        tx.execute(
                            "UPDATE children SET claude_pid = ?2 WHERE child = ?1",
                            (child, p),
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
        }

        pub fn host<'a>(
            &self,
            live: &'a dyn Fn(u32) -> bool,
            agents: &'a dyn Fn() -> Result<Vec<Agent>, Error>,
            version: &'a dyn Fn() -> Result<String, Error>,
            new_id: &'a dyn Fn() -> String,
        ) -> Host<'a> {
            Host {
                base_env: vec![
                    ("PATH".into(), self.vars["PATH"].clone().into()),
                    ("CROOK_CHILD".into(), "leak".into()),
                    ("CLAUDECODE".into(), "1".into()),
                    ("CLAUDE_CODE_MESSAGING_SOCKET".into(), "/x".into()),
                ],
                crook_bin: "/abs/crook".into(),
                live,
                new_id,
                version,
                agents,
                managed_dirs: Vec::new(),
            }
        }
    }
}
