//! Large inputs are checked inside the hook's budget, or answered with an ask
//! when they cannot be. Ignored by default because they are only meaningful in
//! a release build:
//! `cargo nextest run --release --run-ignored only`.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::crook;
use crook::Env;
use crook::project::{self, ItemKind};
use crook::store::{Mode, Store, Touch};
use serde_json::{Value, json};

const BUDGET: Duration = Duration::from_millis(500);
const WRITE_BYTES: usize = 5 << 20;

/// Deterministic prose-like text: lines of 6 to 14 words from a fixed
/// vocabulary, and now and then a file path or a URL, as real documents have.
fn prose(bytes: usize, seed: u64, min_words: usize) -> String {
    let vocab: Vec<String> = (0..4000)
        .map(|i| format!("w{:x}x{}", i * 7919 % 4093, i % 13))
        .collect();
    let mut state = seed;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let mut out = String::with_capacity(bytes + 256);
    while out.len() < bytes {
        let n = min_words + next() % 9;
        for i in 0..n {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&vocab[next() % vocab.len()]);
        }
        match next() % 40 {
            0 => out.push_str(&format!(
                " see src/mod{}/file{}.rs",
                next() % 50,
                next() % 900
            )),
            1 => out.push_str(&format!(" https://example.com/docs/{}", next() % 5000)),
            _ => {}
        }
        out.push('\n');
    }
    out
}

struct World {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    notes: PathBuf,
    repo: PathBuf,
}

impl World {
    /// A project with a private notes folder, and an empty working directory.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let x = base.display().to_string();
        let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
        let env: Env = &env;
        project::init(env, "p").unwrap();
        let notes = base.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(notes.join("secret.md"), "x\n").unwrap();
        let repo = base.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let w = World {
            _tmp: tmp,
            base,
            notes,
            repo,
        };
        w.add(ItemKind::Notes, &w.notes);
        w
    }

    fn add(&self, kind: ItemKind, path: &std::path::Path) {
        let x = self.base.display().to_string();
        let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
        let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        s.write(Touch::PROJECT, |tx| {
            project::add(tx, kind, &path.display().to_string()).map(|_| ())
        })
        .unwrap();
    }

    /// One `Write` into the working directory through the real binary: how long
    /// it took, and the hook's answer.
    fn write(&self, content: &str) -> (Duration, Value) {
        let file_path = self.repo.join("out.md").display().to_string();
        self.call("Write", json!({"file_path": file_path, "content": content}))
    }

    fn call(&self, tool: &str, tool_input: Value) -> (Duration, Value) {
        let input = json!({
            "session_id": "s", "tool_name": tool, "cwd": self.repo.display().to_string(),
            "tool_input": tool_input,
        })
        .to_string();
        let mut c = crook();
        c.args(["hook", "PreToolUse"])
            .env("XDG_STATE_HOME", &self.base)
            .env("HOME", self.base.join("home"))
            .env("CROOK_ROLE", "orchestrator")
            .env("CROOK_PROJECT", "p")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        warm_binary();
        let started = Instant::now();
        let mut child = c.spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
        let out = child.wait_with_output().unwrap();
        let took = started.elapsed();
        writer.join().unwrap();
        assert_eq!(out.status.code(), Some(0));
        let v = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        (took, v)
    }
}

/// The first exec of a relinked binary pays macOS launch checks, which are not
/// the hook's cost: run it once, untimed, per test process.
fn warm_binary() {
    static WARM: std::sync::Once = std::sync::Once::new();
    WARM.call_once(|| {
        let _ = crook().args(["hook", "Nope"]).stdin(Stdio::null()).output();
    });
}

fn reason(v: &Value) -> &str {
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["permissionDecision"], "ask", "{v}");
    h["permissionDecisionReason"].as_str().unwrap()
}

/// A 5 MB `Write` that names a private file at its end.
#[test]
#[ignore = "release-only budget test"]
fn large_write_budget() {
    let w = World::new();
    let secret = w.notes.join("secret.md").display().to_string();
    // A `+` makes every line a candidate for the decoded rescan.
    let mut content = prose(WRITE_BYTES, 7, 6).replace('\n', " a+b\n");
    content.push_str(&format!("see {secret}\n"));
    let (took, v) = w.write(&content);
    eprintln!("5 MB write checked in {took:?}");
    assert_eq!(reason(&v), format!("crook: private reference: {secret}"));
    assert!(took < BUDGET, "checked in {took:?}, budget {BUDGET:?}");
}

/// 200,000 distinct relative paths cost a filesystem lookup each: past the
/// check's deadline the hook asks instead of running on.
#[test]
#[ignore = "release-only budget test"]
fn too_large_to_check_asks_in_budget() {
    let w = World::new();
    let mut cmd = String::from("cat");
    for i in 0..200_000 {
        cmd.push_str(&format!(" t{i}/x"));
    }
    let (took, v) = w.call("Bash", json!({ "command": cmd }));
    eprintln!("{} byte command of relative paths: {took:?}", cmd.len());
    assert_eq!(
        reason(&v),
        "crook: private reference check: input too large to check"
    );
    assert!(took < BUDGET, "answered in {took:?}, budget {BUDGET:?}");
}

/// A 5 MB `Write` that names a folder with a root's spaced name after its
/// parent's name every 200 lines, each read back to every start its line allows
/// and resolved, and the detour into the root at its end.
#[test]
#[ignore = "release-only budget test"]
fn spaced_root_names_budget() {
    let w = World::new();
    let spaced = w.base.join("My Notes");
    std::fs::create_dir_all(&spaced).unwrap();
    w.add(ItemKind::Private, &spaced);
    let parent = w.base.file_name().unwrap().to_str().unwrap().to_string();
    let decoy = format!(" in ../{parent}/My Notes/a.md");
    let mut n = 0;
    let mut content = String::new();
    for (i, line) in prose(WRITE_BYTES, 11, 6).lines().enumerate() {
        content.push_str(line);
        if i % 200 == 0 {
            n += 1;
            content.push_str(&decoy);
        }
        content.push('\n');
    }
    let detour = format!("{}/repo/../My Notes/plan.md", w.base.display());
    content.push_str(&format!("see {detour}\n"));
    let (took, v) = w.write(&content);
    eprintln!("5 MB write with {n} spaced names checked in {took:?}");
    assert_eq!(reason(&v), format!("crook: private reference: {detour}"));
    assert!(took < BUDGET, "checked in {took:?}, budget {BUDGET:?}");
}
