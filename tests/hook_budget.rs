//! Review Focus 4: a large `Write` is checked inside the hook's budget.
//! Ignored by default because it is only meaningful in a release build:
//! `cargo nextest run --release --run-ignored only`.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::crook;
use crook::Env;
use crook::project::{self, ItemKind};
use crook::store::{Mode, Store, Touch};
use serde_json::{Value, json};

const BUDGET: Duration = Duration::from_millis(500);
const INDEX_BYTES: u64 = 7 << 20;
const NOTE_FILES: usize = 12;
const NOTE_BYTES: usize = 900 << 10;
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

const SECRET: &str = "the private quarterly migration moves billing onto the new ledger";
const REPO_FILES: usize = 3000;

struct World {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    notes: PathBuf,
    repo: PathBuf,
}

impl World {
    /// A project with private notes whose index comes to at least 7 MB (asserted
    /// on the first call), and an empty working directory.
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(tmp.path()).unwrap();
        let x = base.display().to_string();
        let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
        let env: Env = &env;
        project::init(env, "p").unwrap();
        let notes = base.join("notes");
        std::fs::create_dir_all(&notes).unwrap();
        for i in 0..NOTE_FILES {
            let text = prose(NOTE_BYTES, 100 + i as u64, 14);
            std::fs::write(notes.join(format!("n{i}.md")), text).unwrap();
        }
        std::fs::write(notes.join("secret.md"), format!("{SECRET}\n")).unwrap();
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

    /// A real git repo of tracked, realistic text, added to the project.
    fn track_repo(&self) {
        for i in 0..REPO_FILES {
            let dir = self.repo.join(format!("d{}", i % 50));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("f{i}.txt")),
                prose(2 << 10, 5000 + i as u64, 8),
            )
            .unwrap();
        }
        for args in [
            &["init", "-q"][..],
            &["add", "-A"],
            &["commit", "-q", "-m", "x"],
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&self.repo)
                .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}");
        }
        self.add(ItemKind::Repo, &self.repo);
    }

    fn index_bytes(&self) -> u64 {
        let index = self.base.join("crook/p/index/index.bin");
        std::fs::metadata(index).unwrap().len()
    }

    /// One `Write` into the working directory through the real binary: how long
    /// it took, and the hook's answer.
    fn write(&self, content: &str) -> (Duration, Value) {
        let input = json!({
            "session_id": "s", "tool_name": "Write", "cwd": self.repo.display().to_string(),
            "tool_input": {"file_path": self.repo.join("out.md").display().to_string(),
                "content": content},
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

fn assert_asks_about_secret(v: &Value, took: Duration) {
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["permissionDecision"], "ask", "{v}");
    let reason = h["permissionDecisionReason"].as_str().unwrap();
    assert!(reason.starts_with("crook: private reference: "), "{reason}");
    assert!(took < BUDGET, "checked in {took:?}, budget {BUDGET:?}");
}

#[test]
#[ignore = "release-only budget test"]
fn large_write_budget() {
    let w = World::new();
    // The first call builds the index; the budget is for the calls after it.
    let (cold, _) = w.write("hello");
    let index_bytes = w.index_bytes();
    assert!(
        index_bytes >= INDEX_BYTES,
        "index is only {index_bytes} bytes"
    );

    let mut content = prose(WRITE_BYTES, 7, 6);
    content.push_str(SECRET);
    content.push('\n');
    let (took, v) = w.write(&content);
    eprintln!("index {index_bytes} bytes, cold build {cold:?}, 5 MB write checked in {took:?}");
    assert_asks_about_secret(&v, took);
    assert!(v.to_string().contains("secret.md"), "{v}");
}

/// A Write quoting megabytes of private notes, with a project repo of thousands
/// of tracked files to subtract against: the repo search is cut off at its
/// deadline, every hit stands, and the answer is still an ask inside the budget.
#[test]
#[ignore = "release-only budget test"]
fn quoting_write_with_big_repo_budget() {
    let w = World::new();
    w.track_repo();
    w.write("hello");
    let mut content = String::new();
    for i in 0..4 {
        content.push_str(&std::fs::read_to_string(w.notes.join(format!("n{i}.md"))).unwrap());
    }
    let (took, v) = w.write(&content);
    eprintln!(
        "{} byte quote of private notes, {REPO_FILES}-file repo: {took:?}",
        content.len()
    );
    assert_asks_about_secret(&v, took);
}
