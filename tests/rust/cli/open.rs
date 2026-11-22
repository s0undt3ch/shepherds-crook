use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use crate::common::crook;

/// A fake `claude`: answers the preflight calls and records how it was started.
fn fake_claude(dir: &Path) {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let f = bin.join("claude");
    std::fs::write(
        &f,
        r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.329 (Claude Code)" ;;
  agents) echo "[]" ;;
  *) { for a in "$@"; do echo "ARG:$a"; done; env | sed 's/^/ENV:/'; echo "PWD:$(pwd)"; } > "$OUT" ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run(dir: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let mut c = crook();
    c.args(args)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", dir.join("bin").display()),
        )
        .env("HOME", dir.join("h"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("CLAUDE_CONFIG_DIR", dir.join("claude"))
        .env("OUT", dir.join("out.txt"));
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

#[test]
fn open_execs_claude_on_a_scrubbed_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let d = tmp.path();
    fake_claude(d);
    let o = run(d, &["init", "demo"], &[]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = run(
        d,
        &["open", "demo"],
        &[
            ("CROOK_CHILD", "leak"),
            ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/x"),
        ],
    );
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{err}");
    assert!(err.contains("will ask you to trust"), "{err}");
    assert!(
        err.contains("MCP tools able to read other projects"),
        "{err}"
    );
    let out = std::fs::read_to_string(d.join("out.txt")).unwrap();
    for want in [
        "ARG:-n",
        "ARG:crook-demo",
        "ARG:--session-id",
        "ARG:--permission-mode",
        "ARG:--plugin-dir",
        "ENV:CROOK_PROJECT=demo",
        "ENV:CROOK_ROLE=orchestrator",
    ] {
        assert!(out.lines().any(|l| l == want), "{want}\n{out}");
    }
    assert!(out.contains("ENV:CROOK_BIN=/"), "{out}");
    assert!(!out.contains("CROOK_CHILD"), "{out}");
    assert!(!out.contains("MESSAGING"), "{out}");
    // `PWD` is the project home (resolved: `/var` is `/private/var` on macOS).
    let pwd = out.lines().find_map(|l| l.strip_prefix("PWD:")).unwrap();
    assert!(pwd.ends_with("/data/crook/demo"), "{pwd}");
}

#[test]
fn open_refuses_inside_a_session_and_for_an_unknown_project() {
    let tmp = tempfile::tempdir().unwrap();
    fake_claude(tmp.path());
    let o = run(tmp.path(), &["open", "demo"], &[("CROOK_ROLE", "child")]);
    assert_eq!(o.status.code(), Some(2));
    let o = run(tmp.path(), &["open", "demo"], &[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("crook init demo"));
    assert!(!tmp.path().join("state").exists());
}
