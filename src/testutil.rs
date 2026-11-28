//! Helpers shared by unit tests.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use crate::ctx::Clock;
use crate::run::{Cmd, Output, RealRunner, Runner};
use crate::{Ctx, Error};

/// An injected environment holding exactly `pairs`.
pub fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |key| map.get(key).cloned()
}

/// The environment every real `git` in a test runs with: the caller's `PATH` and nothing of the developer's
/// home or git config (build log D118).
// Tests run outside any Ctx; this reads only the caller's PATH and a temp folder name.
#[allow(clippy::disallowed_methods)]
pub fn git_env() -> Vec<(String, String)> {
    let home = std::env::temp_dir().join("crook-tests-no-home");
    vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), home.display().to_string()),
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ]
}

/// A writer whose contents the test can read at any moment, even while a command holds it.
#[derive(Clone, Default)]
pub struct SharedBuf(pub Rc<RefCell<Vec<u8>>>);

impl SharedBuf {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 2026-11-27 12:00 UTC.
pub struct FixedClock;

pub const FIXED_DATE: &str = "2026-11-27";

impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_780_800)
    }
}

/// Runs real `git` (temp repositories are fast and exact), records `herdr-app` calls and answers
/// them, and reports every other program as missing, so `date` falls back to the fixed clock.
#[derive(Default)]
pub struct Router {
    herdr: RefCell<Vec<Cmd>>,
    herdr_failures: Cell<usize>,
    herdr_missing: Cell<bool>,
    programs: RefCell<Vec<String>>,
    /// The fixture's environment, which a script run for real gets when the call names none.
    vars: Option<Rc<RefCell<HashMap<String, String>>>>,
}

impl Router {
    pub fn herdr_calls(&self) -> Vec<Cmd> {
        self.herdr.borrow().clone()
    }

    /// The next `times` herdr-app calls exit 1.
    pub fn fail_herdr(&self, times: usize) {
        self.herdr_failures.set(times);
    }

    pub fn remove_herdr(&self) {
        self.herdr_missing.set(true);
    }

    /// Every program asked for, in order.
    pub fn programs(&self) -> Vec<String> {
        self.programs.borrow().clone()
    }
}

impl Runner for Router {
    fn run(&self, cmd: Cmd, timeout: Duration) -> Result<Output, Error> {
        self.programs.borrow_mut().push(cmd.program.clone());
        match cmd.program.as_str() {
            "git" => {
                let mut cmd = cmd;
                if cmd.env.is_none() {
                    cmd = cmd.env(git_env());
                }
                RealRunner.run(cmd, timeout)
            }
            // A test's own script, such as a stand-in login shell.
            program if program.starts_with('/') => {
                let mut cmd = cmd;
                if cmd.env.is_none() {
                    let vars = self.vars.as_ref().map(|v| v.borrow().clone());
                    cmd.env = Some(
                        vars.unwrap_or_default()
                            .into_iter()
                            .map(|(k, v)| (k.into(), v.into()))
                            .collect(),
                    );
                }
                RealRunner.run(cmd, timeout)
            }
            "herdr-app" => {
                self.herdr.borrow_mut().push(cmd);
                if self.herdr_missing.get() {
                    return Err(Error::Spawn {
                        program: "herdr-app".to_string(),
                        source: std::io::Error::other("not found"),
                    });
                }
                if self.herdr_failures.get() > 0 {
                    self.herdr_failures.set(self.herdr_failures.get() - 1);
                    return Ok(Output {
                        code: Some(1),
                        stdout: String::new(),
                        stderr: "no such space".to_string(),
                    });
                }
                Ok(Output::success("{\"ok\": true}\n"))
            }
            other => Err(Error::Spawn {
                program: other.to_string(),
                source: std::io::Error::other("not found"),
            }),
        }
    }
}

type EnvFn = dyn Fn(&str) -> Option<String>;
type VarsFn = dyn Fn() -> Vec<(OsString, OsString)>;
type ExeFn = dyn Fn() -> std::io::Result<PathBuf>;
type IdFn = dyn Fn() -> String;

/// A temp XDG tree and everything a `Ctx` needs, owned in one place so a test borrows a `Ctx`
/// from it.
pub struct Fixture {
    pub dir: tempfile::TempDir,
    vars: Rc<RefCell<std::collections::HashMap<String, String>>>,
    pub runner: Router,
    clock: FixedClock,
    env: Box<EnvFn>,
    all_vars: Box<VarsFn>,
    exe: Box<ExeFn>,
    ids: Box<IdFn>,
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Fixture {
    #[allow(clippy::expect_used)]
    pub fn new() -> Fixture {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let exe_path = bin.join("crook");
        std::fs::write(&exe_path, "").expect("exe");
        let p = |name: &str| root.join(name).display().to_string();
        let vars: std::collections::HashMap<String, String> = [
            ("HOME", p("home")),
            ("XDG_DATA_HOME", p("data")),
            ("XDG_CONFIG_HOME", p("config")),
            ("XDG_STATE_HOME", p("state")),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        let vars = Rc::new(RefCell::new(vars));
        let for_env = Rc::clone(&vars);
        let for_vars = Rc::clone(&vars);
        let counter = Cell::new(0u32);
        Fixture {
            dir,
            vars: Rc::clone(&vars),
            runner: Router {
                vars: Some(Rc::clone(&vars)),
                ..Router::default()
            },
            clock: FixedClock,
            env: Box::new(move |k| for_env.borrow().get(k).cloned()),
            all_vars: Box::new(move || {
                for_vars
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.into(), v.into()))
                    .collect()
            }),
            exe: Box::new(move || Ok(exe_path.clone())),
            ids: Box::new(move || {
                counter.set(counter.get() + 1);
                format!("00000000-0000-4000-8000-{:012}", counter.get())
            }),
        }
    }

    pub fn set_env(&self, key: &str, value: &str) {
        self.vars
            .borrow_mut()
            .insert(key.to_string(), value.to_string());
    }

    #[allow(clippy::expect_used)]
    pub fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().expect("canonical")
    }

    pub fn flocks_root(&self) -> PathBuf {
        self.root().join("data/crook/flocks")
    }

    /// Writes `config.toml` under the fixture's config home.
    #[allow(clippy::expect_used)]
    pub fn write_config(&self, text: &str) {
        let dir = self.root().join("config/crook");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("config.toml"), text).expect("write");
    }

    /// A plugin folder whose `plugin.json` says `version`, which `$CLAUDE_PLUGIN_ROOT` points at, as
    /// in a session's hook. Calling it again moves the version, as an upgrade moves the `current` link.
    #[allow(clippy::expect_used)]
    pub fn set_plugin_version(&self, version: &str) {
        let plugin = self.root().join("plugin-root");
        std::fs::create_dir_all(plugin.join(".claude-plugin")).expect("mkdir");
        std::fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!("{{\"name\": \"crook\", \"version\": \"{version}\"}}"),
        )
        .expect("plugin.json");
        self.set_env("CLAUDE_PLUGIN_ROOT", &plugin.display().to_string());
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx {
            env: &*self.env,
            vars: &*self.all_vars,
            runner: &self.runner,
            clock: &self.clock,
            current_exe: &*self.exe,
            new_id: &*self.ids,
        }
    }
}

pub const COORD_SID: &str = "00000000-0000-4000-8000-0000000000aa";

/// A flock as `new` leaves it, without starting anything: folder, roster, no children.
#[allow(clippy::expect_used)]
pub fn make_flock(fx: &Fixture, slug: &str) -> crate::flock::Flock {
    let ctx = fx.ctx();
    let config = crate::config::load(ctx.env).expect("config");
    let made = crate::flock::create(ctx.env, &config, FIXED_DATE, slug).expect("creates");
    let roster = crate::files::roster::Roster::new(slug, COORD_SID, &made.folder);
    crate::files::roster::create(&made.folder, &roster).expect("roster");
    made
}

/// Starts the flock's coordinator in the fake core, as `crook open` would, and returns its address.
#[allow(clippy::expect_used)]
pub fn start_coordinator(hc: &crate::hc::fake::FakeHc, flock: &crate::flock::Flock) -> String {
    use crate::hc::{Hc, Parent, RunSpec};
    hc.run(&RunSpec {
        name: flock.slug.clone(),
        cwd: flock.folder.clone(),
        labels: vec![(crate::names::LABEL_FLOCK.to_string(), flock.slug.clone())],
        parent: Parent::None,
        argv: vec!["claude".to_string()],
    })
    .expect("coordinator")
    .address
}

#[allow(clippy::expect_used)]
pub fn write_brief(flock: &crate::flock::Flock, name: &str) {
    std::fs::write(
        flock.briefs_dir().join(format!("{name}.md")),
        "# Goal\n\nDo it.\n",
    )
    .expect("brief");
}

#[allow(clippy::expect_used)]
pub fn load_roster(flock: &crate::flock::Flock) -> crate::files::roster::Roster {
    crate::files::roster::load(&flock.folder).expect("roster loads")
}

/// A git repository with one commit, whose own config overrides the developer's.
#[allow(clippy::expect_used)]
pub fn init_repo(parent: &std::path::Path) -> PathBuf {
    let root = parent.join("repo");
    std::fs::create_dir_all(&root).expect("mkdir");
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&root)
            .env_clear()
            .envs(git_env())
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    let hooks = root.join(".git/no-hooks");
    std::fs::create_dir(&hooks).expect("mkdir");
    let excludes = root.join(".git/no-excludes");
    std::fs::write(&excludes, "").expect("write");
    for (k, v) in [
        ("user.name", "Crook Test"),
        ("user.email", "crook@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", hooks.to_str().expect("utf8")),
        ("core.excludesFile", excludes.to_str().expect("utf8")),
    ] {
        git(&["config", k, v]);
    }
    std::fs::write(root.join("a.txt"), "one\n").expect("write");
    git(&["add", "a.txt"]);
    git(&["commit", "-q", "-m", "one"]);
    root
}

type RunHook<'a> = Box<dyn Fn(&crate::hc::RunSpec) + 'a>;
type KillHook<'a> = Box<dyn Fn(&str) + 'a>;
type SendHook<'a> = Box<dyn Fn(&str, &str) + 'a>;

/// The fake core with a look at the moment of a `run` or a `kill`, to prove what had happened by
/// then (the roster, the output so far).
pub struct HookedHc<'a> {
    pub inner: &'a crate::hc::fake::FakeHc,
    pub on_run: Option<RunHook<'a>>,
    pub on_kill: Option<KillHook<'a>>,
    pub on_send: Option<SendHook<'a>>,
}

impl<'a> HookedHc<'a> {
    pub fn new(inner: &'a crate::hc::fake::FakeHc) -> Self {
        HookedHc {
            inner,
            on_run: None,
            on_kill: None,
            on_send: None,
        }
    }
}

impl crate::hc::Hc for HookedHc<'_> {
    fn version(&self) -> Result<String, Error> {
        self.inner.version()
    }
    fn schema(&self) -> Result<crate::hc::Schema, Error> {
        self.inner.schema()
    }
    fn ls(&self, all: bool) -> Result<Vec<crate::hc::Terminal>, Error> {
        self.inner.ls(all)
    }
    fn ls_by_label(
        &self,
        all: bool,
        labels: &[(String, String)],
    ) -> Result<Vec<crate::hc::Terminal>, Error> {
        self.inner.ls_by_label(all, labels)
    }
    fn get(&self, id: &str) -> Result<Option<crate::hc::Terminal>, Error> {
        self.inner.get(id)
    }
    fn run(&self, spec: &crate::hc::RunSpec) -> Result<crate::hc::Terminal, Error> {
        if let Some(hook) = &self.on_run {
            hook(spec);
        }
        self.inner.run(spec)
    }
    fn send(&self, to: &str, text: &str) -> Result<crate::hc::SendOutcome, Error> {
        if let Some(hook) = &self.on_send {
            hook(to, text);
        }
        self.inner.send(to, text)
    }
    fn kill(&self, id: &str) -> Result<crate::hc::KillOutcome, Error> {
        if let Some(hook) = &self.on_kill {
            hook(id);
        }
        self.inner.kill(id)
    }
    fn server_start(&self, env: &[(OsString, OsString)]) -> Result<(), Error> {
        self.inner.server_start(env)
    }
}

/// Both output streams of a command, readable afterwards (and, through clones, meanwhile).
#[derive(Default)]
pub struct Buffers {
    pub out: SharedBuf,
    pub err: SharedBuf,
}

impl Buffers {
    pub fn io(&mut self) -> crate::cmd::Io<'_> {
        crate::cmd::Io {
            out: &mut self.out,
            err: &mut self.err,
        }
    }
}

/// What a hook printed: stdout and stderr.
pub struct HookRun {
    pub stdout: String,
    pub stderr: String,
}

impl HookRun {
    /// The `additionalContext` of the one JSON line on stdout, or `None` when it printed nothing.
    #[allow(clippy::expect_used)]
    pub fn context(&self) -> Option<String> {
        if self.stdout.is_empty() {
            return None;
        }
        let doc: serde_json::Value =
            serde_json::from_str(self.stdout.trim()).expect("one JSON line");
        Some(
            doc["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .expect("additionalContext")
                .to_string(),
        )
    }
}

/// Runs `crook hook <event>` in the terminal `terminal` (None: no `$HC_TERMINAL_ID`), the core up.
pub fn run_hook(
    fx: &Fixture,
    hc: &dyn crate::hc::Hc,
    event: &str,
    terminal: Option<&str>,
    input: &str,
) -> HookRun {
    if let Some(t) = terminal {
        fx.set_env("HC_TERMINAL_ID", t);
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    crate::hook::run(
        &fx.ctx(),
        hc,
        &|| true,
        event,
        &mut input.as_bytes(),
        &mut out,
        &mut err,
    );
    HookRun {
        stdout: String::from_utf8_lossy(&out).into_owned(),
        stderr: String::from_utf8_lossy(&err).into_owned(),
    }
}

pub const SESSION: &str = "11111111-aaaa-4bbb-8ccc-222222222222";

pub fn hook_input(session: &str) -> String {
    serde_json::json!({"session_id": session, "hook_event_name": "x", "source": "startup", "cwd": "/x"})
        .to_string()
}

/// An install folder as a release archive lays it out: `plugin/` with a version and a `mod/` that
/// holds files setup must install and files it must leave out.
#[allow(clippy::expect_used)]
pub fn fake_install(fx: &Fixture, version: &str) -> crate::install::Install {
    let root = fx.root().join("install");
    let plugin = root.join("plugin/.claude-plugin");
    std::fs::create_dir_all(&plugin).expect("mkdir");
    std::fs::write(
        plugin.join("plugin.json"),
        format!("{{\"name\": \"crook\", \"version\": \"{version}\"}}"),
    )
    .expect("plugin.json");
    let m = root.join("mod");
    std::fs::create_dir_all(&m).expect("mkdir");
    for (name, text) in [
        ("mod.json", "{}"),
        ("main.js", "main"),
        ("lib.js", "lib"),
        ("RECIPE.md", "recipe"),
        ("preview.png", "png"),
        ("jsconfig.json", "{}"),
        ("gpui-kit.d.ts", "types"),
        (".hidden", "dot"),
    ] {
        std::fs::write(m.join(name), text).expect("write");
    }
    crate::install::Install::at(root)
}
