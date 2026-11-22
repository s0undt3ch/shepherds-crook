use std::io::Write;
use std::process::{Output, Stdio};

use crate::common::crook;

fn run_with_stdin(mut cmd: std::process::Command, input: &str) -> Output {
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

#[test]
fn user_verb_refused_inside_claude_code() {
    let out = crook()
        .args(["init", "x"])
        .env("CLAUDECODE", "1")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("CLAUDECODE"));
}

#[test]
fn stateless_verbs_skip_the_gate() {
    for args in [
        vec!["--version"],
        vec!["--help"],
        vec!["completion", "print", "zsh"],
    ] {
        let out = crook().args(&args).env("CLAUDECODE", "1").output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?}");
        assert!(!out.stdout.is_empty(), "{args:?}");
    }
}

#[test]
fn completion_covers_bash_zsh_fish() {
    for shell in ["bash", "zsh", "fish"] {
        let out = crook()
            .args(["completion", "print", shell])
            .output()
            .unwrap();
        assert!(out.status.success(), "{shell}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("crook"),
            "{shell}"
        );
    }
}

#[test]
fn hook_with_empty_stdin_exits_zero() {
    let out = run_with_stdin(
        {
            let mut c = crook();
            c.args(["hook", "PreToolUse"]);
            c
        },
        "",
    );
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn hook_unknown_event_is_silent() {
    let out = run_with_stdin(
        {
            let mut c = crook();
            c.args(["hook", "NoSuchEvent"]);
            c
        },
        "",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.is_empty());
}
