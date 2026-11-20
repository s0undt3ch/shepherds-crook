//! Review Focus 4: a large `Write` is checked inside the hook's budget.
//! Ignored by default because it is only meaningful in a release build:
//! `cargo nextest run --release --run-ignored only`.

mod common;

use std::io::Write;
use std::process::Stdio;
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

#[test]
#[ignore = "release-only budget test"]
fn large_write_budget() {
    let tmp = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(tmp.path()).unwrap();
    let x = base.display().to_string();
    let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
    let env: Env = &env;
    project::init(env, "p").unwrap();

    // Private notes, in files under the indexer's per-file cap, sized so the
    // index file comes to at least 7 MB (asserted below).
    let notes = base.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    for i in 0..NOTE_FILES {
        let text = prose(NOTE_BYTES, 100 + i as u64, 14);
        std::fs::write(notes.join(format!("n{i}.md")), text).unwrap();
    }
    let secret = "the private quarterly migration moves billing onto the new ledger";
    std::fs::write(notes.join("secret.md"), format!("{secret}\n")).unwrap();
    let mut s = Store::open(env, "p", Mode::ReadWrite).unwrap();
    s.write(Touch::PROJECT, |tx| {
        project::add(tx, ItemKind::Notes, &notes.display().to_string()).map(|_| ())
    })
    .unwrap();

    let repo = base.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let call = |content: &str| -> (Duration, Value) {
        let input = json!({
            "session_id": "s", "tool_name": "Write", "cwd": repo.display().to_string(),
            "tool_input": {"file_path": repo.join("out.md").display().to_string(), "content": content},
        })
        .to_string();
        let mut c = crook();
        c.args(["hook", "PreToolUse"])
            .env("XDG_STATE_HOME", &base)
            .env("HOME", base.join("home"))
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
    };

    // The first call builds the index; the budget is for the calls after it.
    let (cold, _) = call("hello");
    let index = notes.parent().unwrap().join("crook/p/index/index.bin");
    let index_bytes = std::fs::metadata(&index).unwrap().len();
    assert!(
        index_bytes >= INDEX_BYTES,
        "index is only {index_bytes} bytes"
    );

    let mut content = prose(WRITE_BYTES, 7, 6);
    content.push_str(secret);
    content.push('\n');
    assert!(content.len() >= WRITE_BYTES);
    let (took, v) = call(&content);
    eprintln!("index {index_bytes} bytes, cold build {cold:?}, 5 MB write checked in {took:?}");
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["permissionDecision"], "ask", "{v}");
    assert!(
        h["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("secret.md"),
        "{v}"
    );
    assert!(took < BUDGET, "checked in {took:?}, budget {BUDGET:?}");
}
