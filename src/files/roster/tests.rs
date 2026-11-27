use std::path::Path;

use serde_json::json;

use super::*;

const EXAMPLE: &str = r#"{
  "version": 1,
  "flock": "demo",
  "coordinator": { "session_id": "11111111-1111-4111-8111-111111111111", "folder": "/f/2026-11-27-demo", "state": "active", "started": true },
  "children": [
    {
      "name": "fix-bug",
      "role": "child",
      "model": "sonnet",
      "session_id": "22222222-2222-4222-8222-222222222222",
      "folder": "/r/.worktrees/demo-fix-bug",
      "repo": "/r",
      "branch": "crook/demo/fix-bug",
      "brief": "briefs/fix-bug.md",
      "state": "active"
    }
  ]
}"#;

fn flock_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write_raw(dir: &Path, text: &str) {
    std::fs::write(dir.join("roster.json"), text).expect("write");
}

fn child(name: &str) -> Child {
    Child {
        name: name.to_string(),
        role: "child".to_string(),
        model: "sonnet".to_string(),
        session_id: "33333333-3333-4333-8333-333333333333".to_string(),
        folder: "/f".into(),
        repo: None,
        branch: None,
        brief: format!("briefs/{name}.md"),
        state: State::Active,
    }
}

fn seeded(dir: &Path) {
    create(dir, &Roster::new("demo", "sid", Path::new("/f"))).expect("create");
}

#[test]
fn the_example_round_trips() {
    let dir = flock_dir();
    write_raw(dir.path(), EXAMPLE);
    let roster = load(dir.path()).expect("loads");
    assert_eq!(roster.version, 1);
    assert!(roster.coordinator.started);
    assert_eq!(roster.children[0].repo.as_deref(), Some(Path::new("/r")));
    let again = serde_json::to_value(&roster).expect("json");
    assert_eq!(
        again,
        serde_json::from_str::<serde_json::Value>(EXAMPLE).expect("json")
    );
}

#[test]
fn null_repo_and_branch_are_allowed() {
    let dir = flock_dir();
    write_raw(
        dir.path(),
        &EXAMPLE
            .replace("\"/r\"", "null")
            .replace("\"crook/demo/fix-bug\"", "null"),
    );
    let roster = load(dir.path()).expect("loads");
    assert_eq!(roster.children[0].repo, None);
    assert_eq!(roster.children[0].branch, None);
}

#[test]
fn an_unknown_version_is_an_error_naming_the_file() {
    let dir = flock_dir();
    write_raw(
        dir.path(),
        &EXAMPLE.replace("\"version\": 1", "\"version\": 2"),
    );
    let msg = load(dir.path()).expect_err("fails").to_string();
    assert!(
        msg.contains("roster.json") && msg.contains("version 2"),
        "{msg}"
    );
}

#[test]
fn a_missing_version_is_an_error() {
    let dir = flock_dir();
    write_raw(dir.path(), r#"{"flock": "x"}"#);
    assert!(load(dir.path()).is_err());
}

#[test]
fn a_null_or_empty_model_is_an_error() {
    let dir = flock_dir();
    write_raw(dir.path(), &EXAMPLE.replace("\"sonnet\"", "null"));
    assert!(load(dir.path()).is_err());
    write_raw(dir.path(), &EXAMPLE.replace("\"sonnet\"", "\"\""));
    assert!(load(dir.path()).is_err());
}

#[test]
fn an_unknown_field_or_state_is_an_error() {
    let dir = flock_dir();
    write_raw(
        dir.path(),
        &EXAMPLE.replace("\"role\"", "\"extra\": 1, \"role\""),
    );
    assert!(load(dir.path()).is_err());
    write_raw(dir.path(), &EXAMPLE.replace("\"active\"", "\"paused\""));
    assert!(load(dir.path()).is_err());
}

#[test]
fn a_missing_started_is_an_error() {
    let dir = flock_dir();
    write_raw(dir.path(), &EXAMPLE.replace(", \"started\": true", ""));
    assert!(load(dir.path()).is_err());
}

#[test]
fn a_missing_file_is_an_error_naming_it() {
    let dir = flock_dir();
    let msg = load(dir.path()).expect_err("fails").to_string();
    assert!(msg.contains("roster.json"), "{msg}");
}

#[test]
fn a_new_roster_is_active_and_not_started() {
    let dir = flock_dir();
    seeded(dir.path());
    let roster = load(dir.path()).expect("loads");
    assert_eq!(roster.flock, "demo");
    assert_eq!(roster.coordinator.state, State::Active);
    assert!(!roster.coordinator.started);
    assert!(roster.children.is_empty());
}

#[test]
fn create_refuses_to_replace_a_roster() {
    let dir = flock_dir();
    seeded(dir.path());
    assert!(create(dir.path(), &Roster::new("other", "s", Path::new("/f"))).is_err());
    assert_eq!(load(dir.path()).expect("loads").flock, "demo");
}

#[test]
fn adding_a_child_whose_name_exists_is_an_error() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    let msg = add_child(dir.path(), child("a"))
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("already exists"), "{msg}");
    assert_eq!(load(dir.path()).expect("loads").children.len(), 1);
}

#[test]
fn removing_a_child_frees_its_name() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    remove_child(dir.path(), "a").expect("removes");
    assert!(load(dir.path()).expect("loads").children.is_empty());
    add_child(dir.path(), child("a")).expect("adds again");
}

#[test]
fn an_operation_on_an_unknown_child_is_an_error() {
    let dir = flock_dir();
    seeded(dir.path());
    assert!(remove_child(dir.path(), "x").is_err());
    assert!(set_child_state(dir.path(), "x", State::Done).is_err());
    assert!(set_child_model(dir.path(), "x", "opus").is_err());
}

#[test]
fn state_model_and_coordinator_operations_change_only_their_field() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    set_child_state(dir.path(), "a", State::Done).expect("state");
    set_child_model(dir.path(), "a", "opus").expect("model");
    set_coordinator_state(dir.path(), State::Done).expect("coordinator state");
    set_coordinator_started(dir.path(), true).expect("started");
    let roster = load(dir.path()).expect("loads");
    assert_eq!(roster.children[0].state, State::Done);
    assert_eq!(roster.children[0].model, "opus");
    assert_eq!(roster.children[0].role, "child");
    assert_eq!(roster.coordinator.state, State::Done);
    assert!(roster.coordinator.started);
}

#[test]
fn an_empty_model_is_refused_on_write_and_leaves_the_file_alone() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    assert!(set_child_model(dir.path(), "a", "").is_err());
    let mut bad = child("b");
    bad.model = String::new();
    assert!(add_child(dir.path(), bad).is_err());
    let roster = load(dir.path()).expect("loads");
    assert_eq!(roster.children.len(), 1);
    assert_eq!(roster.children[0].model, "sonnet");
}

#[test]
fn a_child_name_must_be_valid() {
    let dir = flock_dir();
    seeded(dir.path());
    assert!(add_child(dir.path(), child("Bad Name")).is_err());
}

#[test]
fn the_brief_stays_relative_under_briefs() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    assert_eq!(
        load(dir.path()).expect("loads").children[0].brief,
        "briefs/a.md"
    );
    for bad in [
        "/abs/a.md",
        "a.md",
        "../briefs/a.md",
        "briefs/../a.md",
        "briefs",
        "other/a.md",
    ] {
        let mut c = child("b");
        c.brief = bad.to_string();
        assert!(add_child(dir.path(), c).is_err(), "{bad}");
    }
}

#[test]
fn the_written_file_has_exactly_the_documented_keys() {
    let dir = flock_dir();
    seeded(dir.path());
    add_child(dir.path(), child("a")).expect("adds");
    let text = std::fs::read_to_string(dir.path().join("roster.json")).expect("reads");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        value["coordinator"],
        json!({"session_id": "sid", "folder": "/f", "state": "active", "started": false})
    );
    assert_eq!(value["children"][0]["repo"], json!(null));
}

#[test]
fn concurrent_adds_all_survive() {
    let dir = flock_dir();
    seeded(dir.path());
    std::thread::scope(|scope| {
        for t in 0..8 {
            let folder = dir.path();
            scope.spawn(move || add_child(folder, child(&format!("c{t}"))).expect("adds"));
        }
    });
    assert_eq!(load(dir.path()).expect("loads").children.len(), 8);
}
