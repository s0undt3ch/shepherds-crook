mod common;

use std::io::Write;
use std::process::{Command, Output, Stdio};

use common::crook;
use crook::Env;
use crook::crew::{self, Child};
use crook::project;
use crook::store::{Mode, Store, Touch};
use serde_json::{Value, json};

fn hook(event: &str, xdg: &std::path::Path, input: &str, extra: &[(&str, &str)]) -> Output {
    let mut c: Command = crook();
    c.args(["hook", event]).env("XDG_STATE_HOME", xdg);
    for (k, v) in extra {
        c.env(k, v);
    }
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}

const CHILD: [(&str, &str); 3] = [
    ("CROOK_ROLE", "child"),
    ("CROOK_PROJECT", "p"),
    ("CROOK_CHILD", "c1"),
];

fn setup() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let x = tmp.path().display().to_string();
    let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
    let env: Env = &env;
    project::init(env, "p").unwrap();
    let mut s = Store::open(env, "p", Mode::ReadWrite).unwrap();
    s.write(Touch::CHILDREN, |tx| {
        crew::add_child(
            tx,
            &Child {
                child: "c1".into(),
                repo: "/r/a".into(),
                worktree: "/r/a/.worktrees/c1".into(),
                branch: "crook/c1".into(),
                brief: "do it".into(),
                session_id: "s-c1".into(),
            },
        )
    })
    .unwrap();
    tmp
}

fn json_out(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).expect("stdout is JSON")
}

#[test]
fn bad_stdin_asks_and_exits_zero() {
    let tmp = setup();
    let o = hook("PreToolUse", tmp.path(), "not json", &CHILD);
    assert_eq!(o.status.code(), Some(0));
    let v = json_out(&o);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "ask");
    assert_eq!(
        v["hookSpecificOutput"]["permissionDecisionReason"],
        "crook: hook error: bad hook input"
    );
}

#[test]
fn session_start_records_parent_pid() {
    let tmp = setup();
    let input = json!({"session_id": "new", "source": "startup"}).to_string();
    let o = hook("SessionStart", tmp.path(), &input, &CHILD);
    assert_eq!(o.status.code(), Some(0));
    let v = json_out(&o);
    assert!(
        v["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap()
            .contains("Project card")
    );
    let x = tmp.path().display().to_string();
    let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
    let s = Store::open(&env, "p", Mode::ReadOnly).unwrap();
    assert_eq!(
        s.read(|c| crew::claude_pid(c, "c1")).unwrap(),
        Some(std::process::id())
    );
}

#[test]
fn no_decision_is_empty_output() {
    let tmp = setup();
    let input = json!({"session_id": "s-c1", "tool_name": "Bash", "tool_input": {}}).to_string();
    let o = hook("PreToolUse", tmp.path(), &input, &CHILD);
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
}

/// Builds the release binary with the `test-panic` feature and checks that a
/// panic is caught there: this proves `panic = "unwind"` holds in the release
/// profile. Slow, so it runs with the budget test: `cargo nextest run --run-ignored all`.
#[test]
#[ignore = "builds the release profile"]
fn forced_panic_release() {
    let tmp = setup();
    let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/panic-test");
    let st = Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--features",
            "test-panic",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .unwrap();
    assert!(st.success());
    let mut c = Command::new(target.join("release/crook"));
    c.args(["hook", "PreToolUse"])
        .env_clear()
        .env("XDG_STATE_HOME", tmp.path())
        .env("CROOK_HOOK_TEST_PANIC", "1");
    for (k, v) in CHILD {
        c.env(k, v);
    }
    let mut p = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = p
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_name":"Bash"}"#);
    let o = p.wait_with_output().unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let v = json_out(&o);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "ask");
    assert!(
        v["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .starts_with("crook: hook error")
    );
}

#[test]
fn asks_write_timeout() {
    let tmp = setup();
    let notes = std::fs::canonicalize(tmp.path()).unwrap().join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    std::fs::write(notes.join("n.md"), "note\n").unwrap();
    let x = tmp.path().display().to_string();
    let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
    let env: Env = &env;
    let mut s = Store::open(env, "p", Mode::ReadWrite).unwrap();
    s.write(Touch::PROJECT, |tx| {
        project::add(tx, project::ItemKind::Notes, &notes.display().to_string()).map(|_| ())
    })
    .unwrap();

    // Another writer holds the database longer than the hook may wait.
    let db = crook::paths::project_dir(env, "p")
        .unwrap()
        .join("crook.db");
    let held = rusqlite::Connection::open(db).unwrap();
    held.execute_batch("BEGIN IMMEDIATE").unwrap();

    let input = json!({
        "session_id": "s", "tool_name": "Bash",
        "tool_input": {"command": format!("cat {}", notes.join("n.md").display())},
        "cwd": tmp.path().display().to_string(),
    })
    .to_string();
    let orch = [("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "p")];
    let started = std::time::Instant::now();
    let o = hook("PreToolUse", tmp.path(), &input, &orch);
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(4), "{took:?}");
    assert_eq!(o.status.code(), Some(0));
    let v = json_out(&o);
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "ask");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("lost asks row"), "{err}");
    assert!(err.contains("n.md"), "{err}");
    held.execute_batch("ROLLBACK").unwrap();
    let n: i64 = Store::open(env, "p", Mode::ReadOnly)
        .unwrap()
        .read(|c| Ok(c.query_row("SELECT count(*) FROM asks", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0, "the lost row stays lost");
}

/// Claude Code treats a non-zero exit under `onFailure: "block"` as a block, so
/// a decision that cannot be written must not exit 0 with no output.
#[test]
fn unwritable_stdout_exits_two() {
    let tmp = setup();
    // A pipe with no reader: every write to it fails with `EPIPE`.
    let (reader, stdout) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = crook()
        .args(["hook", "PreToolUse"])
        .env("XDG_STATE_HOME", tmp.path())
        .envs(CHILD)
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(b"not json");
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}

/// Rust's own stdout reports success on `EBADF`, so a stdout the hook cannot
/// write to (here read-only) must still end in exit 2, not 0 with no answer.
#[test]
fn read_only_stdout_exits_two() {
    let tmp = setup();
    let stdout = std::fs::File::open("/dev/null").unwrap();
    let mut child = crook()
        .args(["hook", "PreToolUse"])
        .env("XDG_STATE_HOME", tmp.path())
        .envs(CHILD)
        .stdin(Stdio::piped())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(b"not json");
    let o = child.wait_with_output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    // No answer to give: nothing to fail, so still 0.
    let stdout = std::fs::File::open("/dev/null").unwrap();
    let mut child = crook()
        .args(["hook", "Stop"])
        .env("XDG_STATE_HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(stdout)
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(b"{}");
    assert_eq!(child.wait().unwrap().code(), Some(0));
}
