//! `crook wipe` as the binary runs it: the gate, and a project with nothing live.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use common::crook;

fn init(tmp: &Path) {
    let o = run(tmp, &["init", "demo"], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}

fn run(tmp: &Path, args: &[&str], extra: &[(&str, &str)]) -> Output {
    let d = tmp.display();
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    if !claude.exists() {
        // A `claude` that lists no sessions.
        std::fs::write(&claude, "#!/bin/sh\necho '[]'\n").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let mut c = crook();
    c.args(args)
        .env("HOME", format!("{d}/h"))
        .env("XDG_DATA_HOME", format!("{d}/data"))
        .env("XDG_STATE_HOME", format!("{d}/state"))
        .env("XDG_CONFIG_HOME", format!("{d}/cfg"))
        .env("PATH", path);
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

#[test]
fn wipe_is_a_user_verb() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    for env in [("CLAUDECODE", "1"), ("CROOK_ROLE", "orchestrator")] {
        let o = run(tmp.path(), &["wipe", "demo"], &[env]);
        assert_eq!(o.status.code(), Some(2), "{env:?}");
        assert!(
            tmp.path().join("state/crook/demo/crook.db").is_file(),
            "{env:?}"
        );
    }
}

#[test]
fn wipe_removes_a_project_and_keeps_the_diary() {
    let tmp = tempfile::tempdir().unwrap();
    init(tmp.path());
    let home = tmp.path().join("data/crook/demo");
    std::fs::write(home.join("scratch.txt"), "s").unwrap();
    let o = run(tmp.path(), &["wipe", "demo"], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(String::from_utf8_lossy(&o.stdout).contains("wiped demo"));
    assert!(home.join("diary").is_dir() && !home.join("scratch.txt").exists());
    assert!(!tmp.path().join("state/crook/demo").exists());

    init(tmp.path());
    let o = run(tmp.path(), &["wipe", "demo", "--with-diary"], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!home.exists());
}
