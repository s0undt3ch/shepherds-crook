//! What crook writes must validate against the committed JSON Schemas (Q5, task 2.6).
#![allow(clippy::expect_used)]

mod common;

use std::path::Path;

use crook::files::roster::{self, Child, Roster, State};
use serde_json::{Value, json};

use common::{assert_roster_file_valid, roster_errors, status_errors};

/// The ADR 0024 example, with `started` added.
fn example() -> Value {
    json!({
        "version": 1,
        "flock": "demo",
        "coordinator": {
            "session_id": "11111111-1111-4111-8111-111111111111",
            "folder": "/f/2026-11-27-demo",
            "state": "active",
            "started": true
        },
        "children": [{
            "name": "fix-bug",
            "role": "child",
            "model": "sonnet",
            "session_id": "22222222-2222-4222-8222-222222222222",
            "folder": "/r/.worktrees/demo-fix-bug",
            "repo": "/r",
            "branch": "crook/demo/fix-bug",
            "brief": "briefs/fix-bug.md",
            "state": "active",
            "started": true
        }]
    })
}

fn child(name: &str, repo: Option<&str>) -> Child {
    Child {
        name: name.to_string(),
        role: "child".to_string(),
        model: "sonnet".to_string(),
        session_id: "33333333-3333-4333-8333-333333333333".to_string(),
        folder: "/f".into(),
        repo: repo.map(Into::into),
        branch: repo.map(|_| format!("crook/demo/{name}")),
        brief: format!("briefs/{name}.md"),
        state: State::Active,
        started: false,
    }
}

#[test]
fn the_adr_example_with_started_validates() {
    assert_eq!(roster_errors(&example()), Vec::<String>::new());
}

#[test]
fn null_repo_and_branch_validate() {
    let mut v = example();
    v["children"][0]["repo"] = Value::Null;
    v["children"][0]["branch"] = Value::Null;
    assert_eq!(roster_errors(&v), Vec::<String>::new());
}

#[test]
fn a_roster_written_by_each_operation_validates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder: &Path = dir.path();

    roster::create(folder, &Roster::new("demo", "sid", Path::new("/f"))).expect("create");
    assert_roster_file_valid(folder);

    roster::add_child(folder, child("a", Some("/r"))).expect("add");
    roster::add_child(folder, child("b", None)).expect("add");
    assert_roster_file_valid(folder);

    roster::set_child_state(folder, "a", State::Done).expect("state");
    assert_roster_file_valid(folder);

    roster::set_child_model(folder, "a", "opus").expect("model");
    assert_roster_file_valid(folder);

    roster::set_coordinator_state(folder, State::Done).expect("coordinator state");
    assert_roster_file_valid(folder);

    roster::set_coordinator_started(folder, true).expect("started");
    assert_roster_file_valid(folder);

    roster::remove_child(folder, "b").expect("remove");
    assert_roster_file_valid(folder);
}

#[test]
fn a_null_model_fails() {
    let mut v = example();
    v["children"][0]["model"] = Value::Null;
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn an_empty_model_fails() {
    let mut v = example();
    v["children"][0]["model"] = json!("");
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn an_unknown_state_fails() {
    let mut v = example();
    v["children"][0]["state"] = json!("paused");
    assert!(!roster_errors(&v).is_empty());
    let mut v = example();
    v["coordinator"]["state"] = json!("paused");
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn an_unknown_field_fails_at_every_level() {
    let mut v = example();
    v["extra"] = json!(1);
    assert!(!roster_errors(&v).is_empty());
    let mut v = example();
    v["coordinator"]["extra"] = json!(1);
    assert!(!roster_errors(&v).is_empty());
    let mut v = example();
    v["children"][0]["extra"] = json!(1);
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn a_missing_started_fails() {
    let mut v = example();
    v["coordinator"]
        .as_object_mut()
        .expect("object")
        .remove("started");
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn every_field_is_required() {
    for key in ["version", "flock", "coordinator", "children"] {
        let mut v = example();
        v.as_object_mut().expect("object").remove(key);
        assert!(!roster_errors(&v).is_empty(), "{key}");
    }
    for key in [
        "name",
        "role",
        "model",
        "session_id",
        "folder",
        "repo",
        "branch",
        "brief",
        "state",
        "started",
    ] {
        let mut v = example();
        v["children"][0]
            .as_object_mut()
            .expect("object")
            .remove(key);
        assert!(!roster_errors(&v).is_empty(), "{key}");
    }
}

#[test]
fn a_version_other_than_one_fails() {
    let mut v = example();
    v["version"] = json!(2);
    assert!(!roster_errors(&v).is_empty());
}

#[test]
fn a_bad_name_fails() {
    for bad in ["Bad", "a--b", "-a", ""] {
        let mut v = example();
        v["children"][0]["name"] = json!(bad);
        assert!(!roster_errors(&v).is_empty(), "{bad}");
    }
}

/// Writes `value` as a roster and returns whether crook's loader accepts it.
fn loads(value: &Value) -> bool {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("roster.json"), value.to_string()).expect("write");
    roster::load(dir.path()).is_ok()
}

/// The loader and the schema must agree on `value`, and both must give `expected`.
fn assert_agree(label: &str, value: &Value, expected: bool) {
    let schema_ok = roster_errors(value).is_empty();
    let load_ok = loads(value);
    assert_eq!(
        schema_ok,
        expected,
        "schema on {label}: {:?}",
        roster_errors(value)
    );
    assert_eq!(load_ok, expected, "loader on {label}");
}

fn mutated(f: impl FnOnce(&mut Value)) -> Value {
    let mut v = example();
    f(&mut v);
    v
}

#[test]
fn the_loader_and_the_schema_agree_on_good_rosters() {
    assert_agree("example", &example(), true);
    assert_agree("version 1.0", &mutated(|v| v["version"] = json!(1.0)), true);
    assert_agree("no children", &mutated(|v| v["children"] = json!([])), true);
    assert_agree(
        "null repo and branch",
        &mutated(|v| {
            v["children"][0]["repo"] = Value::Null;
            v["children"][0]["branch"] = Value::Null;
        }),
        true,
    );
}

#[test]
fn the_loader_and_the_schema_agree_on_bad_rosters() {
    let cases: Vec<(&str, Value)> = vec![
        ("version 2", mutated(|v| v["version"] = json!(2))),
        ("version 1.5", mutated(|v| v["version"] = json!(1.5))),
        ("version string", mutated(|v| v["version"] = json!("1"))),
        ("flock upper case", mutated(|v| v["flock"] = json!("Demo"))),
        ("flock double dash", mutated(|v| v["flock"] = json!("a--b"))),
        (
            "flock too long",
            mutated(|v| v["flock"] = json!("a".repeat(17))),
        ),
        (
            "flock address-shaped",
            mutated(|v| v["flock"] = json!("t7")),
        ),
        ("flock t1x", mutated(|v| v["flock"] = json!("t1x"))),
        ("flock t1-x", mutated(|v| v["flock"] = json!("t1-x"))),
        (
            "child name too long",
            mutated(|v| v["children"][0]["name"] = json!("a".repeat(16))),
        ),
        (
            "child name slash",
            mutated(|v| v["children"][0]["name"] = json!("a/b")),
        ),
        (
            "child name empty",
            mutated(|v| v["children"][0]["name"] = json!("")),
        ),
        (
            "brief absolute",
            mutated(|v| v["children"][0]["brief"] = json!("/briefs/a.md")),
        ),
        (
            "brief outside",
            mutated(|v| v["children"][0]["brief"] = json!("a.md")),
        ),
        (
            "brief nested",
            mutated(|v| v["children"][0]["brief"] = json!("briefs/x/a.md")),
        ),
        (
            "brief dotdot",
            mutated(|v| v["children"][0]["brief"] = json!("briefs/..")),
        ),
        (
            "brief dot",
            mutated(|v| v["children"][0]["brief"] = json!("briefs/.")),
        ),
        (
            "brief empty file",
            mutated(|v| v["children"][0]["brief"] = json!("briefs/")),
        ),
        (
            "brief escape",
            mutated(|v| v["children"][0]["brief"] = json!("../briefs/a.md")),
        ),
        (
            "model null",
            mutated(|v| v["children"][0]["model"] = Value::Null),
        ),
        (
            "model empty",
            mutated(|v| v["children"][0]["model"] = json!("")),
        ),
        (
            "model blank",
            mutated(|v| v["children"][0]["model"] = json!("  \t")),
        ),
        (
            "role blank",
            mutated(|v| v["children"][0]["role"] = json!(" ")),
        ),
        (
            "child session blank",
            mutated(|v| v["children"][0]["session_id"] = json!("")),
        ),
        (
            "child folder blank",
            mutated(|v| v["children"][0]["folder"] = json!(" ")),
        ),
        (
            "coordinator session blank",
            mutated(|v| v["coordinator"]["session_id"] = json!(" ")),
        ),
        (
            "coordinator folder empty",
            mutated(|v| v["coordinator"]["folder"] = json!("")),
        ),
        (
            "child state",
            mutated(|v| v["children"][0]["state"] = json!("paused")),
        ),
        (
            "extra field",
            mutated(|v| v["children"][0]["extra"] = json!(1)),
        ),
        (
            "child started missing",
            mutated(|v| {
                v["children"][0]
                    .as_object_mut()
                    .expect("object")
                    .remove("started");
            }),
        ),
        (
            "child started not a boolean",
            mutated(|v| v["children"][0]["started"] = json!("yes")),
        ),
        (
            "started missing",
            mutated(|v| {
                v["coordinator"]
                    .as_object_mut()
                    .expect("object")
                    .remove("started");
            }),
        ),
    ];
    for (label, value) in &cases {
        assert_agree(label, value, false);
    }
}

#[test]
fn a_roster_is_refused_on_write_when_the_schema_would_refuse_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder: &Path = dir.path();
    roster::create(folder, &Roster::new("demo", "sid", Path::new("/f"))).expect("create");
    let mut c = child("a", None);
    c.model = "   ".to_string();
    assert!(roster::add_child(folder, c).is_err());
    let mut c = child("a", None);
    c.brief = "briefs/x/a.md".to_string();
    assert!(roster::add_child(folder, c).is_err());
    assert!(roster::create(dir.path(), &Roster::new("Bad", "sid", Path::new("/f"))).is_err());
    assert_roster_file_valid(folder);
}

/// The golden shape of `crook status --json` (binary plan 7.1), one child of each kind.
fn status_example() -> Value {
    let child = |name: &str, terminal: Value, status: &str| {
        json!({
            "name": name, "role": "child", "model": "sonnet", "state": "active",
            "folder": "/r/.worktrees/demo-x", "repo": "/r", "branch": "crook/demo/x",
            "brief": "/f/briefs/x.md", "terminal": terminal,
            "result": {"path": "/f/results/x.md", "status": status, "delivered": true},
        })
    };
    json!({
        "version": 1,
        "crook_version": "0.1.0",
        "flocks": [{
            "slug": "demo", "space": "crook-demo", "folder": "/f", "notes_folder": "/n", "state": "active",
            "coordinator": {
                "session_id": "11111111-1111-4111-8111-111111111111",
                "terminal": {"id": "t3", "state": "working", "app": "claude-code"},
            },
            "children": [
                child("a", json!({"id": "t4", "state": "idle", "app": null}), "working"),
                child("b", Value::Null, "done"),
            ],
        }],
    })
}

#[test]
fn the_status_example_validates_and_so_does_hc_down() {
    assert_eq!(status_errors(&status_example()), Vec::<String>::new());
    let mut down = status_example();
    down["hc_error"] = json!("hc is not installed");
    down["flocks"][0]["coordinator"]["terminal"] = Value::Null;
    assert_eq!(status_errors(&down), Vec::<String>::new());
    let empty = json!({"version": 1, "crook_version": "0.1.0", "flocks": []});
    assert_eq!(status_errors(&empty), Vec::<String>::new());
}

#[test]
fn status_documents_that_break_the_schema_fail() {
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutation)> = vec![
        ("version 2", Box::new(|v| v["version"] = json!(2))),
        (
            "no crook_version",
            Box::new(|v| drop(v.as_object_mut().expect("o").remove("crook_version"))),
        ),
        (
            "no flocks",
            Box::new(|v| drop(v.as_object_mut().expect("o").remove("flocks"))),
        ),
        (
            "hc_error not a string",
            Box::new(|v| v["hc_error"] = json!(1)),
        ),
        (
            "unknown result status",
            Box::new(|v| v["flocks"][0]["children"][0]["result"]["status"] = json!("finished")),
        ),
        (
            "null model",
            Box::new(|v| v["flocks"][0]["children"][0]["model"] = Value::Null),
        ),
        (
            "empty model",
            Box::new(|v| v["flocks"][0]["children"][0]["model"] = json!("")),
        ),
        (
            "terminal a string",
            Box::new(|v| v["flocks"][0]["children"][0]["terminal"] = json!("t4")),
        ),
        (
            "terminal without id",
            Box::new(|v| {
                v["flocks"][0]["coordinator"]["terminal"] = json!({"state": "idle", "app": null})
            }),
        ),
        (
            "unknown state",
            Box::new(|v| v["flocks"][0]["state"] = json!("paused")),
        ),
        (
            "delivered not a boolean",
            Box::new(|v| v["flocks"][0]["children"][1]["result"]["delivered"] = json!("yes")),
        ),
        (
            "no coordinator",
            Box::new(|v| {
                drop(
                    v["flocks"][0]
                        .as_object_mut()
                        .expect("o")
                        .remove("coordinator"),
                )
            }),
        ),
        (
            "child without brief",
            Box::new(|v| {
                drop(
                    v["flocks"][0]["children"][0]
                        .as_object_mut()
                        .expect("o")
                        .remove("brief"),
                )
            }),
        ),
    ];
    for (label, mutate) in &cases {
        let mut v = status_example();
        mutate(&mut v);
        assert!(!status_errors(&v).is_empty(), "{label} should fail");
    }
}

#[test]
fn status_allows_fields_a_newer_crook_adds() {
    let mut v = status_example();
    v["future"] = json!(true);
    v["flocks"][0]["children"][0]["future"] = json!(1);
    assert_eq!(status_errors(&v), Vec::<String>::new());
}
