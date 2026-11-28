use std::time::Duration;

use super::*;
use crate::hc::fake::FakeHc;
use crate::hc::{KillOutcome, Schema, SendOutcome, Terminal};
use crate::testutil::{Fixture, SESSION, hook_input, make_flock, run_hook, start_coordinator};

#[test]
fn the_budgets_sit_below_the_hook_timeouts() {
    assert_eq!(Event::SessionStart.budget(), Duration::from_secs(100));
    assert_eq!(Event::Stop.budget(), Duration::from_secs(20));
    assert_eq!(Event::UserPromptSubmit.budget(), Duration::from_secs(20));
    // hooks.json names the same events and timeouts.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin/hooks/hooks.json");
    let json: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("hooks.json")).expect("json");
    for (key, event) in [
        ("SessionStart", "session-start"),
        ("Stop", "stop"),
        ("UserPromptSubmit", "user-prompt-submit"),
    ] {
        let hook = &json["hooks"][key][0]["hooks"][0];
        let command = hook["command"].as_str().expect("command");
        assert!(command.ends_with(&format!("hook {event}")), "{command}");
        let timeout = hook["timeout"].as_u64().expect("timeout");
        assert!(Event::parse(event).expect("event").budget() < Duration::from_secs(timeout));
    }
}

#[test]
fn input_reads_what_it_needs_and_ignores_the_rest() {
    let input = parse_input(r#"{"session_id":"s","cwd":"/c","source":"resume","extra":{"a":1},"hook_event_name":"Stop"}"#)
        .expect("parses");
    assert_eq!(
        input,
        Input {
            session_id: Some("s".into()),
            source: Some("resume".into()),
            cwd: Some("/c".into())
        }
    );
    assert_eq!(parse_input("{}").expect("empty object"), Input::default());
    assert!(parse_input("not json").is_err());
    assert!(parse_input("[1]").is_err());
}

#[test]
fn bad_input_and_unknown_events_do_nothing_and_say_so() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    for (event, input) in [
        ("stop", "not json"),
        ("stop", ""),
        ("bogus", "{}"),
        ("", "{}"),
    ] {
        let ran = run_hook(&fx, &hc, event, Some("t1"), input);
        assert_eq!(ran.stdout, "", "{event} {input}");
        assert!(
            ran.stderr.starts_with("crook hook: "),
            "{event} {input}: {}",
            ran.stderr
        );
    }
    assert!(hc.calls().is_empty(), "nothing asked hc");
}

#[test]
fn no_terminal_id_means_no_hc_call() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    let ran = run_hook(&fx, &hc, "session-start", None, &hook_input(SESSION));
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    fx.set_env("HC_TERMINAL_ID", "");
    let ran = run_hook(&fx, &hc, "stop", None, &hook_input(SESSION));
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
    assert!(hc.calls().is_empty());
}

#[test]
fn no_core_means_no_hc_call() {
    let fx = Fixture::new();
    fx.set_env("HC_TERMINAL_ID", "t1");
    let hc = FakeHc::new();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    run(
        &fx.ctx(),
        &hc,
        &|| false,
        "stop",
        &mut hook_input(SESSION).as_bytes(),
        &mut out,
        &mut err,
    );
    assert!(out.is_empty());
    assert!(String::from_utf8_lossy(&err).contains("no hc core"));
    assert!(hc.calls().is_empty(), "a hook must never start a core");
}

#[test]
fn a_terminal_that_is_not_a_crook_one_is_left_alone() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    use crate::hc::{Hc, Parent, RunSpec};
    let plain = hc
        .run(&RunSpec {
            name: "plain".into(),
            cwd: "/".into(),
            labels: vec![],
            parent: Parent::None,
            argv: vec!["claude".into()],
        })
        .expect("run")
        .address;
    for event in ["session-start", "stop", "user-prompt-submit"] {
        let ran = run_hook(&fx, &hc, event, Some(&plain), &hook_input(SESSION));
        assert_eq!(
            (ran.stdout.as_str(), ran.stderr.as_str()),
            ("", ""),
            "{event}"
        );
    }
    let ran = run_hook(&fx, &hc, "stop", Some("t99"), &hook_input(SESSION));
    assert_eq!((ran.stdout.as_str(), ran.stderr.as_str()), ("", ""));
}

#[test]
fn a_failing_hc_is_a_stderr_line() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    hc.fail("get", 1);
    let ran = run_hook(&fx, &hc, "stop", Some("t1"), &hook_input(SESSION));
    assert_eq!(ran.stdout, "");
    assert!(
        ran.stderr.contains("could not look up terminal"),
        "{}",
        ran.stderr
    );
}

/// An hc that panics, as a bug in crook would.
struct PanickingHc;

impl Hc for PanickingHc {
    fn version(&self) -> Result<String, crate::Error> {
        panic!("boom")
    }
    fn schema(&self) -> Result<Schema, crate::Error> {
        panic!("boom")
    }
    fn ls(&self, _: bool) -> Result<Vec<Terminal>, crate::Error> {
        panic!("boom")
    }
    fn ls_by_label(&self, _: bool, _: &[(String, String)]) -> Result<Vec<Terminal>, crate::Error> {
        panic!("boom")
    }
    fn get(&self, _: &str) -> Result<Option<Terminal>, crate::Error> {
        panic!("boom")
    }
    fn run(&self, _: &crate::hc::RunSpec) -> Result<Terminal, crate::Error> {
        panic!("boom")
    }
    fn send(&self, _: &str, _: &str) -> Result<SendOutcome, crate::Error> {
        panic!("boom")
    }
    fn read(&self, _: &str, _: u32) -> Result<String, crate::Error> {
        panic!("boom")
    }
    fn kill(&self, _: &str) -> Result<KillOutcome, crate::Error> {
        panic!("boom")
    }
    fn server_start(
        &self,
        _: &[(std::ffi::OsString, std::ffi::OsString)],
    ) -> Result<(), crate::Error> {
        panic!("boom")
    }
}

#[test]
fn a_panic_is_caught_and_nothing_reaches_stdout() {
    let fx = Fixture::new();
    for event in ["session-start", "stop", "user-prompt-submit"] {
        let ran = run_hook(&fx, &PanickingHc, event, Some("t1"), &hook_input(SESSION));
        assert_eq!(ran.stdout, "", "{event}");
        assert!(
            ran.stderr.contains("a panic ended the hook early"),
            "{}",
            ran.stderr
        );
    }
}

fn lines(text: &[&str]) -> Reply {
    Reply {
        lines: text.iter().map(|t| (*t).to_string()).collect(),
        undelivered: Vec::new(),
    }
}

fn pending(dir: &std::path::Path, child: &str, size: usize) -> prompt::Undelivered {
    let path = dir.join(format!("{child}.md"));
    prompt::Undelivered {
        folder: dir.to_path_buf(),
        child: child.to_string(),
        hash: "h".repeat(size.max(1)),
        path,
    }
}

/// A writer that refuses everything, as a closed stdout.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("closed"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn results_are_marked_only_after_the_line_is_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("results")).expect("mkdir");
    let reply = || Reply {
        lines: Vec::new(),
        undelivered: vec![pending(dir.path(), "a", 1)],
    };
    let mut err = Vec::new();
    emit(&mut Broken, &mut err, Event::UserPromptSubmit, reply());
    assert!(String::from_utf8_lossy(&err).contains("could not write the answer"));
    assert!(
        crate::files::marks::load(dir.path())
            .expect("marks")
            .is_empty(),
        "not marked"
    );
    let mut out = Vec::new();
    emit(&mut out, &mut err, Event::UserPromptSubmit, reply());
    assert!(
        crate::files::marks::load(dir.path())
            .expect("marks")
            .contains_key("a")
    );
}

#[test]
fn a_result_line_that_does_not_fit_under_the_cap_is_left_unmarked() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("results")).expect("mkdir");
    // Each line is about 60 characters of prefix plus the folder; 400 of them cannot all fit.
    let many: Vec<_> = (0..400)
        .map(|i| pending(dir.path(), &format!("c{i:03}"), 1))
        .collect();
    let (text, fitted) = fit(&[], &many);
    assert!(fitted > 0 && fitted < many.len(), "{fitted}");
    assert!(text.chars().count() <= MAX_CONTEXT_CHARS);
    assert_eq!(text.lines().count(), fitted);
    let mut out = Vec::new();
    let mut err = Vec::new();
    emit(
        &mut out,
        &mut err,
        Event::UserPromptSubmit,
        Reply {
            lines: Vec::new(),
            undelivered: many,
        },
    );
    let marks = crate::files::marks::load(dir.path()).expect("marks");
    assert_eq!(marks.len(), fitted);
    assert!(marks.contains_key("c000") && !marks.contains_key("c399"));
}

#[test]
fn the_budget_ends_a_hung_hc_and_nothing_of_it_is_left() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fixture::new();
    let dir = fx.root();
    let pidfile = dir.join("hc.pid");
    let hc = dir.join("hc");
    std::fs::write(
        &hc,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec /bin/sleep 60\n",
            pidfile.display()
        ),
    )
    .expect("script");
    std::fs::set_permissions(&hc, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let socket = dir.join("c.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
    fx.set_env("HC_TERMINAL_SOCKET", &socket.display().to_string());
    fx.set_env("HC_TERMINAL_ID", "t1");
    let started = std::time::Instant::now();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    // A script written a moment ago can still be "text file busy" while another test thread forks;
    // the run is repeated until the script has started.
    for _ in 0..5 {
        run_budgeted(
            &fx.ctx(),
            &hc.display().to_string(),
            "stop",
            Duration::from_millis(400),
            &mut hook_input(SESSION).as_bytes(),
            &mut out,
            &mut err,
            || {},
        );
        if pidfile.exists() {
            break;
        }
    }
    // Far below the 10 s a single hc call may take, so the budget, not the call limit, ended it.
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "{:?}",
        started.elapsed()
    );
    assert!(out.is_empty());
    assert!(
        String::from_utf8_lossy(&err).contains("crook hook:"),
        "{}",
        String::from_utf8_lossy(&err)
    );
    let pid: i32 = std::fs::read_to_string(&pidfile)
        .expect("pid")
        .trim()
        .parse()
        .expect("number");
    // The process group is gone: no signal can be sent to the process.
    let gone = (0..50).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_err()
    });
    assert!(gone, "the hung hc ({pid}) is still running");
}

#[test]
fn the_context_goes_out_as_hook_specific_output_and_stop_prints_nothing() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    emit(
        &mut out,
        &mut err,
        Event::UserPromptSubmit,
        lines(&["a", "b"]),
    );
    let doc: Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(
        doc,
        json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": "a\nb"}})
    );
    out.clear();
    emit(&mut out, &mut err, Event::SessionStart, lines(&["x"]));
    assert!(String::from_utf8_lossy(&out).contains("\"hookEventName\":\"SessionStart\""));
    out.clear();
    emit(&mut out, &mut err, Event::Stop, lines(&["x"]));
    emit(&mut out, &mut err, Event::SessionStart, lines(&[]));
    assert!(out.is_empty());
    emit(
        &mut out,
        &mut err,
        Event::SessionStart,
        lines(&[&"é".repeat(MAX_CONTEXT_CHARS + 5)]),
    );
    let doc: Value = serde_json::from_slice(&out).expect("json");
    assert_eq!(
        doc["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("text")
            .chars()
            .count(),
        MAX_CONTEXT_CHARS
    );
    assert_eq!(err, b"");
}

#[test]
fn the_watchdog_fires_after_its_limit_and_not_when_dropped_in_time() {
    let (tx, rx) = std::sync::mpsc::channel();
    let tx2 = tx.clone();
    let _guard = watchdog(Duration::from_millis(30), move || {
        tx.send(()).expect("send")
    });
    assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "fired");

    let fired = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&fired);
    drop(watchdog(Duration::from_millis(100), move || {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !fired.load(std::sync::atomic::Ordering::SeqCst),
        "dropped guard"
    );
    drop(tx2);
}

#[test]
fn a_core_is_reachable_only_when_its_socket_accepts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let socket = dir.path().join("c.sock");
    let text = socket.display().to_string();
    let env = |k: &str| (k == "HC_TERMINAL_SOCKET").then(|| text.clone());
    assert!(!core_reachable(&env), "no socket yet");
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("bind");
    assert!(core_reachable(&env));
    drop(listener);
    // The socket file is still there, as after a core that died: refused.
    assert!(socket.exists());
    assert!(!core_reachable(&env));
    assert!(!core_reachable(&|_: &str| None));
    assert!(!core_reachable(&|_: &str| Some(String::new())));
}

#[test]
fn a_coordinator_session_runs_end_to_end() {
    let fx = Fixture::new();
    fx.set_plugin_version("1.0.0");
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    std::fs::write(flock.result_path("a"), "---\nstatus: done\n---\n").expect("result");
    let ran = run_hook(
        &fx,
        &hc,
        "user-prompt-submit",
        Some(&coordinator),
        &hook_input(SESSION),
    );
    assert_eq!(
        ran.context().expect("context"),
        format!(
            "Undelivered child result: {}",
            flock.result_path("a").display()
        )
    );
    let again = run_hook(
        &fx,
        &hc,
        "user-prompt-submit",
        Some(&coordinator),
        &hook_input(SESSION),
    );
    assert_eq!(again.stdout, "");
}
