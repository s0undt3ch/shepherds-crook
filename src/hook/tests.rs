use serde_json::{Value, json};

use super::*;
use crate::crew::{self, Child};
use crate::project;
use crate::store::Touch;

const BROKEN: &str = "crook: hook error: run crook open p";

struct Fixture {
    tmp: tempfile::TempDir,
    /// The recorded `claude_pid` of every child: a process that has exited.
    pid: u32,
}

/// A pid no process has: one that was spawned and reaped.
fn dead_pid() -> u32 {
    let mut c = std::process::Command::new("true").spawn().unwrap();
    c.wait().unwrap();
    c.id()
}

/// A `sleep` that is killed when the guard drops, so a failed assert leaks nothing.
struct Live(std::process::Child);

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn live() -> Live {
    Live(
        std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap(),
    )
}

/// A live process whose command is `claude`: a copy of `sleep` by that name.
fn live_claude(dir: &std::path::Path) -> Live {
    let bin = dir.join("claude");
    let sleep = ["/bin/sleep", "/usr/bin/sleep"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap();
    std::fs::copy(sleep, &bin).unwrap();
    Live(std::process::Command::new(&bin).arg("60").spawn().unwrap())
}

/// The child env with the test's `PATH`, so the hook can run `ps`.
fn child_with_path() -> Vec<(&'static str, String)> {
    let mut v: Vec<(&str, String)> = CHILD.iter().map(|(k, v)| (*k, v.to_string())).collect();
    v.push(("PATH", std::env::var("PATH").unwrap_or_default()));
    v
}

fn as_refs<'a>(v: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    v.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

impl Fixture {
    fn new() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let f = Fixture {
            tmp,
            pid: dead_pid(),
        };
        project::init(&f.env_fn(&[]), "p").unwrap();
        let mut s = f.store();
        for (c, repo) in [("c1", "/r/a"), ("c2", "/r/b")] {
            s.write(Touch::CHILDREN, |tx| {
                crew::add_child(
                    tx,
                    &Child {
                        child: c.into(),
                        repo: repo.into(),
                        worktree: format!("{repo}/.worktrees/{c}"),
                        branch: format!("crook/{c}"),
                        brief: "do it".into(),
                        session_id: format!("s-{c}"),
                    },
                )?;
                crew::record_session(tx, c, &format!("s-{c}"), f.pid)
            })
            .unwrap();
        }
        f
    }

    fn env_fn(&self, extra: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let mut m: Vec<(String, String)> = extra
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        m.push((
            "XDG_STATE_HOME".into(),
            self.tmp.path().display().to_string(),
        ));
        move |k: &str| m.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    fn store(&self) -> Store {
        Store::open(&self.env_fn(&[]), "p", Mode::ReadWrite).unwrap()
    }

    fn park(&self, child: &str) {
        self.store()
            .write(Touch::STATUS, |tx| {
                crew::task(tx, child, "t")?;
                crew::take_task(tx, child)?;
                crew::need(tx, child, "q?")
            })
            .unwrap();
    }

    fn call(&self, event: &str, input: &str, env: &[(&str, &str)], ppid: u32) -> (i32, String) {
        let e = self.env_fn(env);
        let mut out = Vec::new();
        let code = run(event, &mut input.as_bytes(), &e, ppid, &mut out);
        (code, String::from_utf8(out).unwrap())
    }

    fn pre(&self, env: &[(&str, &str)], session: &str, tool: &str, ti: Value) -> String {
        let input = json!({"session_id": session, "tool_name": tool, "tool_input": ti});
        let (code, out) = self.call("PreToolUse", &input.to_string(), env, 1);
        assert_eq!(code, 0);
        out
    }
}

const CHILD: [(&str, &str); 3] = [
    ("CROOK_ROLE", "child"),
    ("CROOK_PROJECT", "p"),
    ("CROOK_CHILD", "c1"),
];
const ORCH: [(&str, &str); 2] = [("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "p")];

fn decision(out: &str) -> (String, String) {
    let v: Value = serde_json::from_str(out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["hookEventName"], "PreToolUse");
    (
        h["permissionDecision"].as_str().unwrap().into(),
        h["permissionDecisionReason"].as_str().unwrap().into(),
    )
}

fn context(out: &str) -> String {
    let v: Value = serde_json::from_str(out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "SessionStart");
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn hook_never_crashes() {
    let f = Fixture::new();
    let bad = ["", "not json", "[]", r#"{"session_id":"s"}"#];
    for input in bad {
        let (code, out) = f.call("PreToolUse", input, &CHILD, 1);
        assert_eq!(code, 0, "{input:?}");
        assert_eq!(
            decision(&out),
            ("ask".into(), "crook: hook error: bad hook input".into()),
            "{input:?}"
        );
    }
    let (code, out) = f.call("SessionStart", "garbage", &CHILD, 1);
    assert_eq!(code, 0);
    assert_eq!(context(&out), "crook: hook error: bad hook input");

    // No CROOK_PROJECT: every tool but the read-only ones asks.
    for (tool, asks) in [
        ("Bash", true),
        ("Write", true),
        ("Read", false),
        ("Glob", false),
        ("Grep", false),
    ] {
        let out = f.pre(&[], "s", tool, json!({}));
        if asks {
            assert_eq!(
                decision(&out),
                ("ask".into(), "crook: hook error: run crook open ".into()),
                "{tool}"
            );
        } else {
            assert_eq!(out, "", "{tool}");
        }
    }
    let (_, out) = f.call("SessionStart", r#"{"session_id":"s"}"#, &[], 1);
    assert_eq!(context(&out), "crook: hook error: run crook open ");

    // Missing database for a known slug.
    let gone = [("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "nope")];
    let out = f.pre(&gone, "s", "Bash", json!({}));
    assert_eq!(
        decision(&out),
        (
            "ask".into(),
            "crook: hook error: run crook open nope".into()
        )
    );
    assert_eq!(f.pre(&gone, "s", "Read", json!({})), "");
    let (_, out) = f.call("SessionStart", r#"{"session_id":"s"}"#, &gone, 1);
    assert_eq!(context(&out), "crook: hook error: run crook open nope");
    assert!(!f.tmp.path().join("crook/nope").exists(), "nothing created");

    // Unknown events answer nothing.
    let (code, out) = f.call("Stop", "{}", &CHILD, 1);
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn schema_mismatch_is_a_hook_error() {
    let f = Fixture::new();
    let db = paths::project_dir(&f.env_fn(&[]), "p")
        .unwrap()
        .join("crook.db");
    let c = rusqlite::Connection::open(db).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let out = f.pre(&ORCH, "s", "Bash", json!({}));
    assert_eq!(decision(&out), ("ask".into(), BROKEN.into()));
    assert_eq!(f.pre(&ORCH, "s", "Grep", json!({})), "");
    let (_, out) = f.call("SessionStart", r#"{"session_id":"s"}"#, &ORCH, 1);
    assert_eq!(context(&out), BROKEN);
}

#[test]
fn store_error_during_park_check_asks() {
    let f = Fixture::new();
    // A child env naming a child that has no rows: the park check errors.
    let env = [
        ("CROOK_ROLE", "child"),
        ("CROOK_PROJECT", "p"),
        ("CROOK_CHILD", "ghost"),
    ];
    let (d, reason) = decision(&f.pre(&env, "s", "Bash", json!({})));
    assert_eq!(d, "ask");
    assert!(reason.starts_with("crook: hook error: "), "{reason}");
    // Even Read asks: this is a store error, not a broken session.
    assert_eq!(decision(&f.pre(&env, "s", "Read", json!({}))).0, "ask");
}

#[test]
fn session_start_records() {
    let f = Fixture::new();
    for (i, source) in ["startup", "resume", "clear", "compact"].iter().enumerate() {
        let sid = format!("new-{i}");
        let input = json!({"session_id": sid, "source": source}).to_string();
        let (code, out) = f.call("SessionStart", &input, &CHILD, 4242);
        assert_eq!(code, 0);
        assert!(context(&out).contains("Project card"), "{source}");
        let (cur, _) = f.store().read(|c| crew::is_current(c, "c1", &sid)).unwrap();
        assert!(cur, "{source}");
        let pid = f.store().read(|c| crew::claude_pid(c, "c1")).unwrap();
        assert_eq!(pid, Some(4242));
    }
    let input = json!({"session_id": "o1", "source": "startup"}).to_string();
    let (_, out) = f.call("SessionStart", &input, &ORCH, 777);
    assert!(context(&out).contains("Project card"));
    let row: (String, i64) = f
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT session_id, pid FROM orchestrator WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, ("o1".into(), 777));
}

#[test]
fn session_start_fork() {
    let f = Fixture::new();
    let fork = json!({"session_id": "f1", "source": "fork"}).to_string();
    // Different parent pid: a plain fork.
    f.call("SessionStart", &fork, &CHILD, 555);
    let s = f.store();
    assert!(s.read(|c| crew::is_fork(c, "c1", "f1")).unwrap());
    assert!(!s.read(|c| crew::is_current(c, "c1", "f1")).unwrap().0);
    drop(s);
    // Same pid as children.claude_pid: the /branch case.
    let branch = json!({"session_id": "b1", "source": "fork"}).to_string();
    f.call("SessionStart", &branch, &CHILD, f.pid);
    let s = f.store();
    assert!(s.read(|c| crew::is_current(c, "c1", "b1")).unwrap().0);
    assert!(!s.read(|c| crew::is_fork(c, "c1", "b1")).unwrap());
}

fn orch_row(f: &Fixture) -> (Option<String>, Option<u32>) {
    f.store().read(crew::orchestrator).unwrap()
}

#[test]
fn a_second_orchestrator_does_not_take_the_row_of_a_live_one() {
    let f = Fixture::new();
    let claude = live_claude(f.tmp.path());
    let pid = claude.0.id();
    let mut env: Vec<(&str, String)> = ORCH.iter().map(|(k, v)| (*k, v.to_string())).collect();
    env.push(("PATH", std::env::var("PATH").unwrap_or_default()));
    let env = as_refs(&env);
    f.store()
        .write(Touch::empty(), |tx| {
            crew::record_orchestrator(tx, "orch-1", pid)
        })
        .unwrap();
    // Another claude, another id: refused, row unchanged.
    let second = json!({"session_id": "orch-2", "source": "startup"}).to_string();
    let (code, out) = f.call("SessionStart", &second, &env, pid + 1);
    assert_eq!(code, 0);
    assert_eq!(
        context(&out),
        "crook: this session is not the orchestrator; the orchestrator is session orch-1"
    );
    assert_eq!(orch_row(&f), (Some("orch-1".into()), Some(pid)));
    // The recorded id from another process: it gets the card, the pid stays.
    let same = json!({"session_id": "orch-1", "source": "resume"}).to_string();
    let (_, out) = f.call("SessionStart", &same, &env, pid + 1);
    assert!(
        !context(&out).contains("not the orchestrator"),
        "{}",
        context(&out)
    );
    assert_eq!(orch_row(&f), (Some("orch-1".into()), Some(pid)));
    // The hook's own parent is the recorded pid: normal restart of the same process.
    let (_, out) = f.call("SessionStart", &second, &env, pid);
    assert!(!context(&out).contains("not the orchestrator"));
    assert_eq!(orch_row(&f), (Some("orch-2".into()), Some(pid)));
}

#[test]
fn an_orchestrator_replaces_the_row_of_a_dead_one() {
    let f = Fixture::new();
    let mut env: Vec<(&str, String)> = ORCH.iter().map(|(k, v)| (*k, v.to_string())).collect();
    env.push(("PATH", std::env::var("PATH").unwrap_or_default()));
    let env = as_refs(&env);
    let dead = dead_pid();
    f.store()
        .write(Touch::empty(), |tx| {
            crew::record_orchestrator(tx, "old", dead)
        })
        .unwrap();
    let input = json!({"session_id": "new", "source": "startup"}).to_string();
    let (_, out) = f.call("SessionStart", &input, &env, 4242);
    assert!(!context(&out).contains("not the orchestrator"));
    assert_eq!(orch_row(&f), (Some("new".into()), Some(4242)));
}

#[test]
fn forged_session_start_does_not_lift_a_park() {
    let f = Fixture::new();
    let claude = live_claude(f.tmp.path());
    let pid = claude.0.id();
    let env = child_with_path();
    let env = as_refs(&env);
    f.store()
        .write(Touch::CHILDREN, |tx| {
            crew::record_session(tx, "c1", "s-c1", pid)
        })
        .unwrap();
    f.park("c1");
    let forged = json!({"session_id": "decoy", "source": "startup"}).to_string();
    let (code, out) = f.call("SessionStart", &forged, &env, pid + 1);
    assert_eq!(code, 0);
    assert_eq!(
        context(&out),
        "crook: this session is not the child; the child is session s-c1"
    );
    let s = f.store();
    assert_eq!(
        s.read(|c| crew::is_current(c, "c1", "s-c1")).unwrap(),
        (true, "s-c1".into())
    );
    assert_eq!(s.read(|c| crew::claude_pid(c, "c1")).unwrap(), Some(pid));
    drop(s);
    for sid in ["s-c1", "decoy"] {
        assert_eq!(
            decision(&f.pre(&CHILD, sid, "Bash", json!({}))).0,
            "deny",
            "{sid}"
        );
    }
    // The same claude process (a `/clear`) still records its new id.
    let clear = json!({"session_id": "s-c1b", "source": "clear"}).to_string();
    let (_, out) = f.call("SessionStart", &clear, &env, pid);
    assert!(context(&out).contains("Project card"), "{out}");
    assert!(
        f.store()
            .read(|c| crew::is_current(c, "c1", "s-c1b"))
            .unwrap()
            .0
    );
}

#[test]
fn same_id_then_decoy_then_fork_keeps_the_park() {
    let f = Fixture::new();
    let claude = live_claude(f.tmp.path());
    let pid = claude.0.id();
    f.store()
        .write(Touch::CHILDREN, |tx| {
            crew::record_session(tx, "c1", "s-c1", pid)
        })
        .unwrap();
    f.park("c1");
    let env = child_with_path();
    let env = as_refs(&env);
    let x = pid + 7;
    for (sid, source) in [("s-c1", "resume"), ("decoy", "startup"), ("s-c1", "fork")] {
        let input = json!({"session_id": sid, "source": source}).to_string();
        f.call("SessionStart", &input, &env, x);
    }
    let s = f.store();
    assert_eq!(s.read(|c| crew::claude_pid(c, "c1")).unwrap(), Some(pid));
    assert!(s.read(|c| crew::is_current(c, "c1", "s-c1")).unwrap().0);
    drop(s);
    for sid in ["s-c1", "decoy"] {
        assert_eq!(
            decision(&f.pre(&CHILD, sid, "Bash", json!({}))).0,
            "deny",
            "{sid}"
        );
    }
}

#[test]
fn hostile_path_does_not_lift_a_park() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let claude = live_claude(f.tmp.path());
    let pid = claude.0.id();
    f.store()
        .write(Touch::CHILDREN, |tx| {
            crew::record_session(tx, "c1", "s-c1", pid)
        })
        .unwrap();
    f.park("c1");
    let bin = f.tmp.path().join("fake");
    std::fs::create_dir_all(&bin).unwrap();
    let ps = bin.join("ps");
    std::fs::write(&ps, "#!/bin/sh\necho zsh\n").unwrap();
    std::fs::set_permissions(&ps, std::fs::Permissions::from_mode(0o755)).unwrap();
    let fake = bin.display().to_string();
    for path in ["/nonexistent", fake.as_str()] {
        let mut env: Vec<(&str, &str)> = CHILD.to_vec();
        env.push(("PATH", path));
        let input = json!({"session_id": "decoy", "source": "startup"}).to_string();
        let (_, out) = f.call("SessionStart", &input, &env, pid + 1);
        assert_eq!(
            context(&out),
            "crook: this session is not the child; the child is session s-c1",
            "{path}"
        );
    }
    assert!(
        f.store()
            .read(|c| crew::is_current(c, "c1", "s-c1"))
            .unwrap()
            .0
    );
}

/// The child's own claude runs the hook itself before parking: the ppid is
/// the recorded pid, so the claude guard cannot tell it from a restart.
#[test]
fn forge_from_the_child_before_parking_keeps_the_park() {
    for fork_ppid in [None, Some(4242)] {
        let f = Fixture::new();
        let own = f.pid;
        let decoy = json!({"session_id": "decoy", "source": "startup"}).to_string();
        f.call("SessionStart", &decoy, &CHILD, own);
        // Back to the real id as a fork: through `adopt` (same pid) or `add_fork`.
        let back = json!({"session_id": "s-c1", "source": "fork"}).to_string();
        let (_, out) = f.call("SessionStart", &back, &CHILD, fork_ppid.unwrap_or(own));
        assert!(
            context(&out).starts_with("crook: this session is not the child"),
            "{out}"
        );
        let s = f.store();
        assert!(!s.read(|c| crew::is_fork(c, "c1", "s-c1")).unwrap());
        drop(s);
        f.park("c1");
        for sid in ["s-c1", "decoy"] {
            assert_eq!(
                decision(&f.pre(&CHILD, sid, "Bash", json!({}))).0,
                "deny",
                "{sid} {fork_ppid:?}"
            );
        }
    }
}

#[test]
fn clear_keeps_the_old_id_denied_while_parked() {
    let f = Fixture::new();
    let clear = json!({"session_id": "s-c1b", "source": "clear"}).to_string();
    let (_, out) = f.call("SessionStart", &clear, &CHILD, f.pid);
    assert!(context(&out).contains("Project card"), "{out}");
    assert!(
        f.store()
            .read(|c| crew::is_current(c, "c1", "s-c1b"))
            .unwrap()
            .0
    );
    let fork = json!({"session_id": "f1", "source": "fork"}).to_string();
    f.call("SessionStart", &fork, &CHILD, 555);
    f.park("c1");
    for sid in ["s-c1", "s-c1b"] {
        assert_eq!(
            decision(&f.pre(&CHILD, sid, "Bash", json!({}))).0,
            "deny",
            "{sid}"
        );
    }
    assert_eq!(f.pre(&CHILD, "f1", "Bash", json!({})), "");
}

#[test]
fn reused_pid_that_is_not_claude_lets_a_restart_record_itself() {
    let f = Fixture::new();
    let other = live();
    let pid = other.0.id();
    f.store()
        .write(Touch::CHILDREN, |tx| {
            crew::record_session(tx, "c1", "s-c1", pid)
        })
        .unwrap();
    let env = child_with_path();
    let env = as_refs(&env);
    let input = json!({"session_id": "s-new", "source": "resume"}).to_string();
    let (_, out) = f.call("SessionStart", &input, &env, 4242);
    assert!(context(&out).contains("Project card"), "{out}");
    assert!(
        f.store()
            .read(|c| crew::is_current(c, "c1", "s-new"))
            .unwrap()
            .0
    );
}

#[test]
fn same_session_id_takes_the_normal_path() {
    let f = Fixture::new();
    let claude = live_claude(f.tmp.path());
    let pid = claude.0.id();
    f.store()
        .write(Touch::CHILDREN, |tx| {
            crew::record_session(tx, "c1", "s-c1", pid)
        })
        .unwrap();
    let env = child_with_path();
    let env = as_refs(&env);
    let input = json!({"session_id": "s-c1", "source": "resume"}).to_string();
    let (_, out) = f.call("SessionStart", &input, &env, 4242);
    assert!(context(&out).contains("Project card"), "{out}");
    // The live claude keeps the row: a same-id caller only gets the card.
    assert_eq!(
        f.store().read(|c| crew::claude_pid(c, "c1")).unwrap(),
        Some(pid)
    );
}

#[test]
fn restart_after_the_child_died_records_itself() {
    let f = Fixture::new();
    f.park("c1");
    let input = json!({"session_id": "s-new", "source": "resume"}).to_string();
    let (_, out) = f.call("SessionStart", &input, &CHILD, 4242);
    assert!(context(&out).contains("Project card"), "{out}");
    let s = f.store();
    assert!(s.read(|c| crew::is_current(c, "c1", "s-new")).unwrap().0);
    assert_eq!(s.read(|c| crew::claude_pid(c, "c1")).unwrap(), Some(4242));
}

/// Stands in for a closed stdout.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}

/// Writes fine, fails to flush.
struct NoFlush;

impl Write for NoFlush {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }
}

#[test]
fn failed_stdout_write_exits_two() {
    let f = Fixture::new();
    let e = f.env_fn(&CHILD);
    let bash = r#"{"session_id":"s","tool_name":"Bash"}"#;
    assert_eq!(
        run("PreToolUse", &mut "junk".as_bytes(), &e, 1, &mut Closed),
        2
    );
    assert_eq!(
        run("PreToolUse", &mut "junk".as_bytes(), &e, 1, &mut NoFlush),
        2
    );
    // No decision means no output, so nothing can fail.
    let ok = r#"{"session_id":"s-c1","tool_name":"Bash"}"#;
    assert_eq!(run("PreToolUse", &mut ok.as_bytes(), &e, 1, &mut Closed), 0);
    assert_eq!(run("Stop", &mut bash.as_bytes(), &e, 1, &mut Closed), 0);
}

#[test]
fn park_denial() {
    let f = Fixture::new();
    f.call(
        "SessionStart",
        &json!({"session_id": "f1", "source": "fork"}).to_string(),
        &CHILD,
        555,
    );
    // Not parked: nothing is denied.
    assert_eq!(f.pre(&CHILD, "s-c1", "Bash", json!({})), "");
    f.park("c1");
    let (d, _) = decision(&f.pre(&CHILD, "s-c1", "Bash", json!({})));
    assert_eq!(d, "deny");
    assert_eq!(f.pre(&CHILD, "s-c1", "mcp__crook__status", json!({})), "");
    // Unrecorded session in the same env.
    assert_eq!(
        decision(&f.pre(&CHILD, "stranger", "Read", json!({}))).0,
        "deny"
    );
    // A recorded fork is allowed.
    assert_eq!(f.pre(&CHILD, "f1", "Bash", json!({})), "");
    // The same id in children and forks is still the child.
    f.store()
        .write(Touch::CHILDREN, |tx| {
            tx.execute(
                "INSERT INTO forks(child, session_id) VALUES ('c1','s-c1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        decision(&f.pre(&CHILD, "s-c1", "Bash", json!({}))).0,
        "deny"
    );
    // A missing session id is unrecorded.
    let input = json!({"tool_name": "Bash"}).to_string();
    let (_, out) = f.call("PreToolUse", &input, &CHILD, 1);
    assert_eq!(decision(&out).0, "deny");
}

#[test]
fn stop_spawn_ask() {
    let f = Fixture::new();
    let stop = json!({"child": "c1"});
    let spawn = json!({"child": "n", "repo": "/r/a/", "brief": "b"});
    assert_eq!(f.pre(&ORCH, "o", "mcp__crook__stop", stop.clone()), "");
    assert_eq!(f.pre(&ORCH, "o", "mcp__crook__spawn", spawn.clone()), "");
    f.park("c1");
    assert_eq!(
        decision(&f.pre(&ORCH, "o", "mcp__crook__stop", stop)).0,
        "ask"
    );
    assert_eq!(
        decision(&f.pre(&ORCH, "o", "mcp__crook__spawn", spawn)).0,
        "ask"
    );
    let other = json!({"child": "n", "repo": "/r/c", "brief": "b"});
    assert_eq!(f.pre(&ORCH, "o", "mcp__crook__spawn", other), "");
    assert_eq!(
        f.pre(&ORCH, "o", "mcp__crook__stop", json!({"child": "c2"})),
        ""
    );
    assert_eq!(
        f.pre(&ORCH, "o", "mcp__crook__stop", json!({"child": "nope"})),
        ""
    );
    assert_eq!(f.pre(&ORCH, "o", "Bash", json!({})), "");
}

#[cfg(feature = "test-panic")]
#[test]
fn forced_panic_is_caught() {
    let f = Fixture::new();
    let (code, out) = f.call(
        "PreToolUse",
        r#"{"tool_name":"Bash"}"#,
        &[("CROOK_HOOK_TEST_PANIC", "1")],
        1,
    );
    assert_eq!(code, 0);
    assert_eq!(decision(&out).0, "ask");
}
