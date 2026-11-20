mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use common::crook;
use crook::Env;
use crook::project;
use crook::store::{Mode, Store};

fn run(xdg: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = crook()
        .args(args)
        .env("XDG_DATA_HOME", xdg)
        .current_dir(xdg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn record(xdg: &Path) -> project::Record {
    let x = xdg.display().to_string();
    let env = move |k: &str| (k == "XDG_DATA_HOME").then(|| x.clone());
    let e: Env = &env;
    Store::open(e, "p", Mode::ReadOnly)
        .unwrap()
        .read(project::get)
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn init_creates_and_refuses_an_existing_slug() {
    let tmp = tempfile::tempdir().unwrap();
    let o = run(tmp.path(), &["init", "p"], "");
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(record(tmp.path()).version, 1);
    let o = run(tmp.path(), &["init", "p"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("already exists"), "{}", stderr(&o));
    let o = run(tmp.path(), &["init", "Bad Slug"], "");
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn add_notes_is_private() {
    let tmp = tempfile::tempdir().unwrap();
    run(tmp.path(), &["init", "p"], "");
    let o = run(
        tmp.path(),
        &["project", "p", "add-notes", "vault/../vault/n"],
        "",
    );
    assert!(o.status.success(), "{}", stderr(&o));
    // The process's cwd is the resolved path (`/var` is `/private/var` on macOS).
    let want = format!("{}/vault/n", tmp.path().canonicalize().unwrap().display());
    let r = record(tmp.path());
    assert_eq!(r.notes, [want.as_str()]);
    assert_eq!(r.private, [want.as_str()]);
}

#[test]
fn project_commands_edit_the_record() {
    let tmp = tempfile::tempdir().unwrap();
    run(tmp.path(), &["init", "p"], "");
    for (args, input) in [
        (vec!["project", "p", "set-what"], "what\n"),
        (vec!["project", "p", "set-why"], "why\n"),
        (vec!["project", "p", "set-status"], "status\n"),
        (vec!["project", "p", "add-link", "https://x.test/1"], ""),
        (vec!["project", "p", "add-ticket", "T-1"], ""),
        (vec!["project", "p", "add-private", "/secret/**"], ""),
        (vec!["project", "p", "add-allow", "false positive"], ""),
        (vec!["project", "p", "add-repo", "/r/x"], ""),
        (vec!["project", "p", "remove", "ticket", "T-1"], ""),
    ] {
        let o = run(tmp.path(), &args, input);
        assert!(o.status.success(), "{args:?}: {}", stderr(&o));
    }
    let r = record(tmp.path());
    assert_eq!(
        (r.what.as_str(), r.why.as_str(), r.status_text.as_str()),
        ("what", "why", "status")
    );
    assert_eq!(r.links, ["https://x.test/1"]);
    assert!(r.tickets.is_empty());
    assert_eq!(r.private, ["/secret/**"]);
    assert_eq!(r.allow, ["false positive"]);
    assert_eq!(r.repos, ["/r/x"]);
}

#[test]
fn user_verbs_refuse_inside_a_session_and_on_a_missing_project() {
    let tmp = tempfile::tempdir().unwrap();
    let mut c: Command = crook();
    c.args(["project", "p", "add-link", "u"])
        .env("XDG_DATA_HOME", tmp.path())
        .env("CROOK_ROLE", "orchestrator");
    let o = c.output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = run(tmp.path(), &["project", "nope", "add-link", "u"], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("run crook open nope"), "{}", stderr(&o));
}
