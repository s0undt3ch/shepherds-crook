//! The `crook` binary's command line: what it lists, what it parses and what it prints for each command.
#![allow(clippy::expect_used)]

use std::process::{Command, Output};

/// A scratch home for one test: nothing is inherited and every folder crook reads or writes is under
/// it, so no test can touch the developer's own herdr-app, flocks or config (`setup` and
/// `doctor --fix` write there).
struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Scratch {
        Scratch {
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    fn root(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn crook(&self, args: &[&str]) -> Output {
        let root = self.root();
        Command::new(env!("CARGO_BIN_EXE_crook"))
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_STATE_HOME", root.join("state"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("HERDR_APP_HOME", root.join("app"))
            .output()
            .expect("crook starts")
    }
}

fn crook(args: &[&str]) -> Output {
    Scratch::new().crook(args)
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

#[test]
fn setup_writes_only_inside_the_scratch_home() {
    let scratch = Scratch::new();
    let out = scratch.crook(&["setup"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    // Where it wrote is the point: the marker is under HERDR_APP_HOME, and the default home was not used.
    assert!(
        scratch
            .root()
            .join("app/mods/crook/.crook-version")
            .is_file()
    );
    assert!(scratch.root().join("data/crook/flocks").is_dir());
    assert!(!scratch.root().join("home").exists());
    let said = text(&out.stdout);
    assert!(
        said.contains(&scratch.root().display().to_string()),
        "{said}"
    );
    assert!(
        !said.contains("/.config/herdr-app"),
        "setup named a default herdr-app home: {said}"
    );
}

#[test]
fn doctor_and_completion_parse_and_run_in_a_scratch_home() {
    let scratch = Scratch::new();
    for shell in ["bash", "zsh", "fish"] {
        let out = scratch.crook(&["completion", "print", shell]);
        assert_eq!(out.status.code(), Some(0), "{shell}");
        assert!(text(&out.stdout).contains("crook"));
    }
    // No hc on this PATH, so doctor fails, and still prints its report on stdout.
    let out = scratch.crook(&["doctor", "--json"]);
    assert_eq!(out.status.code(), Some(1));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json report");
    assert_eq!(doc["ok"], false);
    let out = scratch.crook(&["doctor", "--fix"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        scratch
            .root()
            .join("app/mods/crook/.crook-version")
            .is_file()
    );
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
