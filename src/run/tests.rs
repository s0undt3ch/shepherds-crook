#![allow(clippy::disallowed_methods)]

use std::time::{Duration, Instant};

use super::fake::FakeRunner;
use super::{Cmd, Output, RealRunner, Runner};
use crate::Error;

const LONG: Duration = Duration::from_secs(20);

fn sh(script: &str) -> Cmd {
    Cmd::new("/bin/sh").args(["-c", script])
}

#[test]
fn captures_stdout_stderr_and_a_zero_exit() {
    let out = RealRunner
        .run(sh("echo out; echo err >&2"), LONG)
        .expect("runs");
    assert_eq!(out.code, Some(0));
    assert_eq!(out.stdout, "out\n");
    assert_eq!(out.stderr, "err\n");
}

#[test]
fn a_non_zero_exit_is_an_output_not_an_error() {
    let out = RealRunner.run(sh("exit 9"), LONG).expect("runs");
    assert_eq!(out.code, Some(9));
}

#[test]
fn a_command_that_outlives_its_deadline_is_killed_in_time() {
    let start = Instant::now();
    let err = RealRunner
        .run(sh("sleep 30"), Duration::from_millis(200))
        .expect_err("times out");
    assert!(matches!(err, Error::Timeout { .. }), "{err}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_descendant_holding_the_pipes_open_does_not_hang_a_finished_command() {
    let start = Instant::now();
    let out = RealRunner
        .run(sh("echo done; sleep 30 &"), LONG)
        .expect("runs");
    assert_eq!(out.stdout, "done\n");
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn output_larger_than_a_pipe_buffer_does_not_deadlock() {
    let out = RealRunner
        .run(
            sh("head -c 2000000 /dev/zero | tr '\\0' x; head -c 2000000 /dev/zero | tr '\\0' y >&2"),
            LONG,
        )
        .expect("runs");
    assert_eq!(out.stdout.len(), 2_000_000);
    assert_eq!(out.stderr.len(), 2_000_000);
}

#[test]
fn a_missing_program_names_the_program() {
    let err = RealRunner
        .run(Cmd::new("/nonexistent/crook-test-program"), LONG)
        .expect_err("fails");
    assert!(matches!(err, Error::Spawn { .. }), "{err}");
    assert!(
        err.to_string().contains("/nonexistent/crook-test-program"),
        "{err}"
    );
}

#[test]
fn a_full_environment_replaces_the_inherited_one() {
    let cmd = Cmd::new("/usr/bin/env").env([("ONLY".to_string(), "1".to_string())]);
    let out = RealRunner.run(cmd, LONG).expect("runs");
    assert_eq!(out.stdout, "ONLY=1\n");
}

#[test]
fn the_working_directory_is_honoured() {
    let dir = std::env::temp_dir().canonicalize().expect("temp dir");
    let out = RealRunner
        .run(Cmd::new("/bin/pwd").cwd(&dir), LONG)
        .expect("runs");
    assert_eq!(out.stdout.trim(), dir.to_str().expect("utf-8 path"));
}

#[test]
fn the_fake_records_calls_and_replays_answers_in_order() {
    let fake = FakeRunner::new([
        Ok(Output::success("one")),
        Err(Error::Timeout {
            program: "hc".into(),
            after: Duration::from_secs(1),
        }),
    ]);
    let first = fake.run(Cmd::new("hc").args(["ls"]), LONG).expect("first");
    assert_eq!(first.stdout, "one");
    assert!(fake.run(Cmd::new("hc").args(["get", "x"]), LONG).is_err());
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].args, ["ls"]);
    assert_eq!(calls[1].args, ["get", "x"]);
}

#[test]
fn a_chatty_descendant_cannot_keep_a_finished_command_waiting() {
    let start = Instant::now();
    let out = RealRunner
        .run(
            sh("(while :; do echo x; sleep 0.05; done) & echo done"),
            Duration::from_secs(2),
        )
        .expect("runs");
    assert!(out.stdout.contains("done\n"), "{:?}", out.stdout);
    assert!(
        start.elapsed() < Duration::from_millis(2500),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_timeout_takes_the_grandchildren_with_it() {
    let marker = std::env::temp_dir().join(format!(
        "crook-run-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    let script = format!("(sleep 1; echo late > {}) & sleep 30", marker.display());
    let err = RealRunner
        .run(sh(&script), Duration::from_millis(200))
        .expect_err("times out");
    assert!(matches!(err, Error::Timeout { .. }), "{err}");
    std::thread::sleep(Duration::from_millis(2000));
    let survived = marker.exists();
    let _ = std::fs::remove_file(&marker);
    assert!(!survived, "a grandchild outlived the timeout");
}

#[test]
fn a_timeout_message_keeps_sub_second_precision() {
    let err = Error::Timeout {
        program: "hc".into(),
        after: Duration::from_millis(250),
    };
    assert_eq!(err.to_string(), "hc: timed out after 250ms");
}

#[test]
fn a_missing_working_folder_is_reported_against_the_folder() {
    let err = RealRunner
        .run(
            Cmd::new("/bin/pwd").cwd(std::path::Path::new("/nonexistent/crook-dir")),
            LONG,
        )
        .expect_err("fails");
    assert!(matches!(err, Error::WorkDir { .. }), "{err}");
    assert!(err.to_string().contains("/nonexistent/crook-dir"), "{err}");
    assert!(!err.to_string().contains("/bin/pwd"), "{err}");
}

#[test]
fn output_beyond_the_cap_is_dropped_without_blocking_the_child() {
    let out = RealRunner
        .run(sh("head -c 20000000 /dev/zero | tr '\\0' x"), LONG)
        .expect("runs");
    assert_eq!(out.stdout.len(), super::MAX_CAPTURE);
}
