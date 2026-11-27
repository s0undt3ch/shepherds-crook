use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::files::roster::Roster;
use crate::flock::Flock;
use crate::hc::fake::{Call, FakeHc};
use crate::hc::{Parent, RunSpec};
use crate::testutil::{
    Buffers, Fixture, HookedHc, init_repo, load_roster, make_flock, start_coordinator, write_brief,
};

struct World {
    fx: Fixture,
    hc: FakeHc,
    flock: Flock,
    coordinator: String,
}

fn world() -> World {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    write_brief(&flock, "auth");
    World {
        fx,
        hc,
        flock,
        coordinator,
    }
}

fn brief(w: &World, name: &str) -> String {
    w.flock
        .briefs_dir()
        .join(format!("{name}.md"))
        .display()
        .to_string()
}

fn args<'a>(child: &'a str, brief: &'a str) -> Args<'a> {
    Args {
        role: "child",
        child,
        brief,
        repo: None,
        model: None,
        flock: None,
    }
}

fn spawn(w: &World, bufs: &mut Buffers, a: &Args) -> Result<(), Error> {
    run(&w.fx.ctx(), &w.hc, &mut bufs.io(), a)
}

fn child_runs(hc: &FakeHc) -> Vec<RunSpec> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Run(s) if s.name.contains('.') => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn the_launch_matches_the_golden_shape() {
    let w = world();
    let path = brief(&w, "auth");
    let mut bufs = Buffers::default();
    spawn(&w, &mut bufs, &args("auth", &path)).expect("spawns");

    let specs = child_runs(&w.hc);
    let spec = &specs[0];
    let entry = &load_roster(&w.flock).children[0];
    assert_eq!(spec.name, "demo.auth");
    assert_eq!(spec.cwd, w.flock.folder);
    assert_eq!(spec.parent, Parent::Terminal(w.coordinator.clone()));
    let own = w.fx.root().join("bin/crook");
    let mut expected: Vec<String> = [
        "env".to_string(),
        format!("CROOK_BIN={}", own.display()),
        "claude".into(),
        "--plugin-dir".into(),
        crate::install::find_own(&w.fx.ctx())
            .expect("install")
            .plugin_dir()
            .display()
            .to_string(),
        "--add-dir".into(),
        w.flock.folder.display().to_string(),
        "--agent".into(),
        "crook:child".into(),
        "--model".into(),
        "sonnet".into(),
        "--session-id".into(),
        entry.session_id.clone(),
    ]
    .into();
    expected.push(format!(
        "Read {}. Write your result to {}. Follow the crook-child skill: create the result file first and \
         keep its status and progress up to date, because your Stop hook reports it to the coordinator.",
        path,
        w.flock.folder.join("results/auth.md").display()
    ));
    assert_eq!(spec.argv, expected);
    assert!(
        spec.labels
            .contains(&("crook.child".to_string(), "auth".to_string()))
    );
    assert_eq!(
        bufs.out.text(),
        format!(
            "spawned demo.auth (terminal t2, session {})\n",
            entry.session_id
        )
    );
}

#[test]
fn the_roster_entry_is_written_before_hc_run() {
    let w = world();
    let seen = Rc::new(Cell::new(false));
    let seen_in = Rc::clone(&seen);
    let folder = w.flock.folder.clone();
    let mut hooked = HookedHc::new(&w.hc);
    hooked.on_run = Some(Box::new(move |spec| {
        if spec.name == "demo.auth" {
            let on_disk: Roster = crate::files::roster::load(&folder).expect("roster");
            assert_eq!(on_disk.children.len(), 1, "entry first");
            assert_eq!(on_disk.children[0].state, State::Active);
            seen_in.set(true);
        }
    }));
    let path = brief(&w, "auth");
    run(
        &w.fx.ctx(),
        &hooked,
        &mut Buffers::default().io(),
        &args("auth", &path),
    )
    .expect("spawns");
    assert!(seen.get());
}

#[test]
fn the_model_is_the_flag_then_the_config_then_sonnet_and_never_null() {
    for (config, flag, expected) in [
        (None, None, "sonnet"),
        (Some("default_child_model = \"haiku\"\n"), None, "haiku"),
        (
            Some("default_child_model = \"haiku\"\n"),
            Some("opus"),
            "opus",
        ),
        (None, Some("opus"), "opus"),
    ] {
        let w = world();
        if let Some(c) = config {
            w.fx.write_config(c);
        }
        let path = brief(&w, "auth");
        let mut a = args("auth", &path);
        a.model = flag;
        spawn(&w, &mut Buffers::default(), &a).expect("spawns");
        assert_eq!(load_roster(&w.flock).children[0].model, expected);
        let argv = &child_runs(&w.hc)[0].argv;
        let at = argv.iter().position(|x| x == "--model").expect("model");
        assert_eq!(argv[at + 1], expected);
    }
}

#[test]
fn a_repo_gets_a_worktree_and_the_roster_records_it() {
    let w = world();
    let repo = init_repo(&w.fx.root());
    let path = brief(&w, "auth");
    let mut a = args("auth", &path);
    a.repo = Some(repo.as_path());
    spawn(&w, &mut Buffers::default(), &a).expect("spawns");

    let worktree = repo.join(".worktrees/demo-auth");
    assert!(worktree.is_dir());
    let entry = &load_roster(&w.flock).children[0];
    assert_eq!(entry.folder, worktree);
    assert_eq!(entry.repo.as_deref(), Some(repo.as_path()));
    assert_eq!(entry.branch.as_deref(), Some("crook/demo/auth"));
    assert_eq!(entry.brief, "briefs/auth.md");
    assert_eq!(child_runs(&w.hc)[0].cwd, worktree);
}

#[test]
fn no_repo_means_the_flock_folder_and_no_repo_fields() {
    let w = world();
    let path = brief(&w, "auth");
    spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect("spawns");
    let entry = &load_roster(&w.flock).children[0];
    assert_eq!(entry.folder, w.flock.folder);
    assert_eq!((entry.repo.as_ref(), entry.branch.as_ref()), (None, None));
}

#[test]
fn a_duplicate_child_is_refused_without_a_launch() {
    let w = world();
    let path = brief(&w, "auth");
    spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect("spawns");
    let err = spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect_err("refused");
    assert!(err.to_string().contains("already has a child"), "{err}");
    assert_eq!(child_runs(&w.hc).len(), 1);
}

#[test]
fn a_brief_outside_briefs_is_refused() {
    let w = world();
    let outside = w.fx.root().join("elsewhere.md");
    std::fs::write(&outside, "x").expect("write");
    let in_root = w.flock.folder.join("loose.md");
    std::fs::write(&in_root, "x").expect("write");
    std::fs::create_dir(w.flock.briefs_dir().join("sub")).expect("mkdir");
    std::fs::write(w.flock.briefs_dir().join("sub/deep.md"), "x").expect("write");
    let link = w.flock.briefs_dir().join("link.md");
    std::os::unix::fs::symlink(&outside, &link).expect("symlink");
    for bad in [
        outside.display().to_string(),
        in_root.display().to_string(),
        "loose.md".to_string(),
        "briefs/sub/deep.md".to_string(),
        link.display().to_string(),
        "briefs/missing.md".to_string(),
        w.flock.briefs_dir().display().to_string(),
    ] {
        let err = spawn(&w, &mut Buffers::default(), &args("auth", &bad)).expect_err(&bad);
        assert!(matches!(err, Error::Flock(_)), "{bad}: {err}");
    }
    assert!(load_roster(&w.flock).children.is_empty());
    assert!(child_runs(&w.hc).is_empty());
}

#[test]
fn a_relative_brief_is_taken_from_the_flock_folder_and_stored_relative() {
    let w = world();
    spawn(&w, &mut Buffers::default(), &args("auth", "briefs/auth.md")).expect("spawns");
    assert_eq!(load_roster(&w.flock).children[0].brief, "briefs/auth.md");
}

#[test]
fn without_a_terminal_or_flock_it_is_refused_before_any_hc_call() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    write_brief(&flock, "auth");
    let hc = FakeHc::new();
    let path = flock.briefs_dir().join("auth.md").display().to_string();
    let err = run(
        &fx.ctx(),
        &hc,
        &mut Buffers::default().io(),
        &args("auth", &path),
    )
    .expect_err("refused");
    assert!(err.to_string().contains("--flock"), "{err}");
    assert!(hc.calls().is_empty());
}

#[test]
fn with_flock_and_no_terminal_the_parent_is_the_live_coordinator() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    write_brief(&flock, "auth");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    let path = flock.briefs_dir().join("auth.md").display().to_string();
    let mut a = args("auth", &path);
    a.flock = Some("demo");
    run(&fx.ctx(), &hc, &mut Buffers::default().io(), &a).expect("spawns");
    assert_eq!(child_runs(&hc)[0].parent, Parent::Terminal(coordinator));
}

#[test]
fn a_flock_with_no_live_coordinator_is_refused_and_nothing_is_left() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    write_brief(&flock, "auth");
    let repo = init_repo(&fx.root());
    let hc = FakeHc::new();
    let path = flock.briefs_dir().join("auth.md").display().to_string();
    let mut a = args("auth", &path);
    a.flock = Some("demo");
    a.repo = Some(repo.as_path());
    let err = run(&fx.ctx(), &hc, &mut Buffers::default().io(), &a).expect_err("refused");
    assert!(err.to_string().contains("no live coordinator"), "{err}");
    assert!(load_roster(&flock).children.is_empty());
    assert!(!repo.join(".worktrees").exists());
}

#[test]
fn a_child_terminal_cannot_spawn() {
    let w = world();
    let path = brief(&w, "auth");
    spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect("spawns");
    write_brief(&w.flock, "other");
    w.fx.set_env("HC_TERMINAL_ID", "t2");
    let other = brief(&w, "other");
    let err = spawn(&w, &mut Buffers::default(), &args("other", &other)).expect_err("refused");
    assert!(err.to_string().contains("belongs to a child"), "{err}");
}

#[test]
fn a_closed_flock_takes_no_children() {
    let w = world();
    crate::files::roster::mark_all_done(&w.flock.folder).expect("close");
    let path = brief(&w, "auth");
    let err = spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect_err("refused");
    assert!(err.to_string().contains("closed"), "{err}");
}

#[test]
fn a_bad_child_name_is_refused_before_hc_is_touched() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    for bad in ["Auth", "a--b", "sixteen-chars-xxx", "-x"] {
        let mut a = args(bad, "briefs/x.md");
        a.flock = Some("demo");
        run(&fx.ctx(), &hc, &mut Buffers::default().io(), &a).expect_err(bad);
    }
    assert!(hc.calls().is_empty());
}

#[test]
fn a_failed_launch_removes_the_entry_and_the_worktree_so_the_name_can_be_used_again() {
    let w = world();
    let repo = init_repo(&w.fx.root());
    let path = brief(&w, "auth");
    let mut a = args("auth", &path);
    a.repo = Some(repo.as_path());

    w.hc.fail("run", 1);
    spawn(&w, &mut Buffers::default(), &a).expect_err("launch fails");
    assert!(load_roster(&w.flock).children.is_empty());
    assert!(!repo.join(".worktrees/demo-auth").exists());

    let mut bufs = Buffers::default();
    spawn(&w, &mut bufs, &a).expect("the same name works again");
    assert_eq!(load_roster(&w.flock).children.len(), 1);
    assert!(repo.join(".worktrees/demo-auth").is_dir());
    assert_eq!(bufs.err.text(), "");
}

#[test]
fn a_failed_launch_without_a_repo_frees_the_name_too() {
    let w = world();
    let path = brief(&w, "auth");
    w.hc.fail("run", 1);
    spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect_err("fails");
    assert!(load_roster(&w.flock).children.is_empty());
    spawn(&w, &mut Buffers::default(), &args("auth", &path)).expect("again");
}

#[test]
fn the_entry_starts_unstarted_and_is_started_once_hc_run_succeeds() {
    let w = world();
    let seen = Rc::new(Cell::new(None));
    let seen_in = Rc::clone(&seen);
    let folder = w.flock.folder.clone();
    let mut hooked = HookedHc::new(&w.hc);
    hooked.on_run = Some(Box::new(move |spec| {
        if spec.name == "demo.auth" {
            let roster = crate::files::roster::load(&folder).expect("roster");
            seen_in.set(Some(roster.children[0].started));
        }
    }));
    let path = brief(&w, "auth");
    run(
        &w.fx.ctx(),
        &hooked,
        &mut Buffers::default().io(),
        &args("auth", &path),
    )
    .expect("spawns");
    assert_eq!(
        seen.get(),
        Some(false),
        "not started at the moment of the launch"
    );
    assert!(load_roster(&w.flock).children[0].started);
}

#[test]
fn a_timeout_with_the_terminal_live_keeps_everything_and_succeeds_with_a_warning() {
    let w = world();
    let repo = init_repo(&w.fx.root());
    let path = brief(&w, "auth");
    let mut a = args("auth", &path);
    a.repo = Some(repo.as_path());
    w.hc.time_out_next_run(true);
    let mut bufs = Buffers::default();
    spawn(&w, &mut bufs, &a).expect("it is running, so it worked");
    assert!(
        bufs.err.text().contains("terminal is live"),
        "{}",
        bufs.err.text()
    );
    assert!(
        bufs.out
            .text()
            .starts_with("spawned demo.auth (terminal t2")
    );
    let roster = load_roster(&w.flock);
    assert!(roster.children[0].started);
    assert!(repo.join(".worktrees/demo-auth").is_dir());
}

#[test]
fn a_timeout_with_nothing_live_undoes_everything() {
    let w = world();
    let repo = init_repo(&w.fx.root());
    let path = brief(&w, "auth");
    let mut a = args("auth", &path);
    a.repo = Some(repo.as_path());
    w.hc.time_out_next_run(false);
    spawn(&w, &mut Buffers::default(), &a).expect_err("timed out");
    assert!(load_roster(&w.flock).children.is_empty());
    assert!(!repo.join(".worktrees/demo-auth").exists());
}

#[test]
fn a_timeout_where_the_check_fails_too_keeps_everything_for_a_resume() {
    let w = world();
    let path = brief(&w, "auth");
    w.hc.time_out_next_run(true);
    w.hc.fail("get", 1);
    let mut bufs = Buffers::default();
    // The `get` failure has to hit the check, not the identity lookup, so the flock is explicit.
    let mut a = args("auth", &path);
    a.flock = Some("demo");
    w.fx.set_env("HC_TERMINAL_ID", "");
    spawn(&w, &mut bufs, &a).expect_err("unknown outcome");
    assert!(
        bufs.err.text().contains("could not tell"),
        "{}",
        bufs.err.text()
    );
    let roster = load_roster(&w.flock);
    assert_eq!(roster.children.len(), 1);
    assert!(!roster.children[0].started);
}

#[test]
fn only_a_role_with_an_agent_file_other_than_the_coordinator_is_accepted() {
    let w = world();
    let path = brief(&w, "auth");
    for bad in ["coordinator", "bogus", "../child", "", "Child"] {
        let mut a = args("auth", &path);
        a.role = bad;
        let err = spawn(&w, &mut Buffers::default(), &a).expect_err(bad);
        assert!(err.to_string().contains("unknown role"), "{bad}: {err}");
    }
    assert!(child_runs(&w.hc).is_empty());
    assert!(load_roster(&w.flock).children.is_empty());
}
