//! `crook stop` as the binary runs it: the gate, and a child with no session.

mod common;

use std::path::Path;
use std::process::Output;

use common::crook;
use crook::crew::{self, Child};
use crook::store::{Mode, Store, Touch};

fn env_for(xdg: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = xdg.display().to_string();
    move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
}

fn project_with_child(xdg: &Path) -> Store {
    let env = env_for(xdg);
    crook::project::init(&env, "p").unwrap();
    let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
    s.write(Touch::CHILDREN, |tx| {
        crew::add_child(
            tx,
            &Child {
                child: "c1".into(),
                repo: "/r".into(),
                worktree: "/r/.worktrees/p/c1".into(),
                branch: "crook/p/c1".into(),
                brief: "b".into(),
                session_id: "s1".into(),
            },
        )
    })
    .unwrap();
    s
}

fn run(xdg: &Path, extra: &[(&str, &str)]) -> Output {
    let mut c = crook();
    c.args(["stop", "c1"]).env("XDG_STATE_HOME", xdg);
    for (k, v) in extra {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn archived(s: &Store) -> bool {
    s.read(|c| crew::resumable(c, "c1")).unwrap().archived
}

#[test]
fn stop_is_a_user_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let s = project_with_child(tmp.path());
    for env in [("CLAUDECODE", "1"), ("CROOK_ROLE", "orchestrator")] {
        let o = run(tmp.path(), &[env]);
        assert_eq!(o.status.code(), Some(2), "{env:?}");
        assert!(!archived(&s), "{env:?}");
    }
}

#[test]
fn stop_archives_a_child_with_no_session_or_worktree() {
    let tmp = tempfile::tempdir().unwrap();
    let s = project_with_child(tmp.path());
    // A `claude` that lists no sessions, found on PATH.
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(&claude, "#!/bin/sh\necho '[]'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let o = run(tmp.path(), &[("PATH", &path)]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        out.contains("stopped c1") && out.contains("0 worktrees"),
        "{out}"
    );
    assert!(archived(&s));
}
