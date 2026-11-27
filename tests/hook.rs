//! `crook hook <event>` as the binary runs it: a fake `hc` on a cleared `PATH`, a real socket
//! standing in for the core, temp XDG folders. Whatever goes wrong, the hook exits 0, prints nothing
//! on stdout and says why on stderr (ADR 0020). No real hc core, claude or herdr-app is started.
#![allow(clippy::expect_used)]
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crook::files::roster::{self, Roster};

fn script(path: &Path, text: &str) {
    std::fs::write(path, text).expect("write");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// Answers `get` with a coordinator of flock `demo` that runs Claude Code, and logs every call. With
/// `$FAKE_HC_HANG` set it sleeps instead, as an hc that never answers.
const FAKE_HC: &str = r#"#!/bin/sh
echo "$*" >> "$FAKE_HC_LOG"
if [ -n "$FAKE_HC_HANG" ]; then /bin/sleep 60; fi
case "$1" in
  get) echo '{"address":"t1","name":"demo","state":"live","labels":{"crook.flock":"demo"},"status":{"app":"claude-code","state":"idle"}}' ;;
  *) exit 2 ;;
esac
"#;

struct Sandbox {
    dir: tempfile::TempDir,
    _listener: Option<UnixListener>,
}

impl Sandbox {
    fn new(core: bool) -> Sandbox {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().canonicalize().expect("canonical");
        for d in ["bin", "home", "data", "config", "state"] {
            std::fs::create_dir_all(root.join(d)).expect("mkdir");
        }
        script(&root.join("bin/hc"), FAKE_HC);
        let listener = core.then(|| UnixListener::bind(root.join("c.sock")).expect("bind"));
        Sandbox {
            dir,
            _listener: listener,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().expect("canonical")
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root().join("hc.log")).unwrap_or_default()
    }

    /// A flock `demo` with one result nobody has seen, and a plugin folder for the version record.
    fn flock(&self) -> PathBuf {
        let folder = self.root().join("data/crook/flocks/2026-11-27-demo");
        std::fs::create_dir_all(folder.join("results")).expect("mkdir");
        std::fs::create_dir_all(folder.join("briefs")).expect("mkdir");
        roster::create(&folder, &Roster::new("demo", "sid", &folder)).expect("roster");
        std::fs::write(folder.join("results/a.md"), "---\nstatus: done\n---\n").expect("result");
        folder
    }

    fn hook(
        &self,
        event: &str,
        stdin: &str,
        extra: &[(&str, &str)],
        path: Option<&Path>,
    ) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_crook"));
        cmd.args(["hook", event])
            .env_clear()
            .env(
                "PATH",
                path.map_or_else(|| self.root().join("bin"), Path::to_path_buf),
            )
            .env("HOME", self.root().join("home"))
            .env("XDG_DATA_HOME", self.root().join("data"))
            .env("XDG_CONFIG_HOME", self.root().join("config"))
            .env("XDG_STATE_HOME", self.root().join("state"))
            .env("FAKE_HC_LOG", self.root().join("hc.log"))
            .env("HC_TERMINAL_ID", "t1")
            .env("HC_TERMINAL_SOCKET", self.root().join("c.sock"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("crook starts");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("crook ends")
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const INPUT: &str = r#"{"session_id":"s-1","hook_event_name":"UserPromptSubmit","cwd":"/x"}"#;

fn assert_quiet_exit_zero(out: &Output, why: &str) {
    assert_eq!(out.status.code(), Some(0), "{why}: {}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "", "{why}");
    assert!(
        text(&out.stderr).contains("crook hook:"),
        "{why}: stderr was {:?}",
        text(&out.stderr)
    );
}

#[test]
fn a_coordinator_prompt_prints_the_undelivered_result() {
    let sb = Sandbox::new(true);
    let folder = sb.flock();
    let out = sb.hook("user-prompt-submit", INPUT, &[], None);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc: serde_json::Value =
        serde_json::from_str(text(&out.stdout).trim()).expect("one JSON line");
    assert_eq!(
        doc["hookSpecificOutput"]["hookEventName"],
        "UserPromptSubmit"
    );
    assert_eq!(
        doc["hookSpecificOutput"]["additionalContext"],
        format!(
            "Undelivered child result: {}",
            folder.join("results/a.md").display()
        )
    );
    // Only the identity lookup: no `server start`, no `schema`, no `ls`.
    assert_eq!(sb.log().trim(), "get --json t1");
    // The mark was written, so the next prompt says nothing.
    let again = sb.hook("user-prompt-submit", INPUT, &[], None);
    assert_eq!(text(&again.stdout), "");
}

#[test]
fn bad_stdin_and_unknown_events_exit_zero_with_a_stderr_line() {
    let sb = Sandbox::new(true);
    for (event, stdin) in [
        ("user-prompt-submit", "not json"),
        ("user-prompt-submit", ""),
        ("stop", "[]"),
        ("whatever", INPUT),
    ] {
        assert_quiet_exit_zero(
            &sb.hook(event, stdin, &[], None),
            &format!("{event} {stdin:?}"),
        );
    }
    assert_eq!(sb.log(), "", "hc was never asked");
}

#[test]
fn no_terminal_means_silence_and_no_hc_call() {
    let sb = Sandbox::new(true);
    let out = sb.hook("session-start", INPUT, &[("HC_TERMINAL_ID", "")], None);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        (text(&out.stdout).as_str(), text(&out.stderr).as_str()),
        ("", "")
    );
    assert_eq!(sb.log(), "");
}

#[test]
fn no_core_socket_means_no_hc_call() {
    // D29: every hc call but --version would start a core, so a hook must not make one.
    let sb = Sandbox::new(false);
    let out = sb.hook("session-start", INPUT, &[], None);
    assert_quiet_exit_zero(&out, "no core");
    assert!(
        text(&out.stderr).contains("no hc core"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(sb.log(), "", "hc was never run");
}

#[test]
fn hc_missing_from_path_exits_zero() {
    let sb = Sandbox::new(true);
    sb.flock();
    let empty = sb.root().join("empty");
    std::fs::create_dir(&empty).expect("mkdir");
    let out = sb.hook("stop", INPUT, &[], Some(&empty));
    assert_quiet_exit_zero(&out, "no hc");
}

/// The runner's 10 second limit on one hc call ends the hang, well inside the hook's budget.
#[test]
fn an_hc_that_hangs_is_cut_off_and_the_hook_exits_zero() {
    let sb = Sandbox::new(true);
    sb.flock();
    let started = std::time::Instant::now();
    let out = sb.hook("user-prompt-submit", INPUT, &[("FAKE_HC_HANG", "1")], None);
    assert_quiet_exit_zero(&out, "hang");
    assert!(started.elapsed() < std::time::Duration::from_secs(20));
}

#[cfg(feature = "test-panic")]
#[test]
fn a_forced_panic_exits_zero_with_nothing_on_stdout() {
    let sb = Sandbox::new(true);
    let out = sb.hook(
        "session-start",
        INPUT,
        &[("CROOK_HOOK_TEST_PANIC", "1")],
        None,
    );
    assert_quiet_exit_zero(&out, "panic");
    assert!(
        text(&out.stderr).contains("forced panic"),
        "{}",
        text(&out.stderr)
    );
}
