//! The `crook` binary's command line: what it lists, what it parses and what it prints for each command.
#![allow(clippy::expect_used)]

use std::process::{Command, Output};

fn crook(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_crook"))
        .args(args)
        .output()
        .expect("crook starts")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn version_prints_the_package_version() {
    let out = crook(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        text(&out.stdout).trim(),
        format!("crook {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn help_lists_exactly_the_planned_commands() {
    let out = crook(&["--help"]);
    assert!(out.status.success());
    let help = text(&out.stdout);
    let listed: Vec<&str> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    let expected = [
        "new",
        "open",
        "close",
        "spawn",
        "stop",
        "resume",
        "status",
        "hook",
        "completion",
        "setup",
        "doctor",
    ];
    assert_eq!(listed, expected, "help was:\n{help}");
}

/// Every form from the plan, each of which parses and then reports "not implemented".
const COMMANDS: &[&[&str]] = &[
    &["new", "my-flock"],
    &["new", "my-flock", "--task", "do the thing"],
    &["open", "my-flock"],
    &["close", "my-flock"],
    &["spawn", "child", "worker", "briefs/worker.md"],
    &[
        "spawn", "child", "worker", "b.md", "--repo", "/tmp/r", "--model", "haiku",
    ],
    &["spawn", "child", "worker", "b.md", "--flock", "my-flock"],
    &["stop", "worker"],
    &["stop", "worker", "--flock", "my-flock"],
    &["resume"],
    &["resume", "worker"],
    &["resume", "worker", "--model", "opus", "--flock", "my-flock"],
    &["status"],
    &["status", "my-flock"],
    &["status", "--json"],
    &["status", "--json", "--flock", "my-flock"],
    &["status", "my-flock", "--json"],
    &["setup"],
    &["doctor"],
    &["doctor", "--fix"],
    &["doctor", "--json"],
    &["doctor", "--fix", "--json"],
    &["completion", "print", "bash"],
    &["completion", "print", "zsh"],
    &["completion", "print", "fish"],
];

#[test]
fn every_command_parses_and_is_not_implemented() {
    for args in COMMANDS {
        let out = crook(args);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{args:?}: {}",
            text(&out.stderr)
        );
        assert!(out.stdout.is_empty(), "{args:?} wrote to stdout");
        assert!(
            text(&out.stderr).contains("not implemented"),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

#[test]
fn hook_always_exits_zero() {
    for args in [
        &["hook"][..],
        &["hook", "session-start"],
        &["hook", "stop", "--bogus"],
        &["hook", "x", "y", "z"],
    ] {
        assert_eq!(crook(args).status.code(), Some(0), "{args:?}");
    }
}

#[test]
fn bad_arguments_are_a_usage_error_not_not_implemented() {
    for args in [
        &["new"][..],
        &["spawn", "child"],
        &["completion", "print", "powershell"],
        &["status", "a", "--flock", "b"],
        &["nonsense"],
    ] {
        let out = crook(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(!text(&out.stderr).contains("not implemented"), "{args:?}");
    }
}
