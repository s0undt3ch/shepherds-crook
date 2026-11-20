mod common;

use std::path::Path;
use std::process::Output;

use common::crook;
use crook::crew::{self, Child};
use crook::store::{Mode, Store, Touch};

fn env_for(xdg: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = xdg.display().to_string();
    move |k: &str| (k == "XDG_DATA_HOME").then(|| x.clone())
}

fn project_with(xdg: &Path, slug: &str, children: &[&str]) -> Store {
    let env = env_for(xdg);
    crook::project::init(&env, slug).unwrap();
    let mut s = Store::open(&env, slug, Mode::ReadWrite).unwrap();
    for id in children {
        s.write(Touch::CHILDREN, |tx| {
            crew::add_child(
                tx,
                &Child {
                    child: (*id).into(),
                    repo: "/r".into(),
                    worktree: format!("/r/.worktrees/{id}"),
                    branch: format!("crook/{id}"),
                    brief: "b".into(),
                    session_id: format!("s-{slug}-{id}"),
                },
            )
        })
        .unwrap();
    }
    s
}

fn run(xdg: &Path, args: &[&str]) -> Output {
    crook()
        .args(args)
        .env("XDG_DATA_HOME", xdg)
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn gen_mtime(xdg: &Path, slug: &str) -> std::time::SystemTime {
    std::fs::metadata(xdg.join("crook").join(slug).join("gen"))
        .unwrap()
        .modified()
        .unwrap()
}

#[test]
fn cli_unpark() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = project_with(tmp.path(), "p", &["c1"]);
    s.write(Touch::STATUS, |tx| crew::need(tx, "c1", "which way?"))
        .unwrap();
    let before = gen_mtime(tmp.path(), "p");
    std::thread::sleep(std::time::Duration::from_millis(30));
    let o = run(tmp.path(), &["unpark", "c1"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(!s.read(|c| crew::parked(c, "c1")).unwrap());
    assert!(gen_mtime(tmp.path(), "p") > before, "unpark bumps gen");
    let o = run(tmp.path(), &["unpark", "c1"]);
    assert!(
        o.status.success(),
        "a second unpark is not an error: {}",
        stderr(&o)
    );
    let o = run(tmp.path(), &["unpark", "nobody"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("nobody"), "{}", stderr(&o));
}

#[test]
fn unpark_is_refused_inside_a_session() {
    let tmp = tempfile::tempdir().unwrap();
    project_with(tmp.path(), "p", &["c1"]);
    let o = crook()
        .args(["unpark", "c1"])
        .env("XDG_DATA_HOME", tmp.path())
        .env("CROOK_ROLE", "child")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn child_address() {
    let tmp = tempfile::tempdir().unwrap();
    let mut a = project_with(tmp.path(), "alpha", &["c1", "only-a"]);
    let mut b = project_with(tmp.path(), "beta", &["c1"]);
    a.write(Touch::STATUS, |tx| crew::need(tx, "c1", "q"))
        .unwrap();
    b.write(Touch::STATUS, |tx| crew::need(tx, "c1", "q"))
        .unwrap();

    let o = run(tmp.path(), &["unpark", "c1"]);
    assert_eq!(o.status.code(), Some(1));
    let e = stderr(&o);
    assert!(e.contains("alpha/c1") && e.contains("beta/c1"), "{e}");
    assert!(a.read(|c| crew::parked(c, "c1")).unwrap(), "nothing lifted");
    assert!(b.read(|c| crew::parked(c, "c1")).unwrap(), "nothing lifted");

    let o = run(tmp.path(), &["unpark", "beta/c1"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(a.read(|c| crew::parked(c, "c1")).unwrap());
    assert!(!b.read(|c| crew::parked(c, "c1")).unwrap());

    a.write(Touch::STATUS, |tx| crew::need(tx, "only-a", "q"))
        .unwrap();
    let o = run(tmp.path(), &["unpark", "only-a"]);
    assert!(
        o.status.success(),
        "a bare child found once resolves: {}",
        stderr(&o)
    );
    assert!(!a.read(|c| crew::parked(c, "only-a")).unwrap());

    let o = run(tmp.path(), &["unpark", "gamma/c1"]);
    assert_eq!(o.status.code(), Some(1), "no such project");
    let o = run(tmp.path(), &["unpark", "Bad Slug/c1"]);
    assert_eq!(o.status.code(), Some(1));
}
