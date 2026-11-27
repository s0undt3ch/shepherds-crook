use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::cmd::spawn;
use crate::files::roster::State;
use crate::flock::Flock;
use crate::hc::fake::{Call, FakeHc};
use crate::testutil::{
    Buffers, Fixture, HookedHc, SharedBuf, init_repo, load_roster, make_flock, start_coordinator,
    write_brief,
};

struct World {
    fx: Fixture,
    hc: FakeHc,
    flock: Flock,
    coordinator: String,
    repo: std::path::PathBuf,
}

/// A live coordinator and two children in worktrees of one repository.
fn world() -> World {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    let repo = init_repo(&fx.root());
    for name in ["auth", "docs"] {
        write_brief(&flock, name);
        let brief = flock
            .briefs_dir()
            .join(format!("{name}.md"))
            .display()
            .to_string();
        spawn::run(
            &fx.ctx(),
            &hc,
            &mut Buffers::default().io(),
            &spawn::Args {
                role: "child",
                child: name,
                brief: &brief,
                repo: Some(&repo),
                model: None,
                flock: None,
            },
        )
        .expect("spawns");
    }
    World {
        fx,
        hc,
        flock,
        coordinator,
        repo,
    }
}

fn close(w: &World, bufs: &mut Buffers) -> Result<(), Error> {
    run(&w.fx.ctx(), &w.hc, &mut bufs.io(), "demo")
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
fn everything_ends_done_and_the_coordinator_is_killed_last() {
    let w = world();
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("closes");

    let roster = load_roster(&w.flock);
    assert_eq!(roster.coordinator.state, State::Done);
    assert!(roster.children.iter().all(|c| c.state == State::Done));
    let killed = kills(&w.hc);
    assert_eq!(killed.last(), Some(&w.coordinator));
    assert_eq!(killed.len(), 3);
    assert_eq!(bufs.out.text(), "closed flock demo\n");
}

#[test]
fn the_order_is_children_then_roster_then_worktrees_then_warnings_then_the_coordinator() {
    let w = world();
    std::fs::write(w.repo.join(".worktrees/demo-docs/scratch.txt"), "wip").expect("write");
    let events: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut bufs = Buffers::default();
    let (out, err): (SharedBuf, SharedBuf) = (bufs.out.clone(), bufs.err.clone());
    let folder = w.flock.folder.clone();
    let log = Rc::clone(&events);
    let coordinator = w.coordinator.clone();
    let mut hooked = HookedHc::new(&w.hc);
    hooked.on_kill = Some(Box::new(move |id| {
        let roster = crate::files::roster::load(&folder).expect("roster");
        let done = roster.coordinator.state == State::Done;
        let who = if id == coordinator {
            "coordinator"
        } else {
            "child"
        };
        log.borrow_mut().push(format!(
            "kill {who}: roster done={done}, warned={}, said={}",
            err.text().contains("kept worktree"),
            out.text().contains("closed flock")
        ));
    }));
    run(&w.fx.ctx(), &hooked, &mut bufs.io(), "demo").expect("closes");
    assert_eq!(
        *events.borrow(),
        [
            "kill child: roster done=false, warned=false, said=false",
            "kill child: roster done=false, warned=false, said=false",
            "kill coordinator: roster done=true, warned=true, said=true",
        ]
    );
}

#[test]
fn clean_worktrees_are_removed_and_unsafe_ones_kept_each_with_a_warning() {
    let w = world();
    let docs = w.repo.join(".worktrees/demo-docs");
    let auth = w.repo.join(".worktrees/demo-auth");
    std::fs::write(docs.join("scratch.txt"), "wip").expect("write");
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("closes");
    assert!(!auth.exists());
    assert!(docs.is_dir());
    let err = bufs.err.text();
    let warnings: Vec<&str> = err.lines().collect();
    assert_eq!(warnings.len(), 1, "{err}");
    assert!(warnings[0].starts_with("warning: kept worktree "));
    assert!(warnings[0].contains("docs") && warnings[0].contains("untracked files"));
}

#[test]
fn a_second_close_retries_the_kept_worktree_and_prints_the_warning_while_it_is_unsafe() {
    let w = world();
    let docs = w.repo.join(".worktrees/demo-docs");
    let scratch = docs.join("scratch.txt");
    std::fs::write(&scratch, "wip").expect("write");
    close(&w, &mut Buffers::default()).expect("first");
    let kills_after_first = kills(&w.hc).len();
    let calls_after_first = w.hc.calls().len();

    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("second, still dirty");
    assert!(bufs.err.text().contains("untracked files"));
    assert_eq!(bufs.out.text(), "");

    std::fs::remove_file(&scratch).expect("clean");
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("third, clean");
    assert!(!docs.exists());
    assert_eq!(
        (bufs.out.text(), bufs.err.text()),
        (String::new(), String::new())
    );

    // Nothing was live, so nothing was killed.
    assert_eq!(kills(&w.hc).len(), kills_after_first);
    assert!(
        w.hc.calls().len() > calls_after_first,
        "hc is asked what is live"
    );

    // With nothing kept, another close prints nothing and exits 0.
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("fourth");
    assert_eq!(
        (bufs.out.text(), bufs.err.text()),
        (String::new(), String::new())
    );
}

#[test]
fn a_child_that_will_not_die_is_a_warning_and_the_close_goes_on() {
    let w = world();
    w.hc.fail("kill", 1);
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("closes");
    assert!(
        bufs.err.text().contains("could not end"),
        "{}",
        bufs.err.text()
    );
    assert_eq!(load_roster(&w.flock).coordinator.state, State::Done);
}

#[test]
fn an_unknown_slug_is_refused() {
    let w = world();
    let err = run(&w.fx.ctx(), &w.hc, &mut Buffers::default().io(), "nope").expect_err("refused");
    assert!(err.to_string().contains("demo"), "{err}");
}

#[test]
fn a_closed_flock_is_then_refused_by_spawn_and_open_and_skipped_by_resume() {
    let w = world();
    close(&w, &mut Buffers::default()).expect("closes");
    let open = crate::cmd::open::run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        "demo",
        None,
    );
    assert!(open.is_err());
    write_brief(&w.flock, "late");
    let brief = w.flock.briefs_dir().join("late.md").display().to_string();
    let spawned = spawn::run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        &spawn::Args {
            role: "child",
            child: "late",
            brief: &brief,
            repo: None,
            model: None,
            flock: Some("demo"),
        },
    );
    assert!(spawned.is_err());
    let before = w.hc.calls().len();
    crate::cmd::resume::run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        None,
        None,
        Some("demo"),
    )
    .expect("skipped");
    let runs = w.hc.calls()[before..]
        .iter()
        .filter(|c| matches!(c, Call::Run(_)))
        .count();
    assert_eq!(runs, 0);
}

#[test]
fn a_child_whose_kill_failed_keeps_its_worktree_with_a_warning() {
    let w = world();
    w.hc.fail("kill", 1);
    let mut bufs = Buffers::default();
    close(&w, &mut bufs).expect("closes");
    let kept: Vec<_> = ["auth", "docs"]
        .iter()
        .filter(|n| w.repo.join(format!(".worktrees/demo-{n}")).is_dir())
        .collect();
    assert_eq!(kept.len(), 1, "the one whose terminal would not die");
    assert!(
        bufs.err.text().contains("still running"),
        "{}",
        bufs.err.text()
    );
}

#[test]
fn a_failed_coordinator_kill_is_retried_by_closing_again() {
    let w = world();
    // The two children go first; the third kill, the coordinator's, fails.
    let flaky = FailLast {
        inner: &w.hc,
        kills: std::cell::Cell::new(0),
        fail_at: 3,
    };
    let mut bufs = Buffers::default();
    run(&w.fx.ctx(), &flaky, &mut bufs.io(), "demo").expect_err("coordinator kill failed");
    assert_eq!(load_roster(&w.flock).coordinator.state, State::Done);
    assert!(w.hc.get("demo").expect("get").expect("known").is_live());

    let mut again = Buffers::default();
    close(&w, &mut again).expect("retry");
    assert!(!w.hc.get("demo").expect("get").expect("known").is_live());
    assert_eq!(
        again.out.text(),
        "",
        "already closed, so no second `closed` line"
    );
}

#[test]
fn a_child_terminal_cannot_close_the_flock() {
    let w = world();
    w.fx.set_env("HC_TERMINAL_ID", "t2");
    let err = close(&w, &mut Buffers::default()).expect_err("refused");
    assert!(err.to_string().contains("belongs to a child"), "{err}");
    assert_eq!(load_roster(&w.flock).coordinator.state, State::Active);
}

/// Fails the `fail_at`th kill and no other.
struct FailLast<'a> {
    inner: &'a dyn crate::hc::Hc,
    kills: std::cell::Cell<usize>,
    fail_at: usize,
}

impl crate::hc::Hc for FailLast<'_> {
    fn version(&self) -> Result<String, Error> {
        self.inner.version()
    }
    fn schema(&self) -> Result<crate::hc::Schema, Error> {
        self.inner.schema()
    }
    fn ls(&self, all: bool) -> Result<Vec<crate::hc::Terminal>, Error> {
        self.inner.ls(all)
    }
    fn ls_by_label(
        &self,
        all: bool,
        labels: &[(String, String)],
    ) -> Result<Vec<crate::hc::Terminal>, Error> {
        self.inner.ls_by_label(all, labels)
    }
    fn get(&self, id: &str) -> Result<Option<crate::hc::Terminal>, Error> {
        self.inner.get(id)
    }
    fn run(&self, spec: &crate::hc::RunSpec) -> Result<crate::hc::Terminal, Error> {
        self.inner.run(spec)
    }
    fn send(&self, to: &str, text: &str) -> Result<crate::hc::SendOutcome, Error> {
        self.inner.send(to, text)
    }
    fn kill(&self, id: &str) -> Result<crate::hc::KillOutcome, Error> {
        self.kills.set(self.kills.get() + 1);
        if self.kills.get() == self.fail_at {
            return Err(Error::Hc {
                args: "kill".to_string(),
                kind: None,
                message: "scripted failure".to_string(),
            });
        }
        self.inner.kill(id)
    }
    fn server_start(&self, env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Result<(), Error> {
        self.inner.server_start(env)
    }
}
