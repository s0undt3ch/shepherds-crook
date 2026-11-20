mod common;

use std::io::Write;
use std::process::{Command, Output, Stdio};

use common::crook;
use crook::Env;
use crook::store::{Mode, Store};
use serde_json::{Value, json};

fn mod_cmd(xdg: &std::path::Path) -> Command {
    let mut c = crook();
    c.args(["mod", "ping"])
        .env("XDG_DATA_HOME", xdg)
        .env("CROOK_ROLE", "child")
        .env("CROOK_PROJECT", "p")
        .env("CROOK_CHILD", "c1");
    c
}

fn run_with_stdin(mut cmd: Command, input: &str) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout_json(out: &Output) -> Value {
    serde_json::from_slice(&out.stdout).expect("stdout is one JSON object")
}

const GOOD: &str = r#"{"session_id":"s1","user_turn":false}"#;

#[test]
fn mod_ping() {
    let tmp = tempfile::tempdir().unwrap();
    let xdg = tmp.path().display().to_string();
    let env = |k: &str| (k == "XDG_DATA_HOME").then(|| xdg.clone());
    let env: Env = &env;
    Store::open(env, "p", Mode::Migrate).unwrap();

    let out = run_with_stdin(mod_cmd(tmp.path()), GOOD);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        stdout_json(&out),
        json!({"ok":true,"slug":"p","role":"child","child":"c1","schema":1,
               "gen":format!("{xdg}/crook/p/gen")})
    );
}

#[test]
fn mod_missing_db() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run_with_stdin(mod_cmd(tmp.path()), GOOD);
    assert_eq!(out.status.code(), Some(1));
    let v = stdout_json(&out);
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("run crook open p"));
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

#[test]
fn mod_bad_json() {
    let tmp = tempfile::tempdir().unwrap();
    let out = run_with_stdin(mod_cmd(tmp.path()), "{not json");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["ok"], false);
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}

#[test]
fn mod_without_identity_is_a_json_error() {
    let out = run_with_stdin(
        {
            let mut c = crook();
            c.args(["mod", "ping"]);
            c
        },
        GOOD,
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(stdout_json(&out)["ok"], false);
}
