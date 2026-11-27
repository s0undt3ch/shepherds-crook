use super::*;
use crate::hc::fake::{Call, FakeHc};
use crate::hc::{Hc, Parent, RunSpec};
use crate::testutil::{Buffers, COORD_SID, Fixture, load_roster, make_flock};

fn runs(hc: &FakeHc) -> Vec<RunSpec> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Run(spec) => Some(spec),
            _ => None,
        })
        .collect()
}

fn open(fx: &Fixture, hc: &FakeHc, bufs: &mut Buffers, task: Option<&str>) -> Result<(), Error> {
    run(&fx.ctx(), hc, &mut bufs.io(), "demo", task)
}

#[test]
fn the_first_open_uses_session_id_sets_started_and_renames() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("opens");

    let spec = &runs(&hc)[0];
    let at = spec
        .argv
        .iter()
        .position(|a| a == "--session-id")
        .expect("flag");
    assert_eq!(spec.argv[at + 1], COORD_SID);
    assert!(!spec.argv.contains(&"--resume".to_string()));
    assert!(load_roster(&flock).coordinator.started);
    assert_eq!(bufs.out.text(), "opened flock demo (terminal t1)\n");
    assert_eq!(bufs.err.text(), "");
    let herdr = fx.runner.herdr_calls();
    assert_eq!(herdr.len(), 2);
    assert_eq!(herdr[0].args[1], "space.rename");
    assert_eq!(herdr[1].args[1], "tab.rename");
}

#[test]
fn preflight_comes_first() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    let calls = hc.calls();
    assert_eq!(calls[0], Call::Version);
    assert!(matches!(calls[1], Call::ServerStart(_)));
    assert_eq!(calls[2], Call::Schema);
}

#[test]
fn a_later_open_resumes_with_no_prompt() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    hc.kill("demo").expect("kill");
    open(&fx, &hc, &mut Buffers::default(), Some("ignored")).expect("opens again");

    let second = &runs(&hc)[1];
    let at = second
        .argv
        .iter()
        .position(|a| a == "--resume")
        .expect("flag");
    assert_eq!(second.argv[at + 1], COORD_SID);
    assert_eq!(second.argv.len(), at + 2, "a resume has no prompt");
    assert!(!second.argv.contains(&"--session-id".to_string()));
}

#[test]
fn a_failed_launch_leaves_started_false() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    hc.fail("run", 1);
    let err = open(&fx, &hc, &mut Buffers::default(), None).expect_err("fails");
    assert!(matches!(err, Error::Hc { .. }));
    assert!(!load_roster(&flock).coordinator.started);
    assert!(fx.runner.herdr_calls().is_empty());
    // The retry is a first start again.
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    assert!(runs(&hc)[1].argv.contains(&"--session-id".to_string()));
}

#[test]
fn the_launch_carries_crook_bin_settings_labels_and_no_parent() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    let spec = &runs(&hc)[0];
    assert_eq!(spec.name, "demo");
    assert_eq!(spec.parent, Parent::None);
    let own = fx.root().join("bin/crook");
    // A debug build with no CROOK_BIN passes its own path (Q19).
    assert_eq!(
        spec.argv[..2],
        ["env".to_string(), format!("CROOK_BIN={}", own.display())]
    );
    assert!(spec.argv.contains(&"--settings".to_string()));
    let label = |k: &str| {
        spec.labels
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(label("crook.flock"), Some("demo"));
    assert_eq!(label("crook.version"), Some("0.0.0-dev"));
    assert_eq!(label("herdr.space"), Some("crook-demo"));
    assert_eq!(label("herdr.tab"), Some("crook-demo-coord"));
}

#[test]
fn crook_bin_from_the_environment_wins() {
    let fx = Fixture::new();
    fx.set_env("CROOK_BIN", "/opt/crook");
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    assert_eq!(runs(&hc)[0].argv[1], "CROOK_BIN=/opt/crook");
}

#[test]
fn a_live_coordinator_starts_nothing_and_only_renames() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("second open");
    assert_eq!(runs(&hc).len(), 1);
    assert!(bufs.out.text().contains("already open (terminal t1)"));
    assert_eq!(fx.runner.herdr_calls().len(), 4, "renamed again");
}

#[test]
fn a_closed_flock_is_refused() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    crate::files::roster::mark_all_done(&flock.folder).expect("close");
    let hc = FakeHc::new();
    let err = open(&fx, &hc, &mut Buffers::default(), None).expect_err("refused");
    assert!(err.to_string().contains("closed"), "{err}");
    assert!(runs(&hc).is_empty());
}

#[test]
fn an_unknown_slug_is_refused_with_the_slugs_that_exist() {
    let fx = Fixture::new();
    make_flock(&fx, "alpha");
    make_flock(&fx, "beta");
    let hc = FakeHc::new();
    let err = open(&fx, &hc, &mut Buffers::default(), None).expect_err("refused");
    let msg = err.to_string();
    assert!(
        msg.contains("demo") && msg.contains("alpha") && msg.contains("beta"),
        "{msg}"
    );
    assert!(hc.calls().is_empty());
}

#[test]
fn the_first_task_reaches_the_prompt() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), Some("fix the bug")).expect("opens");
    let prompt = runs(&hc)[0].argv.last().cloned().expect("prompt");
    assert!(prompt.ends_with("fix the bug"), "{prompt}");
}

#[test]
fn the_config_sets_the_coordinator_model_and_the_notes_folder() {
    let fx = Fixture::new();
    let notes = fx.root().join("notes");
    fx.write_config(&format!(
        "coordinator_model = \"haiku\"\nnotes_root = \"{}\"\n",
        notes.display()
    ));
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    open(&fx, &hc, &mut Buffers::default(), None).expect("opens");
    let argv = &runs(&hc)[0].argv;
    let at = argv.iter().position(|a| a == "--model").expect("model");
    assert_eq!(argv[at + 1], "haiku");
    let adds: Vec<&String> = argv
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--add-dir")
        .map(|(i, _)| &argv[i + 1])
        .collect();
    assert_eq!(
        adds,
        [
            &flock.folder.display().to_string(),
            &notes.join("2026-11-27-demo").display().to_string()
        ]
    );
}

#[test]
fn a_missing_herdr_app_is_a_warning_and_the_open_still_succeeds() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    fx.runner.remove_herdr();
    let hc = FakeHc::new();
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("opens");
    assert!(
        bufs.err.text().starts_with("warning: "),
        "{}",
        bufs.err.text()
    );
    assert_eq!(bufs.out.text(), "opened flock demo (terminal t1)\n");
    assert!(load_roster(&flock).coordinator.started);
}

#[test]
fn herdr_app_that_is_not_ready_yet_is_retried() {
    let fx = Fixture::new();
    make_flock(&fx, "demo");
    fx.runner.fail_herdr(1);
    let hc = FakeHc::new();
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("opens");
    assert_eq!(bufs.err.text(), "");
    assert_eq!(
        fx.runner.herdr_calls().len(),
        3,
        "space twice, then the tab"
    );
}

#[test]
fn a_timeout_with_the_coordinator_live_counts_as_opened() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    hc.time_out_next_run(true);
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("it is running");
    assert!(
        bufs.err.text().contains("coordinator is live"),
        "{}",
        bufs.err.text()
    );
    assert!(load_roster(&flock).coordinator.started);
}

#[test]
fn a_timeout_with_nothing_live_is_an_error_and_started_stays_false() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    hc.time_out_next_run(false);
    let err = open(&fx, &hc, &mut Buffers::default(), None).expect_err("fails");
    assert!(matches!(err, Error::Timeout { .. }));
    assert!(!load_roster(&flock).coordinator.started);
}

#[test]
fn finding_the_coordinator_live_marks_it_started() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    crate::testutil::start_coordinator(&hc, &flock);
    assert!(!load_roster(&flock).coordinator.started);
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("fine");
    assert!(load_roster(&flock).coordinator.started);
    assert!(bufs.out.text().contains("renamed its space and tab"));
}

#[test]
fn a_rename_that_warned_is_not_claimed() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    crate::testutil::start_coordinator(&hc, &flock);
    fx.runner.remove_herdr();
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("fine");
    assert!(!bufs.out.text().contains("renamed"), "{}", bufs.out.text());
    assert!(bufs.err.text().starts_with("warning: "));
}

fn start_shell(hc: &FakeHc, flock: &crate::flock::Flock, name: &str, program: &str) -> String {
    hc.run(&RunSpec {
        name: name.to_string(),
        cwd: flock.folder.clone(),
        labels: vec![("crook.flock".to_string(), "demo".to_string())],
        parent: Parent::None,
        argv: vec![program.to_string()],
    })
    .expect("terminal")
    .address
}

#[test]
fn a_restored_shell_with_the_coordinators_name_is_replaced_not_treated_as_it() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let shell = start_shell(&hc, &flock, "demo", "bash");
    let mut bufs = Buffers::default();
    open(&fx, &hc, &mut bufs, None).expect("opens");
    assert!(hc.calls().contains(&Call::Kill(shell.clone())));
    assert_eq!(runs(&hc).len(), 2, "the shell, then the real coordinator");
    assert!(
        bufs.err.text().contains("runs a shell"),
        "{}",
        bufs.err.text()
    );
    assert!(bufs.out.text().starts_with("opened flock demo"));
}

#[test]
fn a_live_agent_under_the_name_is_left_alone_even_when_launched_through_env() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    // `env CROOK_BIN=… claude` shows as `env` until the session reports itself.
    for program in ["claude", "env"] {
        let live = start_shell(&hc, &flock, "demo", program);
        open(&fx, &hc, &mut Buffers::default(), None).expect("fine");
        assert!(!hc.calls().contains(&Call::Kill(live.clone())), "{program}");
        hc.kill(&live).expect("clean up");
    }
}
