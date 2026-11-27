use super::*;
use crate::files::result::sha256_hex;
use crate::hc::fake::{Call, FakeHc};
use crate::hook::version::{self, Record};
use crate::testutil::{Fixture, SESSION, hook_input, make_flock, run_hook, start_coordinator};

struct Rig {
    fx: Fixture,
    hc: FakeHc,
    flock: crate::flock::Flock,
    coordinator: String,
    child: String,
}

const TEXT: &str = "---\nstatus: done\n---\nit works\n";

fn rig() -> Rig {
    let fx = Fixture::new();
    fx.set_plugin_version("1.0.0");
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    hc.set_app(&coordinator, "claude-code");
    use crate::hc::{Hc, Parent, RunSpec};
    let child = hc
        .run(&RunSpec {
            name: "demo.a".into(),
            cwd: flock.folder.clone(),
            labels: vec![
                ("crook.flock".into(), "demo".into()),
                ("crook.child".into(), "a".into()),
            ],
            parent: Parent::Terminal(coordinator.clone()),
            argv: vec!["claude".into()],
        })
        .expect("run")
        .address;
    hc.set_app(&child, "claude-code");
    version::record_start(&fx.ctx(), SESSION, true).expect("record");
    Rig {
        fx,
        hc,
        flock,
        coordinator,
        child,
    }
}

fn stop(r: &Rig, terminal: &str) -> crate::testutil::HookRun {
    run_hook(&r.fx, &r.hc, "stop", Some(terminal), &hook_input(SESSION))
}

fn sends(r: &Rig) -> Vec<(String, String)> {
    r.hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Send { to, text } => Some((to, text)),
            _ => None,
        })
        .collect()
}

fn mark_of(r: &Rig) -> Option<String> {
    marks::load(&r.flock.folder)
        .expect("marks")
        .get("a")
        .cloned()
}

fn write(r: &Rig, text: &str) {
    std::fs::write(r.flock.result_path("a"), text).expect("result");
}

/// What the coordinator is sent: spike 14's prefix, never a bare path (D69).
fn result_path(r: &Rig) -> String {
    format!("Result updated: {}", r.flock.result_path("a").display())
}

#[test]
fn a_child_with_no_result_sends_nothing() {
    let r = rig();
    let ran = stop(&r, &r.child);
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert_eq!(sends(&r), vec![]);
}

#[test]
fn an_unchanged_result_sends_nothing() {
    let r = rig();
    write(&r, TEXT);
    marks::mark(&r.flock.folder, "a", &sha256_hex(TEXT.as_bytes())).expect("mark");
    stop(&r, &r.child);
    assert_eq!(sends(&r), vec![]);
    // No parent lookup either: step 1 ends it (0009).
    assert!(!r.hc.calls().contains(&Call::Get(r.coordinator.clone())));
}

#[test]
fn a_changed_result_sends_its_path_and_marks_it() {
    let r = rig();
    write(&r, TEXT);
    let ran = stop(&r, &r.child);
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert_eq!(sends(&r), vec![(r.coordinator.clone(), result_path(&r))]);
    assert_eq!(mark_of(&r), Some(sha256_hex(TEXT.as_bytes())));
    // The next turn changes nothing, so nothing more is sent.
    stop(&r, &r.child);
    assert_eq!(sends(&r).len(), 1);
    // A rewrite is sent again.
    write(&r, "---\nstatus: done\n---\nmore\n");
    stop(&r, &r.child);
    assert_eq!(sends(&r).len(), 2);
}

#[test]
fn a_parent_that_is_not_claude_code_gets_nothing_and_keeps_the_mark_old() {
    let r = rig();
    write(&r, TEXT);
    r.hc.set_app(&r.coordinator, "something-else");
    stop(&r, &r.child);
    assert_eq!(sends(&r), vec![]);
    assert_eq!(mark_of(&r), None);
    // A coordinator that has ended.
    let fx2 = rig();
    write(&fx2, TEXT);
    fx2.hc.exit(&fx2.coordinator, 0);
    stop(&fx2, &fx2.child);
    assert_eq!(sends(&fx2), vec![]);
    assert_eq!(mark_of(&fx2), None);
}

#[test]
fn each_failing_send_outcome_keeps_the_mark() {
    let r = rig();
    write(&r, TEXT);
    for outcome in [
        SendOutcome::Blocked,
        SendOutcome::Finished,
        SendOutcome::NotFound,
        SendOutcome::Failed("x".into()),
    ] {
        r.hc.force_send(outcome);
        let ran = stop(&r, &r.child);
        assert_eq!(mark_of(&r), None);
        assert!(ran.stderr.contains("was not delivered"), "{}", ran.stderr);
        assert_eq!(ran.stdout, "");
    }
}

#[test]
fn a_send_that_errors_keeps_the_mark() {
    let r = rig();
    write(&r, TEXT);
    r.hc.fail("send", 1);
    let ran = stop(&r, &r.child);
    assert_eq!(mark_of(&r), None);
    assert!(
        ran.stderr.contains("could not report the result"),
        "{}",
        ran.stderr
    );
}

#[test]
fn the_reload_still_runs_after_a_failed_send() {
    let r = rig();
    write(&r, TEXT);
    r.fx.set_plugin_version("1.1.0");
    r.hc.fail("send", 1);
    stop(&r, &r.child);
    // The first send (the result) failed; the second (the reload) went through.
    assert_eq!(
        sends(&r),
        vec![
            (r.coordinator.clone(), result_path(&r)),
            (r.child.clone(), "/reload-plugins".into())
        ]
    );
    assert_eq!(
        version::load(&r.fx.ctx(), SESSION).expect("load"),
        Some(Record {
            version: "1.1.0".into(),
            reloaded: true
        })
    );
    assert_eq!(mark_of(&r), None);
}

#[test]
fn a_broken_flock_does_not_stop_the_reload() {
    let r = rig();
    std::fs::remove_file(r.flock.folder.join("roster.json")).expect("remove");
    r.fx.set_plugin_version("1.1.0");
    let ran = stop(&r, &r.child);
    assert!(
        ran.stderr.contains("could not report the result"),
        "{}",
        ran.stderr
    );
    assert_eq!(sends(&r), vec![(r.child.clone(), "/reload-plugins".into())]);
}

#[test]
fn the_coordinator_does_only_the_reload() {
    let r = rig();
    write(&r, TEXT);
    stop(&r, &r.coordinator);
    assert_eq!(sends(&r), vec![]);
    r.fx.set_plugin_version("1.1.0");
    stop(&r, &r.coordinator);
    assert_eq!(
        sends(&r),
        vec![(r.coordinator.clone(), "/reload-plugins".into())]
    );
    assert_eq!(mark_of(&r), None);
}

#[test]
fn a_stop_never_prints_a_decision() {
    let r = rig();
    write(&r, TEXT);
    r.fx.set_plugin_version("1.1.0");
    for _ in 0..2 {
        assert_eq!(stop(&r, &r.child).stdout, "");
    }
}

#[test]
fn a_text_that_starts_with_a_slash_is_never_sent() {
    let r = rig();
    write(&r, TEXT);
    stop(&r, &r.child);
    let sent = sends(&r);
    assert!(sent[0].1.starts_with("Result updated: /"), "{sent:?}");
}

fn child_of(r: &Rig, parent: &str, name: &str) -> String {
    use crate::hc::{Hc, Parent, RunSpec};
    r.hc.run(&RunSpec {
        name: name.into(),
        cwd: r.flock.folder.clone(),
        labels: vec![
            ("crook.flock".into(), "demo".into()),
            ("crook.child".into(), "a".into()),
        ],
        parent: Parent::Terminal(parent.into()),
        argv: vec!["claude".into()],
    })
    .expect("run")
    .address
}

#[test]
fn a_live_bare_shell_parent_gets_nothing_even_with_the_coordinators_labels() {
    // D52: a core restore leaves a shell under the coordinator's name and labels.
    let r = rig();
    write(&r, TEXT);
    use crate::hc::{Hc, Parent, RunSpec};
    let shell =
        r.hc.run(&RunSpec {
            name: "demo-shell".into(),
            cwd: r.flock.folder.clone(),
            labels: vec![("crook.flock".into(), "demo".into())],
            parent: Parent::None,
            argv: vec!["sh".into()],
        })
        .expect("run")
        .address;
    let child = child_of(&r, &shell, "demo.a2");
    r.hc.set_app(&child, "claude-code");
    stop(&r, &child);
    assert_eq!(sends(&r), vec![]);
    assert_eq!(mark_of(&r), None);
}

#[test]
fn a_parent_of_another_flock_or_a_child_terminal_gets_nothing() {
    let r = rig();
    write(&r, TEXT);
    use crate::hc::{Hc, Parent, RunSpec};
    let spawn = |labels: Vec<(String, String)>, name: &str| {
        let t =
            r.hc.run(&RunSpec {
                name: name.into(),
                cwd: r.flock.folder.clone(),
                labels,
                parent: Parent::None,
                argv: vec!["claude".into()],
            })
            .expect("run")
            .address;
        r.hc.set_app(&t, "claude-code");
        t
    };
    let other = spawn(vec![("crook.flock".into(), "other".into())], "other");
    let sibling = spawn(
        vec![
            ("crook.flock".into(), "demo".into()),
            ("crook.child".into(), "z".into()),
        ],
        "demo.z",
    );
    for (i, parent) in [other, sibling].iter().enumerate() {
        let child = child_of(&r, parent, &format!("demo.k{i}"));
        r.hc.set_app(&child, "claude-code");
        stop(&r, &child);
    }
    assert_eq!(sends(&r), vec![]);
}

#[test]
fn a_corrupt_marks_file_does_not_stop_delivery_and_is_replaced() {
    let r = rig();
    write(&r, TEXT);
    std::fs::write(r.flock.folder.join("results/.delivered.json"), "garbage").expect("write");
    let ran = stop(&r, &r.child);
    assert_eq!(
        ran.stderr.matches("crook hook:").count(),
        1,
        "{}",
        ran.stderr
    );
    assert_eq!(sends(&r), vec![(r.coordinator.clone(), result_path(&r))]);
    assert_eq!(mark_of(&r), Some(sha256_hex(TEXT.as_bytes())));
}
