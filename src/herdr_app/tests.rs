use std::time::Duration;

use super::*;
use crate::run::Output;
use crate::run::fake::FakeRunner;

fn quick() -> Options {
    Options {
        interval: Duration::from_millis(5),
        budget: Duration::from_millis(60),
    }
}

fn ok() -> Result<Output, Error> {
    Ok(Output::success("{\"ok\": true}\n"))
}

fn failing() -> Result<Output, Error> {
    Ok(Output {
        code: Some(1),
        stdout: String::new(),
        stderr: "no such space".to_string(),
    })
}

fn missing() -> Result<Output, Error> {
    Err(Error::Spawn {
        program: "herdr-app".to_string(),
        source: std::io::Error::other("not found"),
    })
}

#[test]
fn renames_the_space_then_the_tab_with_json_arguments() {
    let runner = FakeRunner::new([ok(), ok()]);
    assert_eq!(rename_coordinator_with(&runner, "demo", &quick()), None);
    let calls = runner.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].program, "herdr-app");
    assert_eq!(
        calls[0].args,
        ["do", "space.rename", r#"{"id":"crook-demo","name":"demo"}"#]
    );
    assert_eq!(
        calls[1].args,
        [
            "do",
            "tab.rename",
            r#"{"id":"crook-demo-coord","name":"coordinator"}"#
        ]
    );
}

#[test]
fn retries_until_the_space_exists() {
    let runner = FakeRunner::new([failing(), failing(), ok(), ok()]);
    assert_eq!(rename_coordinator_with(&runner, "demo", &quick()), None);
    assert_eq!(runner.calls().len(), 4);
}

#[test]
fn a_refusal_in_the_json_counts_as_a_failure() {
    let refused = Ok(Output::success("{\"ok\": false}"));
    let runner = FakeRunner::new([refused, ok(), ok()]);
    assert_eq!(rename_coordinator_with(&runner, "demo", &quick()), None);
    assert_eq!(runner.calls().len(), 3);
}

#[test]
fn a_missing_app_is_a_warning_after_one_try() {
    let runner = FakeRunner::new([missing()]);
    let warning = rename_coordinator_with(&runner, "demo", &quick()).expect("warns");
    assert!(warning.contains("not on PATH"), "{warning}");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn an_app_that_keeps_failing_is_a_warning_once_the_budget_is_spent() {
    let runner = FakeRunner::new([failing(), failing(), failing(), failing()]);
    let started = std::time::Instant::now();
    let warning = rename_coordinator_with(&runner, "demo", &quick()).expect("warns");
    assert!(warning.contains("space.rename"), "{warning}");
    assert!(runner.calls().len() >= 2, "it retried");
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "and gave up"
    );
}

#[test]
fn a_timeout_is_a_warning_after_one_try() {
    let timeout = Err(Error::Timeout {
        program: "herdr-app".to_string(),
        after: Duration::from_secs(3),
    });
    let runner = FakeRunner::new([timeout]);
    let warning = rename_coordinator_with(&runner, "demo", &quick()).expect("warns");
    assert!(warning.contains("timed out"), "{warning}");
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_tab_that_fails_after_the_space_worked_still_warns() {
    let runner = FakeRunner::new([ok(), failing(), failing(), failing(), failing()]);
    let warning = rename_coordinator_with(&runner, "demo", &quick()).expect("warns");
    assert!(warning.contains("tab.rename"));
}
