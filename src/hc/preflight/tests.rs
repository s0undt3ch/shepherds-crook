use std::ffi::OsString;
use std::time::Duration;

use super::*;
use crate::hc::fake::{Call, FakeHc};

fn vars() -> Vec<(OsString, OsString)> {
    [
        ("PATH", "/usr/bin"),
        ("CLAUDECODE", "1"),
        ("CLAUDE_CODE_SESSION_ID", "abc"),
        ("HC_TERMINAL_ID", "t9"),
        ("HC_SESSION", "scratch"),
        ("CLAUDE_CONFIG_DIR", "/c"),
    ]
    .iter()
    .map(|(k, v)| ((*k).into(), (*v).into()))
    .collect()
}

fn quick() -> Options {
    Options {
        start_deadline: Duration::from_millis(80),
        poll_interval: Duration::from_millis(5),
    }
}

/// For a test that must pass: a long deadline, so a slow runner never turns a late answer into a timeout.
fn patient() -> Options {
    Options {
        start_deadline: Duration::from_secs(30),
        poll_interval: Duration::from_millis(1),
    }
}

fn kinds(hc: &FakeHc) -> Vec<&'static str> {
    hc.calls()
        .iter()
        .map(|c| match c {
            Call::Version => "version",
            Call::ServerStart(_) => "server_start",
            Call::Schema => "schema",
            _ => "other",
        })
        .collect()
}

#[test]
fn the_floor_is_the_one_adr_0016_pins() {
    assert_eq!(MIN_HC_VERSION, "0.1.22");
}

#[test]
fn a_good_hc_passes_with_the_core_started_before_any_core_call() {
    let hc = FakeHc::new();
    run_with(&hc, &vars(), &quick()).expect("passes");
    assert_eq!(kinds(&hc), ["version", "server_start", "schema"]);
}

#[test]
fn the_core_is_started_with_the_scrubbed_environment() {
    let hc = FakeHc::new();
    run_with(&hc, &vars(), &quick()).expect("passes");
    let Call::ServerStart(env) = &hc.calls()[1] else {
        panic!("not a start");
    };
    let names: Vec<&str> = env.iter().filter_map(|(k, _)| k.to_str()).collect();
    assert_eq!(names, ["PATH", "HC_SESSION", "CLAUDE_CONFIG_DIR"]);
}

#[test]
fn an_old_version_is_refused_naming_the_floor_and_starts_nothing() {
    let hc = FakeHc::new().with_version("0.1.21");
    let msg = run_with(&hc, &vars(), &quick())
        .expect_err("refused")
        .to_string();
    assert!(msg.contains("0.1.22"), "{msg}");
    assert!(msg.contains("0.1.21"), "{msg}");
    assert_eq!(kinds(&hc), ["version"]);
}

#[test]
fn newer_versions_pass() {
    for v in [
        "0.1.22",
        "0.1.23",
        "0.1.100",
        "0.2.0-rc.1",
        "0.2.0",
        "1.0.0",
    ] {
        let hc = FakeHc::new().with_version(v);
        run_with(&hc, &vars(), &quick()).unwrap_or_else(|e| panic!("{v}: {e}"));
    }
}

#[test]
fn a_pre_release_of_the_floor_is_too_old() {
    let hc = FakeHc::new().with_version("0.1.22-rc.1");
    assert!(run_with(&hc, &vars(), &quick()).is_err());
}

#[test]
fn a_version_it_cannot_order_is_refused() {
    for v in ["", "banana", "0.1", "v0.2.0"] {
        let hc = FakeHc::new().with_version(v);
        let msg = run_with(&hc, &vars(), &quick()).expect_err(v).to_string();
        assert!(msg.contains("version"), "{v}: {msg}");
        assert!(msg.contains("0.1.22"), "{v}: {msg}");
    }
}

#[test]
fn a_schema_without_labels_on_terminal_run_is_refused() {
    let hc = FakeHc::new().without_run_labels();
    let msg = run_with(&hc, &vars(), &quick())
        .expect_err("refused")
        .to_string();
    assert!(msg.contains("terminal.run"), "{msg}");
    assert!(msg.contains("labels"), "{msg}");
}

#[test]
fn a_schema_without_terminal_run_is_refused() {
    let hc = FakeHc::new().without_run();
    assert!(run_with(&hc, &vars(), &quick()).is_err());
}

#[test]
fn a_core_that_answers_late_is_waited_for() {
    let hc = FakeHc::new();
    hc.fail("schema", 3);
    run_with(&hc, &vars(), &patient()).expect("passes");
    assert_eq!(
        kinds(&hc),
        [
            "version",
            "server_start",
            "schema",
            "server_start",
            "schema",
            "server_start",
            "schema",
            "server_start",
            "schema"
        ]
    );
    let scrubbed: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), "/usr/bin".into()),
        ("HC_SESSION".into(), "scratch".into()),
        ("CLAUDE_CONFIG_DIR".into(), "/c".into()),
    ];
    for call in hc.calls() {
        if let Call::ServerStart(env) = call {
            assert_eq!(env, scrubbed);
        }
    }
}

#[test]
fn a_core_that_never_comes_up_times_out_with_a_clear_message() {
    let hc = FakeHc::new();
    hc.fail("schema", usize::MAX);
    let start = std::time::Instant::now();
    let msg = run_with(&hc, &vars(), &quick())
        .expect_err("times out")
        .to_string();
    assert!(msg.contains("did not come up"), "{msg}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    assert!(kinds(&hc).iter().filter(|k| **k == "schema").count() > 1);
}

#[test]
fn a_core_that_will_not_start_says_so() {
    let hc = FakeHc::new();
    hc.fail("server_start", 1);
    let msg = run_with(&hc, &vars(), &quick())
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("could not start the hc core"), "{msg}");
    assert_eq!(kinds(&hc), ["version", "server_start"]);
}

#[test]
fn a_missing_hc_is_reported_from_the_version_call() {
    let hc = FakeHc::new();
    hc.fail("version", 1);
    assert!(run_with(&hc, &vars(), &quick()).is_err());
    assert_eq!(kinds(&hc), ["version"]);
}

#[test]
fn the_default_options_wait_a_few_seconds() {
    let o = Options::default();
    assert!(o.start_deadline >= Duration::from_secs(5));
    assert!(o.poll_interval <= Duration::from_millis(500));
}
