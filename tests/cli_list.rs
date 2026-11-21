mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;

use common::crook;
use crook::crew::{self, Child};
use crook::store::{Mode, Store, Touch};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agents.json");

fn env_for(xdg: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let x = xdg.display().to_string();
    move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
}

/// A `claude` that prints the fixture, and `attach <id>` as the line `attach <id>`,
/// so no test runs the live command.
fn fake_bin(dir: &Path) {
    let bin = dir.join("claude");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\n[ \"$1\" = attach ] && {{ echo \"attach $2\"; exit 0; }}\n\
             [ \"$1 $2\" = \"agents --json\" ] || exit 3\nexec /bin/cat {FIXTURE}\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn add(s: &mut Store, id: &str, session: &str) {
    s.write(Touch::CHILDREN, |tx| {
        crew::add_child(
            tx,
            &Child {
                child: id.into(),
                repo: "/work/repo".into(),
                worktree: format!("/work/repo/.worktrees/{id}"),
                branch: format!("crook/{id}"),
                brief: "b".into(),
                session_id: session.into(),
            },
        )
    })
    .unwrap();
}

fn project(xdg: &Path, slug: &str) -> Store {
    let env = env_for(xdg);
    crook::project::init(&env, slug).unwrap();
    Store::open(&env, slug, Mode::ReadWrite).unwrap()
}

fn run(tmp: &Path, args: &[&str]) -> Output {
    let bin = tmp.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_bin(&bin);
    crook()
        .args(args)
        .env("XDG_STATE_HOME", tmp.join("state"))
        .env("PATH", &bin)
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn list_shows_counts_and_card() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    add(&mut s, "c1", "aaaa-interactive");
    add(&mut s, "c2", "bbbb-background");
    s.write(Touch::STATUS, |tx| {
        tx.execute("UPDATE status SET state = 'parked' WHERE child = 'c2'", [])?;
        Ok(())
    })
    .unwrap();
    let o = run(tmp.path(), &["list"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = out(&o);
    let line = text.lines().find(|l| l.starts_with("alpha")).unwrap();
    assert!(line.contains("working 1"), "{line}");
    assert!(line.contains("parked 1"), "{line}");
    assert!(line.contains("done 0"), "{line}");
    assert!(line.contains("card fresh"), "{line}");
    assert!(line.contains("orchestrator -"), "{line}");
}

#[test]
fn list_marks_a_stale_card_and_a_live_orchestrator() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    // The orchestrator's pid is this test process, which is alive; the fake
    // `claude` below reports that pid with the recorded session.
    let me = std::process::id();
    s.write(Touch::PROJECT, |tx| {
        tx.execute("UPDATE project SET dones_since_card = 3", [])?;
        tx.execute(
            "INSERT INTO orchestrator(id, session_id, pid) VALUES (1, 'orch-1', ?1)",
            [me],
        )?;
        Ok(())
    })
    .unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let claude = bin.join("claude");
    std::fs::write(
        &claude,
        format!(
            "#!/bin/sh\necho '[{{\"sessionId\":\"orch-1\",\"pid\":{me},\"cwd\":\"/x\",\"kind\":\"interactive\",\"status\":\"idle\"}}]'\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let o = crook()
        .arg("list")
        .env("XDG_STATE_HOME", &state)
        .env("PATH", &bin)
        .output()
        .unwrap();
    let text = out(&o);
    let line = text.lines().find(|l| l.starts_with("alpha")).unwrap();
    assert!(line.contains("card stale"), "{line}");
    assert!(line.contains("orchestrator live"), "{line}");
}

#[test]
fn list_orchestrator_with_another_session_is_not_live() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    let me = std::process::id();
    s.write(Touch::PROJECT, |tx| {
        tx.execute(
            "INSERT INTO orchestrator(id, session_id, pid) VALUES (1, 'orch-1', ?1)",
            [me],
        )?;
        Ok(())
    })
    .unwrap();
    // The pid is alive, but the fixture holds no agent with it: a reused pid.
    let o = run(tmp.path(), &["list"]);
    let text = out(&o);
    let line = text.lines().find(|l| l.starts_with("alpha")).unwrap();
    assert!(line.contains("orchestrator -"), "{line}");
}

#[test]
fn list_broken_db() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    drop(project(&state, "good"));
    let bad = state.join("crook/bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("crook.db"), "this is not a database, just text").unwrap();
    let o = run(tmp.path(), &["list"]);
    assert!(o.status.success(), "exit 0");
    let text = out(&o);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let good = lines.iter().find(|l| l.starts_with("good")).unwrap();
    assert!(good.contains("working 0"), "{good}");
    let bad_line = lines.iter().find(|l| l.starts_with("bad")).unwrap();
    assert!(bad_line.contains("error:"), "{bad_line}");
}

#[test]
fn list_old_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let dir = state.join("crook/old");
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("crook.db");
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch("CREATE TABLE project(id INTEGER)").unwrap();
    drop(c);
    let before = std::fs::read(&db).unwrap();
    let o = run(tmp.path(), &["list"]);
    assert!(o.status.success());
    let text = out(&o);
    assert!(text.contains("old"), "{text}");
    assert!(text.contains("run crook open old"), "{text}");
    let c = rusqlite::Connection::open(&db).unwrap();
    let v: i32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 0, "not migrated");
    assert_eq!(std::fs::read(&db).unwrap(), before, "file untouched");
}

#[test]
fn list_state_glob() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    drop(project(&state, "instate"));
    // A database under the data tree is not a project.
    let stray = tmp.path().join("data/crook/indata");
    std::fs::create_dir_all(&stray).unwrap();
    drop(rusqlite::Connection::open(stray.join("crook.db")).unwrap());
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    fake_bin(&bin);
    let o = crook()
        .arg("list")
        .env("XDG_STATE_HOME", &state)
        .env("XDG_DATA_HOME", tmp.path().join("data"))
        .env("PATH", &bin)
        .output()
        .unwrap();
    let text = out(&o);
    assert!(text.contains("instate"), "{text}");
    assert!(!text.contains("indata"), "{text}");
}

#[test]
fn list_with_no_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let o = run(tmp.path(), &["list"]);
    assert!(o.status.success());
    assert_eq!(out(&o).trim(), "no projects");
}

#[test]
fn status_flags_no_mod() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    add(&mut s, "c1", "aaaa-interactive");
    add(&mut s, "c2", "bbbb-background");
    let o = run(tmp.path(), &["status"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let text = out(&o);
    // The stray session sits inside c1's worktree and is recorded nowhere.
    let stray = text.lines().find(|l| l.contains("cccc-orphan")).unwrap();
    assert!(stray.contains("no crook mod"), "{stray}");
    assert!(stray.contains("alpha/c1"), "{stray}");
    // The recorded sessions are not flagged, and the unrelated one is ignored.
    assert!(!text.contains("aaaa-interactive"), "{text}");
    assert!(!text.contains("dddd-elsewhere"), "{text}");
    let c1 = text.lines().find(|l| l.starts_with("alpha/c1")).unwrap();
    assert!(c1.contains("agent idle"), "{c1}");
    let c2 = text.lines().find(|l| l.starts_with("alpha/c2")).unwrap();
    assert!(c2.contains("agent blocked"), "{c2}");
}

#[test]
fn status_shows_untaken_and_no_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    add(&mut s, "c3", "unknown-session");
    s.write(Touch::INBOX, |tx| {
        crew::task(tx, "c3", "do a thing")?;
        Ok(())
    })
    .unwrap();
    let o = run(tmp.path(), &["status"]);
    let text = out(&o);
    let line = text.lines().find(|l| l.starts_with("alpha/c3")).unwrap();
    assert!(line.contains("untaken 1"), "{line}");
    assert!(line.contains("no live agent"), "{line}");
}

#[test]
fn next_prints_the_oldest_waiting_child() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    add(&mut s, "c1", "aaaa-interactive");
    add(&mut s, "c2", "bbbb-background");
    add(&mut s, "c3", "unknown-session");
    s.write(Touch::STATUS, |tx| {
        // c2's agent is blocked (fixture); c3 is parked, but newer.
        tx.execute("UPDATE status SET changed_at = 100 WHERE child = 'c2'", [])?;
        tx.execute(
            "UPDATE status SET state = 'parked', changed_at = 200 WHERE child = 'c3'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let o = run(tmp.path(), &["next"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // Outside herdr, `next` runs `claude attach <session id>`.
    assert_eq!(out(&o).trim(), "attach bbbb-background");
    assert!(String::from_utf8_lossy(&o.stderr).contains("alpha/c2"));
    s.write(Touch::STATUS, |tx| {
        tx.execute("UPDATE status SET changed_at = 50 WHERE child = 'c3'", [])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        out(&run(tmp.path(), &["next"])).trim(),
        "attach unknown-session"
    );
}

#[test]
fn next_with_nothing_waiting() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let mut s = project(&state, "alpha");
    add(&mut s, "c1", "aaaa-interactive");
    let o = run(tmp.path(), &["next"]);
    assert!(o.status.success());
    assert_eq!(out(&o).trim(), "nothing waiting");
}
