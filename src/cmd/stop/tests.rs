use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::cmd::spawn;
use crate::flock::Flock;
use crate::hc::fake::{Call, FakeHc};
use crate::testutil::{
    Buffers, Fixture, HookedHc, init_repo, load_roster, make_flock, start_coordinator, write_brief,
};

struct World {
    fx: Fixture,
    hc: FakeHc,
    flock: Flock,
    repo: std::path::PathBuf,
}

/// A flock with a live coordinator and one child, `auth`, in a worktree of a repository.
fn world() -> World {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    write_brief(&flock, "auth");
    let repo = init_repo(&fx.root());
    let brief = flock.briefs_dir().join("auth.md").display().to_string();
    spawn::run(
        &fx.ctx(),
        &hc,
        &mut Buffers::default().io(),
        &spawn::Args {
            role: "child",
            child: "auth",
            brief: &brief,
            repo: Some(&repo),
            model: None,
            flock: None,
        },
    )
    .expect("spawns");
    World {
        fx,
        hc,
        flock,
        repo,
    }
}

fn stop(w: &World, bufs: &mut Buffers, child: &str) -> Result<(), Error> {
    run(&w.fx.ctx(), &w.hc, &mut bufs.io(), child, None)
}

fn kills(hc: &FakeHc) -> Vec<String> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Kill(id) => Some(id),
            _ => None,
        })
        .collect()
}

#[test]
fn the_terminal_is_killed_before_the_child_is_marked_done() {
    let w = world();
    let states = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&states);
    let folder = w.flock.folder.clone();
    let mut hooked = HookedHc::new(&w.hc);
    hooked.on_kill = Some(Box::new(move |id| {
        let state = crate::files::roster::load(&folder)
            .expect("roster")
            .children[0]
            .state;
        seen.borrow_mut().push((id.to_string(), state));
    }));
    let mut bufs = Buffers::default();
    run(&w.fx.ctx(), &hooked, &mut bufs.io(), "auth", None).expect("stops");
    assert_eq!(*states.borrow(), [("t2".to_string(), State::Active)]);
    assert_eq!(load_roster(&w.flock).children[0].state, State::Done);
    assert_eq!(bufs.out.text(), "stopped demo.auth\n");
}

#[test]
fn a_child_that_is_not_live_is_still_marked_done() {
    let w = world();
    w.hc.kill("demo.auth").expect("lost");
    let before = kills(&w.hc).len();
    stop(&w, &mut Buffers::default(), "auth").expect("stops");
    assert_eq!(load_roster(&w.flock).children[0].state, State::Done);
    assert_eq!(kills(&w.hc).len(), before, "nothing live to kill");
}

#[test]
fn a_clean_worktree_is_removed_and_the_branch_stays() {
    let w = world();
    let worktree = w.repo.join(".worktrees/demo-auth");
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect("stops");
    assert!(!worktree.exists());
    assert_eq!(bufs.err.text(), "");
    let branches = std::process::Command::new("git")
        .args(["branch", "--list", "crook/*"])
        .current_dir(&w.repo)
        .env_clear()
        .envs(crate::testutil::git_env())
        .output()
        .expect("git");
    assert!(String::from_utf8_lossy(&branches.stdout).contains("crook/demo/auth"));
}

#[test]
fn an_unsafe_worktree_is_kept_with_a_warning_and_the_reason() {
    let w = world();
    let worktree = w.repo.join(".worktrees/demo-auth");
    std::fs::write(worktree.join("scratch.txt"), "wip").expect("write");
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect("stops");
    assert!(worktree.is_dir());
    let err = bufs.err.text();
    assert!(err.starts_with("warning: kept worktree "), "{err}");
    assert!(err.contains("untracked files"), "{err}");
    assert_eq!(load_roster(&w.flock).children[0].state, State::Done);
}

#[test]
fn an_unknown_child_is_refused_with_the_names_that_exist() {
    let w = world();
    let err = stop(&w, &mut Buffers::default(), "nope").expect_err("refused");
    assert!(err.to_string().contains("auth"), "{err}");
}

#[test]
fn a_second_stop_retries_the_worktree_and_kills_nothing() {
    let w = world();
    let worktree = w.repo.join(".worktrees/demo-auth");
    let scratch = worktree.join("scratch.txt");
    std::fs::write(&scratch, "wip").expect("write");
    stop(&w, &mut Buffers::default(), "auth").expect("first");
    let kills_after_first = kills(&w.hc).len();

    // Still dirty: the same warning again.
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect("second");
    assert!(bufs.err.text().contains("untracked files"));
    assert_eq!(bufs.out.text(), "", "no `stopped` line the second time");

    // Clean now: removed, silently.
    std::fs::remove_file(&scratch).expect("clean");
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect("third");
    assert!(!worktree.exists());
    assert_eq!(
        (bufs.out.text(), bufs.err.text()),
        (String::new(), String::new())
    );
    assert_eq!(kills(&w.hc).len(), kills_after_first);
}

#[test]
fn stop_with_flock_works_outside_a_crook_terminal() {
    let w = world();
    w.fx.set_env("HC_TERMINAL_ID", "");
    run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        "auth",
        Some("demo"),
    )
    .expect("stops");
    assert_eq!(load_roster(&w.flock).children[0].state, State::Done);
}

#[test]
fn without_a_terminal_or_flock_it_is_refused() {
    let w = world();
    w.fx.set_env("HC_TERMINAL_ID", "");
    let calls = w.hc.calls().len();
    let err = stop(&w, &mut Buffers::default(), "auth").expect_err("refused");
    assert!(err.to_string().contains("--flock"), "{err}");
    assert_eq!(w.hc.calls().len(), calls);
}

#[test]
fn a_failed_kill_leaves_the_child_active_and_fails() {
    let w = world();
    w.hc.fail("kill", 1);
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect_err("kill failed");
    assert_eq!(load_roster(&w.flock).children[0].state, State::Active);
    assert_eq!(bufs.out.text(), "");
    assert!(w.repo.join(".worktrees/demo-auth").is_dir());
    // The retry works.
    stop(&w, &mut Buffers::default(), "auth").expect("stops");
    assert_eq!(load_roster(&w.flock).children[0].state, State::Done);
}

#[test]
fn a_done_child_whose_terminal_is_still_live_is_killed_and_its_worktree_kept_while_it_lives() {
    let w = world();
    // Marked done behind crook's back, as a crash between the two steps once could leave it.
    crate::files::roster::set_child_state(&w.flock.folder, "auth", State::Done).expect("done");
    w.hc.fail("kill", 1);
    let mut bufs = Buffers::default();
    stop(&w, &mut bufs, "auth").expect_err("kill failed");
    assert!(
        bufs.err.text().contains("could not be ended"),
        "{}",
        bufs.err.text()
    );
    assert!(w.repo.join(".worktrees/demo-auth").is_dir());

    stop(&w, &mut Buffers::default(), "auth").expect("second try");
    assert!(!w.repo.join(".worktrees/demo-auth").exists());
    assert_eq!(kills(&w.hc).len(), 2, "the live terminal was ended, once");
}
