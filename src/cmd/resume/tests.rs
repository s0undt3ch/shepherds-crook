use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::cmd::{spawn, stop};
use crate::files::roster::State;
use crate::hc::fake::{Call, FakeHc};
use crate::hc::{Parent, RunSpec};
use crate::testutil::{
    Buffers, Fixture, HookedHc, load_roster, make_flock, start_coordinator, write_brief,
};

struct World {
    fx: Fixture,
    hc: FakeHc,
    flock: Flock,
    coordinator: String,
}

/// A live coordinator and the children `names`, each spawned in the flock folder.
fn world(names: &[&str]) -> World {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    for name in names {
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
                repo: None,
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
    }
}

fn resume(
    w: &World,
    bufs: &mut Buffers,
    child: Option<&str>,
    model: Option<&str>,
) -> Result<(), Error> {
    run(&w.fx.ctx(), &w.hc, &mut bufs.io(), child, model, None)
}

fn runs_since(hc: &FakeHc, skip: usize) -> Vec<RunSpec> {
    hc.calls()[skip..]
        .iter()
        .filter_map(|c| match c {
            Call::Run(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

fn session_flag(spec: &RunSpec) -> (&str, &str) {
    let at = spec
        .argv
        .iter()
        .position(|a| a == "--session-id" || a == "--resume")
        .expect("session flag");
    (&spec.argv[at], &spec.argv[at + 1])
}

#[test]
fn only_active_children_without_a_live_terminal_are_resumed() {
    let w = world(&["lost", "alive", "stopped"]);
    w.hc.kill("demo.lost").expect("lost");
    w.hc.kill("demo.stopped").expect("lost");
    stop::run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        "stopped",
        None,
    )
    .expect("stops");
    let before = w.hc.calls().len();
    let mut bufs = Buffers::default();
    resume(&w, &mut bufs, None, None).expect("resumes");

    let specs = runs_since(&w.hc, before);
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "demo.lost");
    assert_eq!(bufs.out.text(), "resumed demo.lost (terminal t5)\n");
}

#[test]
fn the_original_name_session_role_and_model_are_reused_even_if_the_config_changed() {
    let w = world(&["lost"]);
    let session = load_roster(&w.flock).children[0].session_id.clone();
    w.fx.write_config("default_child_model = \"opus\"\n");
    w.hc.kill("demo.lost").expect("lost");
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), None, None).expect("resumes");

    let spec = &runs_since(&w.hc, before)[0];
    assert_eq!(spec.name, "demo.lost");
    assert_eq!(session_flag(spec), ("--resume", session.as_str()));
    let at = spec
        .argv
        .iter()
        .position(|a| a == "--model")
        .expect("model");
    assert_eq!(
        spec.argv[at + 1],
        "sonnet",
        "the roster's model, not the config's"
    );
    assert!(spec.argv.contains(&"crook:child".to_string()));
    assert_eq!(spec.cwd, w.flock.folder);
    let prompt = spec.argv.last().expect("prompt");
    assert!(prompt.starts_with("You were restarted."));
    assert!(prompt.contains("briefs/lost.md") && prompt.contains("results/lost.md"));
    assert!(
        spec.labels
            .contains(&("crook.child".to_string(), "lost".to_string()))
    );
}

#[test]
fn the_parent_comes_from_the_environment_or_from_the_live_coordinator() {
    let w = world(&["lost"]);
    w.hc.kill("demo.lost").expect("lost");
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), Some("lost"), None).expect("from env");
    assert_eq!(
        runs_since(&w.hc, before)[0].parent,
        Parent::Terminal(w.coordinator.clone())
    );

    // From outside any crook terminal, with --flock: found by label.
    w.fx.set_env("HC_TERMINAL_ID", "");
    w.hc.kill("demo.lost").expect("lost again");
    let before = w.hc.calls().len();
    run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        Some("lost"),
        None,
        Some("demo"),
    )
    .expect("from label");
    assert_eq!(
        runs_since(&w.hc, before)[0].parent,
        Parent::Terminal(w.coordinator.clone())
    );
}

#[test]
fn no_live_coordinator_is_refused() {
    let w = world(&["lost"]);
    w.hc.kill("demo.lost").expect("lost");
    w.hc.kill("demo").expect("coordinator gone");
    w.fx.set_env("HC_TERMINAL_ID", "");
    let before = w.hc.calls().len();
    let err = run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        None,
        None,
        Some("demo"),
    )
    .expect_err("refused");
    assert!(err.to_string().contains("no live coordinator"), "{err}");
    assert!(runs_since(&w.hc, before).is_empty());
}

#[test]
fn model_needs_a_child() {
    let w = world(&["lost"]);
    let err = resume(&w, &mut Buffers::default(), None, Some("opus")).expect_err("refused");
    assert!(err.to_string().contains("--model needs a child"), "{err}");
    assert_eq!(load_roster(&w.flock).children[0].model, "sonnet");
}

#[test]
fn a_new_model_is_in_the_roster_before_the_launch() {
    let w = world(&["lost"]);
    w.hc.kill("demo.lost").expect("lost");
    let seen = Rc::new(RefCell::new(None));
    let seen_in = Rc::clone(&seen);
    let folder = w.flock.folder.clone();
    let mut hooked = HookedHc::new(&w.hc);
    hooked.on_run = Some(Box::new(move |spec| {
        if spec.name == "demo.lost" {
            let model = crate::files::roster::load(&folder)
                .expect("roster")
                .children[0]
                .model
                .clone();
            *seen_in.borrow_mut() = Some(model);
        }
    }));
    run(
        &w.fx.ctx(),
        &hooked,
        &mut Buffers::default().io(),
        Some("lost"),
        Some("opus"),
        None,
    )
    .expect("resumes");
    assert_eq!(seen.borrow().as_deref(), Some("opus"));
    let spec = &runs_since(&w.hc, 0).into_iter().last().expect("run");
    let at = spec
        .argv
        .iter()
        .position(|a| a == "--model")
        .expect("model");
    assert_eq!(spec.argv[at + 1], "opus");
}

#[test]
fn a_model_change_on_a_working_child_is_refused_and_changes_nothing() {
    let w = world(&["busy"]);
    w.hc.set_status("demo.busy", "working", None);
    let before = w.hc.calls().len();
    let err = resume(&w, &mut Buffers::default(), Some("busy"), Some("opus")).expect_err("refused");
    assert!(err.to_string().contains("working"), "{err}");
    assert_eq!(load_roster(&w.flock).children[0].model, "sonnet");
    let killed = w.hc.calls()[before..]
        .iter()
        .any(|c| matches!(c, Call::Kill(_)));
    assert!(!killed);
}

#[test]
fn an_idle_live_child_is_killed_not_marked_done_and_resumed_on_the_new_model() {
    let w = world(&["idle"]);
    w.hc.set_status("demo.idle", "idle", None);
    let before = w.hc.calls().len();
    let mut bufs = Buffers::default();
    resume(&w, &mut bufs, Some("idle"), Some("opus")).expect("resumes");

    let calls = &w.hc.calls()[before..];
    let kill = calls
        .iter()
        .position(|c| matches!(c, Call::Kill(id) if id == "t2"))
        .expect("kill");
    let run_at = calls
        .iter()
        .position(|c| matches!(c, Call::Run(_)))
        .expect("run");
    assert!(kill < run_at);
    let roster = load_roster(&w.flock);
    assert_eq!(roster.children[0].state, State::Active);
    assert_eq!(roster.children[0].model, "opus");
    assert_eq!(bufs.out.text(), "resumed demo.idle (terminal t3)\n");
}

#[test]
fn a_live_child_without_a_model_is_left_alone() {
    let w = world(&["alive"]);
    let before = w.hc.calls().len();
    let mut bufs = Buffers::default();
    resume(&w, &mut bufs, Some("alive"), None).expect("fine");
    assert!(runs_since(&w.hc, before).is_empty());
    assert!(bufs.out.text().contains("already running"));
}

#[test]
fn a_closed_flock_is_skipped_and_an_explicit_child_of_it_is_refused() {
    let w = world(&["lost"]);
    w.hc.kill("demo.lost").expect("lost");
    crate::files::roster::mark_all_done(&w.flock.folder).expect("close");
    let before = w.hc.calls().len();
    let mut bufs = Buffers::default();
    resume(&w, &mut bufs, None, None).expect("skipped");
    assert!(runs_since(&w.hc, before).is_empty());
    assert_eq!(bufs.out.text(), "");
    assert!(resume(&w, &mut Buffers::default(), Some("lost"), None).is_err());
}

#[test]
fn a_stopped_child_is_not_resumed_even_by_name() {
    let w = world(&["gone"]);
    stop::run(
        &w.fx.ctx(),
        &w.hc,
        &mut Buffers::default().io(),
        "gone",
        None,
    )
    .expect("stops");
    let before = w.hc.calls().len();
    let err = resume(&w, &mut Buffers::default(), Some("gone"), None).expect_err("refused");
    assert!(err.to_string().contains("done"), "{err}");
    let mut bufs = Buffers::default();
    resume(&w, &mut bufs, None, None).expect("nothing to do");
    assert!(runs_since(&w.hc, before).is_empty());
    assert!(bufs.out.text().contains("no children to resume"));
}

#[test]
fn an_unknown_child_is_refused() {
    let w = world(&["lost"]);
    let err = resume(&w, &mut Buffers::default(), Some("nope"), None).expect_err("refused");
    assert!(err.to_string().contains("no child named"), "{err}");
}

#[test]
fn one_failing_child_does_not_stop_the_others() {
    let w = world(&["one", "two"]);
    w.hc.kill("demo.one").expect("lost");
    w.hc.kill("demo.two").expect("lost");
    w.hc.fail("run", 1);
    let before = w.hc.calls().len();
    let mut bufs = Buffers::default();
    let err = resume(&w, &mut bufs, None, None).expect_err("one failed");
    assert!(err.to_string().contains("1 of the children"), "{err}");
    assert!(bufs.err.text().contains("could not resume demo.one"));
    assert!(bufs.out.text().contains("resumed demo.two"));
    assert_eq!(runs_since(&w.hc, before).len(), 2);
}

#[test]
fn spawn_and_resume_agree_on_everything_but_the_session_flag_and_the_prompt() {
    let w = world(&["same"]);
    let spawned = runs_since(&w.hc, 0)
        .into_iter()
        .find(|s| s.name == "demo.same")
        .expect("spawn");
    w.hc.kill("demo.same").expect("lost");
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), None, None).expect("resumes");
    let resumed = &runs_since(&w.hc, before)[0];
    assert_eq!(spawned.labels, resumed.labels);
    assert_eq!(spawned.cwd, resumed.cwd);
    assert_eq!(spawned.parent, resumed.parent);
    let head = |s: &RunSpec| {
        let at = s
            .argv
            .iter()
            .position(|a| a == "--session-id" || a == "--resume")
            .expect("flag");
        s.argv[..at].to_vec()
    };
    assert_eq!(head(&spawned), head(resumed));
}

#[test]
fn a_child_that_never_started_is_launched_with_session_id_and_then_marked_started() {
    let w = world(&["crashed"]);
    // The crash window: the entry was written, `hc run` never happened.
    w.hc.kill("demo.crashed").expect("gone");
    crate::files::roster::set_child_started(&w.flock.folder, "crashed", false).expect("unstart");
    let session = load_roster(&w.flock).children[0].session_id.clone();
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), None, None).expect("resumes");

    let spec = &runs_since(&w.hc, before)[0];
    assert_eq!(session_flag(spec), ("--session-id", session.as_str()));
    assert!(
        spec.argv.last().expect("prompt").starts_with("Read "),
        "a first start, not a restart"
    );
    assert!(load_roster(&w.flock).children[0].started);

    // From now on it is a resume.
    w.hc.kill("demo.crashed").expect("gone again");
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), None, None).expect("resumes");
    assert_eq!(session_flag(&runs_since(&w.hc, before)[0]).0, "--resume");
}

#[test]
fn a_live_child_marked_unstarted_is_marked_started() {
    let w = world(&["alive"]);
    crate::files::roster::set_child_started(&w.flock.folder, "alive", false).expect("unstart");
    resume(&w, &mut Buffers::default(), None, None).expect("fine");
    assert!(load_roster(&w.flock).children[0].started);
}

#[test]
fn a_restored_shell_under_a_children_name_is_ended_and_the_child_resumed() {
    use crate::hc::Hc;
    let w = world(&["lost"]);
    w.hc.kill("demo.lost").expect("lost");
    let shell =
        w.hc.run(&RunSpec {
            name: "demo.lost".to_string(),
            cwd: w.flock.folder.clone(),
            labels: vec![("crook.flock".to_string(), "demo".to_string())],
            parent: Parent::Terminal(w.coordinator.clone()),
            argv: vec!["zsh".to_string()],
        })
        .expect("shell")
        .address;
    let before = w.hc.calls().len();
    resume(&w, &mut Buffers::default(), None, None).expect("resumes");
    let calls = &w.hc.calls()[before..];
    assert!(calls.contains(&Call::Kill(shell)));
    assert_eq!(runs_since(&w.hc, before).len(), 1);
}
