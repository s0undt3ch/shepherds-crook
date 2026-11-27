use super::*;

fn kv(k: &str, v: &str) -> (String, String) {
    (k.to_string(), v.to_string())
}

fn spec(name: &str) -> RunSpec {
    RunSpec {
        name: name.to_string(),
        cwd: "/work".into(),
        labels: vec![kv("crook.flock", "demo")],
        parent: Parent::None,
        argv: vec!["sleep".to_string(), "30".to_string()],
    }
}

fn err_kind(e: &Error) -> Option<&str> {
    match e {
        Error::Hc { kind, .. } => kind.as_deref(),
        _ => None,
    }
}

#[test]
fn run_adds_a_live_terminal_with_the_next_address() {
    let hc = FakeHc::new();
    let a = hc.run(&spec("one")).expect("run");
    let b = hc.run(&spec("two")).expect("run");
    assert_eq!((a.address.as_str(), b.address.as_str()), ("t1", "t2"));
    let got = hc.get("one").expect("get").expect("found");
    assert_eq!(got.state, TermState::Live);
    assert_eq!(
        got.labels.get("crook.flock").map(String::as_str),
        Some("demo")
    );
    assert_eq!(
        hc.get("t2").expect("get").expect("found").name.as_deref(),
        Some("two")
    );
}

#[test]
fn a_live_name_cannot_be_used_twice() {
    let hc = FakeHc::new();
    hc.run(&spec("one")).expect("run");
    let err = hc.run(&spec("one")).expect_err("taken");
    assert_eq!(err_kind(&err), Some("name_taken"));
}

#[test]
fn names_follow_hcs_own_rule() {
    let hc = FakeHc::new();
    for ok in [
        "a",
        "demo.child",
        "a-b_c.d",
        "t0",
        "t01",
        "tt7",
        &"a".repeat(32),
    ] {
        assert!(hc.run(&spec(ok)).is_ok(), "{ok}");
    }
    for bad in [
        "",
        "-a",
        ".a",
        "A",
        "t7",
        "t10",
        "t1x",
        "a b",
        "a/b",
        &"a".repeat(33),
    ] {
        let err = hc.run(&spec(bad)).expect_err(bad);
        assert_eq!(err_kind(&err), Some("invalid_params"), "{bad}");
    }
}

#[test]
fn a_finished_parent_is_accepted_like_real_hc() {
    let hc = FakeHc::new();
    let parent = hc.run(&spec("par")).expect("run");
    hc.kill("par").expect("kill");
    let mut child = spec("kid");
    child.parent = Parent::Terminal(parent.address.clone());
    let kid = hc.run(&child).expect("hc accepts a finished parent");
    assert_eq!(kid.state, TermState::Live);
    assert_eq!(kid.parent.as_deref(), Some(parent.address.as_str()));
}

#[test]
fn a_parent_must_exist_and_a_dying_parent_takes_its_children() {
    let hc = FakeHc::new();
    let parent = hc.run(&spec("boss")).expect("run");
    let mut child = spec("kid");
    child.parent = Parent::Terminal(parent.address.clone());
    hc.run(&child).expect("run");
    let mut orphan = spec("orphan");
    orphan.parent = Parent::Terminal("t99".to_string());
    assert!(hc.run(&orphan).is_err());

    assert_eq!(hc.kill("boss").expect("kill"), KillOutcome::Killed);
    assert_eq!(
        hc.get("t2").expect("get").expect("found").state,
        TermState::Finished
    );
}

#[test]
fn kill_finishes_a_terminal_and_a_finished_name_can_be_reused() {
    let hc = FakeHc::new();
    hc.run(&spec("one")).expect("run");
    assert_eq!(hc.kill("one").expect("kill"), KillOutcome::Killed);
    let dead = hc.get("t1").expect("get").expect("found");
    assert_eq!(dead.state, TermState::Finished);
    assert_eq!(dead.reason.as_deref(), Some("killed"));

    let again = hc.run(&spec("one")).expect("a finished name is free");
    assert_eq!(again.address, "t2");
    assert_eq!(
        hc.get("one").expect("get").expect("found").address,
        "t2",
        "the live one wins the name"
    );
    assert_eq!(
        hc.kill("t1").expect("kill"),
        KillOutcome::Killed,
        "killing a finished one is fine"
    );
    assert_eq!(hc.kill("nosuch").expect("kill"), KillOutcome::NotFound);
}

#[test]
fn ls_hides_the_finished_unless_asked_and_carries_no_labels() {
    let hc = FakeHc::new();
    hc.run(&spec("one")).expect("run");
    hc.run(&spec("two")).expect("run");
    hc.kill("one").expect("kill");
    let live = hc.ls(false).expect("ls");
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].name.as_deref(), Some("two"));
    assert!(live[0].labels.is_empty());
    assert_eq!(hc.ls(true).expect("ls").len(), 2);
}

#[test]
fn ls_by_label_needs_every_label() {
    let hc = FakeHc::new();
    hc.run(&spec("one")).expect("run");
    let mut other = spec("two");
    other.labels = vec![kv("crook.flock", "demo"), kv("crook.child", "two")];
    hc.run(&other).expect("run");
    let both = hc
        .ls_by_label(false, &[kv("crook.flock", "demo")])
        .expect("ls");
    assert_eq!(both.len(), 2);
    let one = hc
        .ls_by_label(
            false,
            &[kv("crook.flock", "demo"), kv("crook.child", "two")],
        )
        .expect("ls");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].name.as_deref(), Some("two"));
}

#[test]
fn send_outcomes_follow_the_terminal() {
    let hc = FakeHc::new();
    hc.run(&spec("live")).expect("run");
    hc.run(&spec("asking")).expect("run");
    hc.set_status("asking", "blocked", Some("question"));
    hc.run(&spec("dead")).expect("run");
    hc.kill("dead").expect("kill");
    assert_eq!(hc.send("live", "x").expect("send"), SendOutcome::Sent);
    assert_eq!(hc.send("asking", "x").expect("send"), SendOutcome::Blocked);
    assert_eq!(hc.send("dead", "x").expect("send"), SendOutcome::Finished);
    assert_eq!(hc.send("nosuch", "x").expect("send"), SendOutcome::NotFound);
}

#[test]
fn a_forced_send_outcome_wins() {
    let hc = FakeHc::new();
    hc.run(&spec("live")).expect("run");
    hc.force_send(SendOutcome::Failed("nope".to_string()));
    assert_eq!(
        hc.send("live", "x").expect("send"),
        SendOutcome::Failed("nope".to_string())
    );
}

#[test]
fn every_call_is_recorded_in_order() {
    let hc = FakeHc::new();
    hc.version().expect("version");
    hc.server_start(&[("A".into(), "1".into())]).expect("start");
    hc.run(&spec("one")).expect("run");
    hc.send("one", "hi").expect("send");
    hc.get("one").expect("get");
    hc.kill("one").expect("kill");
    hc.ls(true).expect("ls");
    hc.schema().expect("schema");
    assert_eq!(
        hc.calls(),
        vec![
            Call::Version,
            Call::ServerStart(vec![("A".into(), "1".into())]),
            Call::Run(spec("one")),
            Call::Send {
                to: "one".to_string(),
                text: "hi".to_string()
            },
            Call::Get("one".to_string()),
            Call::Kill("one".to_string()),
            Call::Ls {
                all: true,
                labels: vec![]
            },
            Call::Schema,
        ]
    );
}

#[test]
fn a_call_can_be_told_to_fail_a_number_of_times() {
    let hc = FakeHc::new();
    hc.fail("schema", 2);
    assert!(hc.schema().is_err());
    assert!(hc.schema().is_err());
    assert!(hc.schema().is_ok());
    assert_eq!(hc.calls().len(), 3, "failed calls are still recorded");
}

#[test]
fn the_version_and_the_schema_can_be_set() {
    let hc = FakeHc::new().with_version("0.1.21").without_run_labels();
    assert_eq!(hc.version().expect("version"), "0.1.21");
    assert!(
        !hc.schema()
            .expect("schema")
            .has_param("terminal.run", "labels")
    );
    assert!(
        FakeHc::new()
            .schema()
            .expect("schema")
            .has_param("terminal.run", "labels")
    );
}

mod shape {
    //! The fake's output has the keys the captured real output has (`tests/fixtures/hc/0.1.22`).
    use std::collections::BTreeSet;

    use super::*;
    use crate::hc::fixtures::fixture;

    /// The JSON keys a terminal would print for the fields it fills in, without `turn`, which
    /// crook does not model.
    fn keys(t: &Terminal) -> BTreeSet<String> {
        let mut k: BTreeSet<&str> = ["address", "state"].into();
        let fields = [
            ("agent", t.agent.is_some()),
            ("cwd", t.cwd.is_some()),
            ("name", t.name.is_some()),
            ("parent", t.parent.is_some()),
            ("status", t.status.is_some()),
            ("reason", t.reason.is_some()),
            ("exit_code", t.exit_code.is_some()),
            ("signal", t.signal.is_some()),
            ("labels", !t.labels.is_empty()),
        ];
        k.extend(fields.iter().filter(|(_, on)| *on).map(|(n, _)| *n));
        k.into_iter().map(str::to_string).collect()
    }

    fn fixture_keys(value: &serde_json::Value) -> BTreeSet<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .filter(|k| *k != "turn")
            .cloned()
            .collect()
    }

    fn entries(name: &str) -> Vec<serde_json::Value> {
        let v: serde_json::Value = serde_json::from_str(&fixture(name).stdout).expect("json");
        v["terminals"].as_array().expect("terminals").clone()
    }

    fn named(list: &[serde_json::Value], name: &str) -> BTreeSet<String> {
        let found = list
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("{name} in the fixture"));
        fixture_keys(found)
    }

    fn fake_keys(list: &[Terminal], name: &str) -> BTreeSet<String> {
        keys(
            list.iter()
                .find(|t| t.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("{name} in the fake")),
        )
    }

    fn scenario() -> FakeHc {
        let hc = FakeHc::new();
        let mut a = spec("worker-a");
        a.labels = vec![kv("crook.flock", "demo")];
        hc.run(&a).expect("run");
        let mut b = spec("worker-b");
        b.parent = Parent::Terminal("worker-a".to_string());
        hc.run(&b).expect("run");
        hc.set_status("worker-b", "blocked", Some("question"));
        hc.run(&spec("worker-c")).expect("run");
        hc.exit("worker-c", 3);
        hc
    }

    #[test]
    fn run_prints_what_real_run_prints() {
        let hc = FakeHc::new();
        let no_parent = hc.run(&spec("worker-a")).expect("run");
        let mut b = spec("worker-b");
        b.parent = Parent::Terminal("worker-a".to_string());
        let with_parent = hc.run(&b).expect("run");
        let real: serde_json::Value =
            serde_json::from_str(&fixture("run_labels").stdout).expect("json");
        assert_eq!(keys(&no_parent), fixture_keys(&real));
        let real: serde_json::Value =
            serde_json::from_str(&fixture("run_blocked").stdout).expect("json");
        assert_eq!(keys(&with_parent), fixture_keys(&real));
        assert!(no_parent.labels.is_empty(), "run prints no labels");
    }

    #[test]
    fn ls_prints_what_real_ls_prints() {
        let hc = scenario();
        let live = hc.ls(false).expect("ls");
        let real = entries("ls");
        for name in ["worker-a", "worker-b"] {
            assert_eq!(fake_keys(&live, name), named(&real, name), "{name}");
        }
        let blocked = live
            .iter()
            .find(|t| t.name.as_deref() == Some("worker-b"))
            .expect("b");
        assert_eq!(
            blocked.status,
            Some(Status {
                state: "blocked".to_string(),
                kind: None,
                app: None
            }),
            "ls gives the status word only"
        );

        let all = hc.ls(true).expect("ls");
        let real = entries("ls_all");
        assert_eq!(fake_keys(&all, "worker-c"), named(&real, "worker-c"));
    }

    #[test]
    fn get_prints_what_real_get_prints() {
        let hc = scenario();
        hc.kill("worker-a").expect("kill");
        let killed: serde_json::Value =
            serde_json::from_str(&fixture("get_killed").stdout).expect("json");
        let ended = hc.get("worker-a").expect("get").expect("found");
        for key in ["reason", "signal", "labels", "name", "address", "state"] {
            assert!(killed.get(key).is_some(), "fixture has {key}");
        }
        assert_eq!(ended.reason.as_deref(), Some("killed"));
        assert_eq!(ended.signal, Some(1));
        assert!(!ended.labels.is_empty());
    }
}
