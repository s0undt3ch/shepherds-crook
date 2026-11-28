use super::*;
use crate::files::roster::{Child, State};
use crate::hc::Hc;
use crate::hc::fake::FakeHc;
use crate::testutil::{
    Fixture, SESSION, hook_input, load_roster, make_flock, run_hook, start_coordinator, write_brief,
};

struct Rig {
    fx: Fixture,
    hc: FakeHc,
    flock: crate::flock::Flock,
    coordinator: String,
}

fn rig() -> Rig {
    let fx = Fixture::new();
    fx.set_plugin_version("1.0.0");
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    Rig {
        fx,
        hc,
        flock,
        coordinator,
    }
}

fn add(r: &Rig, name: &str, state: State, started: bool) {
    roster::add_child(
        &r.flock.folder,
        Child {
            name: name.into(),
            role: "child".into(),
            model: "sonnet".into(),
            session_id: format!("00000000-0000-4000-8000-0000000000{:02}", name.len() + 10),
            folder: r.flock.folder.clone(),
            repo: None,
            branch: None,
            brief: format!("briefs/{name}.md"),
            state,
            started,
        },
    )
    .expect("add");
    write_brief(&r.flock, name);
}

fn live_child(r: &Rig, name: &str) -> String {
    use crate::hc::{Parent, RunSpec};
    r.hc.run(&RunSpec {
        name: format!("demo.{name}"),
        cwd: r.flock.folder.clone(),
        labels: vec![
            ("crook.flock".into(), "demo".into()),
            ("crook.child".into(), name.into()),
        ],
        parent: Parent::Terminal(r.coordinator.clone()),
        argv: vec!["claude".into()],
    })
    .expect("run")
    .address
}

fn start(r: &Rig, terminal: &str) -> crate::testutil::HookRun {
    run_hook(
        &r.fx,
        &r.hc,
        "session-start",
        Some(terminal),
        &hook_input(SESSION),
    )
}

fn runs(r: &Rig) -> Vec<String> {
    r.hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            crate::hc::fake::Call::Run(spec) => Some(spec.name),
            _ => None,
        })
        .collect()
}

#[test]
fn a_child_session_writes_a_record_and_resumes_nothing() {
    let r = rig();
    add(&r, "a", State::Active, false);
    add(&r, "b", State::Active, true);
    let a = live_child(&r, "a");
    let before = runs(&r);
    let ran = start(&r, &a);
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert_eq!(runs(&r), before, "no launch");
    let record = version::load(&r.fx.ctx(), SESSION)
        .expect("load")
        .expect("record");
    assert_eq!(
        record,
        version::Record {
            version: "1.0.0".into(),
            reloaded: false
        }
    );
    // D50: a running session proves the child started, and only that child.
    let on_disk = load_roster(&r.flock);
    assert!(on_disk.children[0].started);
}

#[test]
fn the_coordinator_session_marks_itself_started() {
    let r = rig();
    assert!(!load_roster(&r.flock).coordinator.started);
    start(&r, &r.coordinator);
    assert!(load_roster(&r.flock).coordinator.started);
}

#[test]
fn the_coordinator_resumes_the_right_children_and_prints_them() {
    let r = rig();
    add(&r, "lost", State::Active, true);
    add(&r, "running", State::Active, true);
    add(&r, "done", State::Done, true);
    add(&r, "fresh", State::Active, false);
    live_child(&r, "running");
    let ran = start(&r, &r.coordinator);
    assert_eq!(ran.stderr, "");
    let mut launched = runs(&r)[2..].to_vec();
    launched.sort();
    assert_eq!(launched, ["demo.fresh", "demo.lost"]);
    let context = ran.context().expect("context");
    assert!(
        context.contains("crook: resumed demo.lost (terminal "),
        "{context}"
    );
    assert!(
        context.contains("crook: resumed demo.fresh (terminal "),
        "{context}"
    );
    assert!(
        !context.contains("demo.running") && !context.contains("demo.done"),
        "{context}"
    );
    // The never-started child was launched fresh and is marked started now.
    assert!(load_roster(&r.flock).children.iter().all(|c| c.started));
}

#[test]
fn a_resume_failure_for_one_child_still_resumes_the_others() {
    let r = rig();
    add(&r, "first", State::Active, true);
    add(&r, "second", State::Active, true);
    r.hc.fail("run", 1);
    let ran = start(&r, &r.coordinator);
    assert_eq!(runs(&r).len(), 3, "both were tried: {:?}", runs(&r));
    let context = ran.context().expect("one was resumed");
    assert_eq!(context.matches("resumed").count(), 1, "{context}");
    assert!(ran.stderr.contains("could not resume"), "{}", ran.stderr);
}

#[test]
fn the_coordinator_start_also_rescans_the_results() {
    let r = rig();
    std::fs::write(r.flock.result_path("a"), "---\nstatus: done\n---\n").expect("result");
    let ran = start(&r, &r.coordinator);
    assert_eq!(
        ran.context().expect("context"),
        format!(
            "Undelivered child result: {}",
            r.flock.result_path("a").display()
        )
    );
    // Marked, so the next prompt does not repeat it.
    let again = run_hook(
        &r.fx,
        &r.hc,
        "user-prompt-submit",
        Some(&r.coordinator),
        &hook_input(SESSION),
    );
    assert_eq!(again.stdout, "");
}

#[test]
fn a_closed_flock_resumes_nothing() {
    let r = rig();
    add(&r, "a", State::Active, true);
    roster::mark_all_done(&r.flock.folder).expect("close");
    let before = runs(&r).len();
    let ran = start(&r, &r.coordinator);
    assert_eq!(runs(&r).len(), before);
    assert_eq!(ran.stdout, "");
}

#[test]
fn nothing_to_say_prints_nothing() {
    let r = rig();
    let ran = start(&r, &r.coordinator);
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
}

#[test]
fn a_missing_plugin_version_is_a_warning_and_the_rest_still_runs() {
    let r = rig();
    r.fx.set_env("CLAUDE_PLUGIN_ROOT", "/nope");
    add(&r, "lost", State::Active, true);
    let ran = start(&r, &r.coordinator);
    assert!(
        ran.stderr.contains("could not record the plugin version"),
        "{}",
        ran.stderr
    );
    assert!(
        ran.context()
            .expect("context")
            .contains("resumed demo.lost")
            || ran.stderr.contains("could not resume")
    );
}

#[test]
fn input_without_a_session_id_still_resumes() {
    let r = rig();
    add(&r, "lost", State::Active, true);
    let ran = run_hook(&r.fx, &r.hc, "session-start", Some(&r.coordinator), "{}");
    assert!(ran.stderr.contains("no session_id"), "{}", ran.stderr);
    assert!(
        ran.context()
            .is_some_and(|c| c.contains("resumed demo.lost"))
    );
}

#[test]
fn a_broken_flock_is_a_warning() {
    let r = rig();
    std::fs::remove_file(r.flock.folder.join("roster.json")).expect("remove");
    let ran = start(&r, &r.coordinator);
    assert_eq!(ran.stdout, "");
    assert!(ran.stderr.contains("broken flock"), "{}", ran.stderr);
}

fn start_from(r: &Rig, source: &str) {
    let input = serde_json::json!({"session_id": SESSION, "source": source}).to_string();
    run_hook(&r.fx, &r.hc, "session-start", Some(&r.coordinator), &input);
}

fn recorded(r: &Rig) -> String {
    version::load(&r.fx.ctx(), SESSION)
        .expect("load")
        .expect("record")
        .version
}

#[test]
fn startup_and_resume_overwrite_the_record() {
    for source in ["startup", "resume"] {
        let r = rig();
        start_from(&r, "startup");
        r.fx.set_plugin_version("1.1.0");
        start_from(&r, source);
        assert_eq!(recorded(&r), "1.1.0", "{source}");
    }
}

#[test]
fn clear_and_compact_never_overwrite_but_create_when_missing() {
    for source in ["clear", "compact", "something-new"] {
        let r = rig();
        start_from(&r, source);
        assert_eq!(recorded(&r), "1.0.0", "{source}: created");
        r.fx.set_plugin_version("1.1.0");
        start_from(&r, source);
        assert_eq!(recorded(&r), "1.0.0", "{source}: kept");
    }
}

const CLEARED: &str = "11111111-2222-4333-8444-555555555555";

fn start_as(r: &Rig, terminal: &str, session: &str, source: &str) {
    let input = serde_json::json!({"session_id": session, "source": source}).to_string();
    run_hook(&r.fx, &r.hc, "session-start", Some(terminal), &input);
}

#[test]
fn a_clear_moves_only_the_session_id_of_that_child() {
    let r = rig();
    add(&r, "a", State::Active, true);
    add(&r, "bb", State::Active, true);
    let before = load_roster(&r.flock);
    let a = live_child(&r, "a");
    start_as(&r, &a, CLEARED, "clear");
    let after = load_roster(&r.flock);
    assert_eq!(after.children[0].session_id, CLEARED);
    assert_eq!(after.children[1].session_id, before.children[1].session_id);
    assert_eq!(after.coordinator.session_id, before.coordinator.session_id);
}

#[test]
fn a_clear_in_the_coordinator_moves_its_session_id() {
    let r = rig();
    start_as(&r, &r.coordinator, CLEARED, "clear");
    assert_eq!(load_roster(&r.flock).coordinator.session_id, CLEARED);
}

#[test]
fn startup_and_resume_never_move_a_session_id() {
    for source in ["startup", "resume", "something-new"] {
        let r = rig();
        add(&r, "a", State::Active, true);
        let before = load_roster(&r.flock);
        let a = live_child(&r, "a");
        start_as(&r, &a, CLEARED, source);
        start_as(&r, &r.coordinator, CLEARED, source);
        let after = load_roster(&r.flock);
        assert_eq!(
            after.children[0].session_id, before.children[0].session_id,
            "{source}"
        );
        assert_eq!(
            after.coordinator.session_id, before.coordinator.session_id,
            "{source}"
        );
    }
}
