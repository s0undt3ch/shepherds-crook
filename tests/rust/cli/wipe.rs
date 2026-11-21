//! `crook wipe` as the binary runs it: the gate, and a project with nothing live.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use crate::common::crook;

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
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("wiped demo"), "{out}");
    // A file crook did not make stays, and is named.
    assert!(home.join("diary").is_dir() && home.join("scratch.txt").is_file());
    assert!(out.contains("scratch.txt"), "{out}");
    assert!(!tmp.path().join("state/crook/demo").exists());

    // The home is still crook's, so it can be adopted again.
    std::fs::remove_file(home.join("scratch.txt")).unwrap();
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

/// The review's reproduction: `home` points where the user keeps repos, and a
/// project is named after one of them. Neither `init` nor `wipe` may touch it.
#[test]
fn a_home_override_never_adopts_or_wipes_an_existing_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    let repo = code.join("api");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::write(repo.join("main.rs"), "fn main() {}\n").unwrap();
    let cfg = tmp.path().join("cfg/crook/config.toml");
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(&cfg, format!("home = {:?}\n", code.display().to_string())).unwrap();

    let o = run(tmp.path(), &["init", "api"], &[]);
    assert_ne!(o.status.code(), Some(0));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("crook did not make it"), "{err}");
    assert!(!tmp.path().join("state/crook/api").exists());
    assert!(!repo.join("diary").exists());

    // A project whose home crook did not make (adopted before this check
    // existed) is wiped without touching that home.
    std::fs::write(&cfg, "").unwrap();
    init_slug(tmp.path(), "api");
    std::fs::write(&cfg, format!("home = {:?}\n", code.display().to_string())).unwrap();
    let o = run(tmp.path(), &["wipe", "api", "--with-diary"], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(out.contains("crook did not make it"), "{out}");
    assert!(repo.join(".git").is_dir() && repo.join("main.rs").is_file());
    assert!(!tmp.path().join("state/crook/api").exists());
}

fn init_slug(tmp: &Path, slug: &str) {
    let o = run(tmp, &["init", slug], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
}
