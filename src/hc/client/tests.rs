use std::time::{Duration, Instant};

use super::*;
use crate::hc::fixtures::fixture;
use crate::run::fake::FakeRunner;
use crate::run::{Cmd, Output, RealRunner};

fn runner(names: &[&str]) -> FakeRunner {
    FakeRunner::new(names.iter().map(|n| Ok(fixture(n))))
}

fn hc(args: &[&str]) -> Cmd {
    Cmd::new("hc").args(args.iter().copied())
}

fn kv(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

fn os(k: &str, v: &str) -> (std::ffi::OsString, std::ffi::OsString) {
    (k.into(), v.into())
}

#[test]
fn version_reads_the_number_after_the_program_name() {
    let r = runner(&["version"]);
    assert_eq!(HcClient::new(&r).version().expect("version"), "0.1.22");
    assert_eq!(r.calls(), [hc(&["--version"])]);
}

#[test]
fn a_version_line_it_cannot_read_is_an_error() {
    let r = FakeRunner::new([Ok(Output::success("hello\n"))]);
    assert!(HcClient::new(&r).version().is_err());
}

#[test]
fn schema_shows_whether_terminal_run_takes_labels() {
    let r = runner(&["schema"]);
    let schema = HcClient::new(&r).schema().expect("schema");
    assert_eq!(r.calls(), [hc(&["schema"])]);
    assert!(schema.has_param("terminal.run", "labels"));
    assert!(schema.has_param("terminal.run", "outlive_parent"));
    assert!(!schema.has_param("terminal.run", "no_such_param"));
    assert!(!schema.has_param("no.such.method", "labels"));
}

#[test]
fn ls_parses_the_summary_the_real_core_prints() {
    let r = runner(&["ls"]);
    let list = HcClient::new(&r).ls(false).expect("ls");
    assert_eq!(r.calls(), [hc(&["ls", "--json"])]);
    assert_eq!(list.len(), 3);

    let a = &list[0];
    assert_eq!(a.address, "t1");
    assert_eq!(a.name.as_deref(), Some("worker-a"));
    assert_eq!(a.state, TermState::Live);
    assert_eq!(a.parent, None);
    assert_eq!(a.status, None);
    assert_eq!(a.agent.as_deref(), Some("sh"));
    assert!(a.labels.is_empty(), "ls carries no labels");

    let b = &list[1];
    assert_eq!(b.parent.as_deref(), Some("t1"));
    assert_eq!(b.agent.as_deref(), Some("claude"));
    assert_eq!(
        b.status,
        Some(Status {
            state: "blocked".to_string(),
            kind: None,
            app: None
        })
    );

    assert_eq!(list[2].name.as_deref(), Some("demo.child"));
}

#[test]
fn ls_all_includes_the_finished_with_how_they_ended() {
    let r = runner(&["ls_all"]);
    let list = HcClient::new(&r).ls(true).expect("ls");
    assert_eq!(r.calls(), [hc(&["ls", "--json", "--all"])]);
    assert_eq!(list.len(), 4);
    let killed = &list[0];
    assert_eq!(killed.state, TermState::Finished);
    assert_eq!(killed.reason.as_deref(), Some("killed"));
    assert_eq!(killed.signal, Some(1));
    let exited = &list[2];
    assert_eq!(exited.state, TermState::Finished);
    assert_eq!(exited.exit_code, Some(3));
    assert_eq!(exited.reason.as_deref(), Some("exited"));
    assert!(list[3].state == TermState::Live);
}

#[test]
fn an_empty_core_lists_nothing() {
    let r = runner(&["ls_empty"]);
    assert!(HcClient::new(&r).ls(false).expect("ls").is_empty());
}

#[test]
fn ls_by_label_passes_every_label() {
    let r = runner(&["ls_label"]);
    let list = HcClient::new(&r)
        .ls_by_label(false, &[kv("crook.flock", "demo"), kv("crook.child", "b")])
        .expect("ls");
    assert_eq!(
        r.calls(),
        [hc(&[
            "ls",
            "--json",
            "--label",
            "crook.flock=demo",
            "--label",
            "crook.child=b"
        ])]
    );
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name.as_deref(), Some("worker-b"));
}

#[test]
fn get_of_a_running_terminal_has_its_labels() {
    let r = runner(&["get_running"]);
    let t = HcClient::new(&r)
        .get("worker-a")
        .expect("get")
        .expect("found");
    assert_eq!(r.calls(), [hc(&["get", "--json", "worker-a"])]);
    assert_eq!(t.address, "t1");
    assert_eq!(t.state, TermState::Live);
    assert_eq!(t.parent, None);
    assert_eq!(t.status, None);
    assert_eq!(
        t.labels.get("crook.flock").map(String::as_str),
        Some("demo")
    );
    assert_eq!(t.labels.get("crook.child").map(String::as_str), Some("a"));
    assert_eq!(
        t.labels.get("crook.version").map(String::as_str),
        Some("0.0.0-dev")
    );
}

#[test]
fn get_of_a_blocked_terminal_has_the_status_record() {
    let r = runner(&["get_blocked"]);
    let t = HcClient::new(&r).get("t2").expect("get").expect("found");
    assert_eq!(t.parent.as_deref(), Some("t1"));
    assert_eq!(
        t.status,
        Some(Status {
            state: "blocked".to_string(),
            kind: Some("question".to_string()),
            app: Some("claude".to_string())
        })
    );
}

#[test]
fn get_of_a_finished_terminal_has_its_exit() {
    let r = runner(&["get_finished"]);
    let t = HcClient::new(&r)
        .get("worker-c")
        .expect("get")
        .expect("found");
    assert_eq!(t.state, TermState::Finished);
    assert_eq!(t.exit_code, Some(3));
    assert_eq!(t.status, None);
}

#[test]
fn get_of_a_killed_terminal_has_reason_and_signal() {
    let r = runner(&["get_killed"]);
    let t = HcClient::new(&r)
        .get("worker-a")
        .expect("get")
        .expect("found");
    assert_eq!(t.state, TermState::Finished);
    assert_eq!(t.reason.as_deref(), Some("killed"));
    assert_eq!(t.signal, Some(1));
}

#[test]
fn a_name_with_a_dot_comes_back_whole() {
    let r = runner(&["get_dot"]);
    let t = HcClient::new(&r)
        .get("demo.child")
        .expect("get")
        .expect("found");
    assert_eq!(t.name.as_deref(), Some("demo.child"));
}

#[test]
fn get_of_a_missing_terminal_is_none() {
    let r = runner(&["get_missing"]);
    assert_eq!(HcClient::new(&r).get("nosuch").expect("get"), None);
}

#[test]
fn get_that_fails_otherwise_is_an_error_with_hcs_message() {
    let r = FakeRunner::new([Ok(Output {
        code: Some(1),
        stdout: String::new(),
        stderr: r#"{"error":{"kind":"unavailable","message":"the core is starting"}}"#.to_string(),
    })]);
    let err = HcClient::new(&r).get("t1").expect_err("fails");
    let msg = err.to_string();
    assert!(msg.contains("the core is starting"), "{msg}");
    assert!(msg.contains("get"), "{msg}");
    assert!(matches!(err, Error::Hc { kind: Some(k), .. } if k == "unavailable"));
}

#[test]
fn get_output_that_is_not_a_terminal_is_an_error_not_a_panic() {
    for stdout in ["", "nope", "[]", r#"{"name":"x"}"#] {
        let r = FakeRunner::new([Ok(Output::success(stdout))]);
        assert!(HcClient::new(&r).get("t1").is_err(), "{stdout:?}");
    }
}

#[test]
fn an_id_that_looks_like_a_flag_is_refused_before_hc_runs() {
    let r = FakeRunner::new([]);
    let c = HcClient::new(&r);
    assert!(c.get("--json").is_err());
    assert!(c.kill("-x").is_err());
    assert!(c.send("-x", "hi").is_err());
    assert!(r.calls().is_empty());
}

fn spec() -> RunSpec {
    RunSpec {
        name: "worker-a".to_string(),
        cwd: "/work".into(),
        labels: vec![kv("crook.flock", "demo"), kv("crook.child", "a")],
        parent: Parent::None,
        argv: ["sh", "-c", "sleep 30"].map(String::from).to_vec(),
    }
}

#[test]
fn run_builds_the_launch_argv_and_parses_the_new_terminal() {
    let r = runner(&["run_labels"]);
    let t = HcClient::new(&r).run(&spec()).expect("run");
    assert_eq!(
        r.calls(),
        [hc(&[
            "run",
            "--bg",
            "--json",
            "--name",
            "worker-a",
            "--cwd",
            "/work",
            "--label",
            "crook.flock=demo",
            "--label",
            "crook.child=a",
            "--no-parent",
            "--",
            "sh",
            "-c",
            "sleep 30"
        ])]
    );
    assert_eq!(t.address, "t1");
    assert_eq!(t.name.as_deref(), Some("worker-a"));
    assert_eq!(t.state, TermState::Live);
}

#[test]
fn run_with_a_parent_passes_it() {
    let r = runner(&["run_blocked"]);
    let mut s = spec();
    s.parent = Parent::Terminal("t1".to_string());
    HcClient::new(&r).run(&s).expect("run");
    let args = r.calls().remove(0).args;
    let at = args.iter().position(|a| a == "--parent").expect("--parent");
    assert_eq!(args[at + 1], "t1");
    assert!(!args.contains(&"--no-parent".to_string()));
}

#[test]
fn a_run_with_a_dot_in_the_name_works() {
    let r = runner(&["run_name_dot"]);
    let mut s = spec();
    s.name = "demo.child".to_string();
    let t = HcClient::new(&r).run(&s).expect("hc accepts a dot");
    assert_eq!(t.name.as_deref(), Some("demo.child"));
}

#[test]
fn run_failures_carry_hcs_kind_and_message() {
    for (fx, kind, text) in [
        ("run_name_long", "invalid_params", "bad name"),
        ("run_name_taken", "name_taken", "already named"),
        ("run_missing_cwd", "spawn_failed", "cannot start"),
    ] {
        let r = runner(&[fx]);
        let err = HcClient::new(&r).run(&spec()).expect_err(fx);
        let msg = err.to_string();
        assert!(msg.contains(text), "{fx}: {msg}");
        assert!(
            matches!(&err, Error::Hc { kind: Some(k), .. } if k == kind),
            "{fx}: {err:?}"
        );
    }
}

#[test]
fn send_maps_each_exit_code_of_spike_2() {
    for (fx, want) in [
        ("send_sent", SendOutcome::Sent),
        ("send_blocked", SendOutcome::Blocked),
        ("send_finished", SendOutcome::Finished),
        ("send_missing", SendOutcome::NotFound),
    ] {
        let r = runner(&[fx]);
        let got = HcClient::new(&r).send("worker-a", "hello").expect(fx);
        assert_eq!(got, want, "{fx}");
        assert_eq!(
            r.calls(),
            [hc(&["send", "--json", "--", "worker-a", "hello"])]
        );
    }
}

#[test]
fn send_text_that_starts_with_a_dash_stays_text() {
    let r = runner(&["send_sent"]);
    HcClient::new(&r).send("t1", "--force").expect("send");
    assert_eq!(r.calls(), [hc(&["send", "--json", "--", "t1", "--force"])]);
}

#[test]
fn send_with_any_other_exit_is_failed_with_the_message() {
    for code in [Some(1), Some(2), Some(6), None] {
        let r = FakeRunner::new([Ok(Output {
            code,
            stdout: String::new(),
            stderr: r#"{"error":{"kind":"usage","message":"bad call"}}"#.to_string(),
        })]);
        match HcClient::new(&r).send("t1", "x").expect("an outcome") {
            SendOutcome::Failed(msg) => assert!(msg.contains("bad call"), "{msg}"),
            other => panic!("{code:?}: {other:?}"),
        }
    }
}

#[test]
fn a_send_that_times_out_is_an_error() {
    let r = FakeRunner::new([Err(Error::Timeout {
        program: "hc".to_string(),
        after: Duration::from_secs(10),
    })]);
    assert!(matches!(
        HcClient::new(&r).send("t1", "x"),
        Err(Error::Timeout { .. })
    ));
}

#[test]
fn kill_reports_killed_or_not_found() {
    let r = runner(&["kill"]);
    assert_eq!(
        HcClient::new(&r).kill("worker-a").expect("kill"),
        KillOutcome::Killed
    );
    assert_eq!(r.calls(), [hc(&["kill", "--json", "worker-a"])]);

    let r = runner(&["kill_finished"]);
    assert_eq!(
        HcClient::new(&r).kill("worker-a").expect("kill"),
        KillOutcome::Killed
    );

    let r = runner(&["kill_missing"]);
    assert_eq!(
        HcClient::new(&r).kill("nosuch").expect("kill"),
        KillOutcome::NotFound
    );
}

#[test]
fn a_kill_that_fails_otherwise_is_an_error() {
    let r = FakeRunner::new([Ok(Output {
        code: Some(1),
        stdout: String::new(),
        stderr: "boom".to_string(),
    })]);
    let err = HcClient::new(&r).kill("t1").expect_err("fails");
    assert!(err.to_string().contains("boom"), "{err}");
}

#[test]
fn server_start_runs_with_exactly_the_environment_it_is_given() {
    let r = runner(&["server_start_cold"]);
    let env = vec![os("PATH", "/bin"), os("HC_SESSION", "scratch")];
    HcClient::new(&r).server_start(&env).expect("start");
    assert_eq!(
        r.calls(),
        [hc(&["server", "start", "--json"]).env(env.clone())]
    );
}

#[test]
fn server_start_that_fails_is_an_error() {
    let r = FakeRunner::new([Ok(Output {
        code: Some(1),
        stdout: String::new(),
        stderr: "no socket".to_string(),
    })]);
    let err = HcClient::new(&r).server_start(&[]).expect_err("fails");
    assert!(err.to_string().contains("no socket"), "{err}");
}

#[test]
fn every_call_has_the_same_ten_second_timeout() {
    assert_eq!(CALL_TIMEOUT, Duration::from_secs(10));
    let r = FakeRunner::new([
        Ok(fixture("version")),
        Ok(fixture("schema")),
        Ok(fixture("ls")),
        Ok(fixture("get_running")),
        Ok(fixture("run_labels")),
        Ok(fixture("send_sent")),
        Ok(fixture("kill")),
        Ok(fixture("server_start_cold")),
    ]);
    let c = HcClient::new(&r);
    c.version().expect("version");
    c.schema().expect("schema");
    c.ls(false).expect("ls");
    c.get("t1").expect("get");
    c.run(&spec()).expect("run");
    c.send("t1", "x").expect("send");
    c.kill("t1").expect("kill");
    c.server_start(&[]).expect("start");
    assert_eq!(r.timeouts(), vec![CALL_TIMEOUT; 8]);
}

#[cfg(unix)]
#[test]
fn a_hung_hc_becomes_a_timeout_error_not_a_hang() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("hc");
    std::fs::write(&script, "#!/bin/sh\nexec sleep 30\n").expect("write");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let client = HcClient::new(&RealRunner)
        .with_program(script.display().to_string())
        .with_timeout(Duration::from_millis(200));
    let start = Instant::now();
    let err = client.ls(false).expect_err("times out");
    assert!(matches!(err, Error::Timeout { .. }), "{err}");
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_missing_hc_is_a_clear_error() {
    let client = HcClient::new(&RealRunner).with_program("/nonexistent/hc".to_string());
    let err = client.version().expect_err("fails");
    assert!(err.to_string().contains("/nonexistent/hc"), "{err}");
}
