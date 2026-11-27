use serde_json::{Value, json};

use super::*;
use crate::files::result::sha256_hex;
use crate::hc::fake::FakeHc;
use crate::hc::{Hc, Parent, RunSpec};
use crate::testutil::{Buffers, Fixture, load_roster, make_flock, start_coordinator, write_brief};

fn child(name: &str, state: State) -> Child {
    Child {
        name: name.to_string(),
        role: "child".to_string(),
        model: "sonnet".to_string(),
        session_id: format!("00000000-0000-4000-8000-00000000000{}", name.len()),
        folder: format!("/work/{name}").into(),
        repo: None,
        branch: None,
        brief: format!("briefs/{name}.md"),
        state,
        started: true,
    }
}

fn run_child(hc: &FakeHc, flock: &Flock, name: &str, parent: &str) -> String {
    hc.run(&RunSpec {
        name: names::hc_child(&flock.slug, name),
        cwd: flock.folder.clone(),
        labels: vec![
            (names::LABEL_FLOCK.to_string(), flock.slug.clone()),
            (names::LABEL_CHILD.to_string(), name.to_string()),
        ],
        parent: Parent::Terminal(parent.to_string()),
        argv: vec!["claude".to_string()],
    })
    .expect("run")
    .address
}

fn write_result(flock: &Flock, name: &str, text: &str) {
    std::fs::write(flock.result_path(name), text).expect("result");
}

fn status_of(
    fx: &Fixture,
    hc: &FakeHc,
    slug: Option<&str>,
    json: bool,
) -> (Result<(), Error>, Buffers) {
    let mut bufs = Buffers::default();
    let out = run(&fx.ctx(), hc, &mut bufs.io(), slug, json);
    (out, bufs)
}

/// A live coordinator, a live working child with a delivered result, a paused child with a result
/// not yet delivered, a done child and a child whose result is unreadable.
fn scenario(fx: &Fixture) -> (Flock, FakeHc) {
    let flock = make_flock(fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    hc.set_status(&coordinator, "idle", None);
    hc.set_app(&coordinator, "claude-code");
    for (name, state) in [
        ("live", State::Active),
        ("paused", State::Active),
        ("finished", State::Done),
        ("broken", State::Active),
    ] {
        roster::add_child(&flock.folder, child(name, state)).expect("add");
        write_brief(&flock, name);
    }
    let live = run_child(&hc, &flock, "live", &coordinator);
    hc.set_status(&live, "working", None);
    hc.set_app(&live, "claude-code");
    let delivered = "---\nstatus: working\n---\nbody\n";
    write_result(&flock, "live", delivered);
    marks::mark(&flock.folder, "live", &sha256_hex(delivered.as_bytes())).expect("mark");
    write_result(&flock, "paused", "---\nstatus: done\n---\n");
    write_result(&flock, "finished", "---\nstatus: failed\n---\n");
    write_result(&flock, "broken", "no frontmatter\n");
    (flock, hc)
}

/// Every document the tests parse must satisfy the committed schema (7.2).
fn parse(bufs: &Buffers) -> Value {
    let doc: Value =
        serde_json::from_str(bufs.out.text().trim()).expect("stdout is one JSON document");
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/status.schema.json");
    let schema: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("schema")).expect("json");
    let validator = jsonschema::validator_for(&schema).expect("compiles");
    let errors: Vec<String> = validator.iter_errors(&doc).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "{errors:#?}\n{doc}");
    doc
}

#[test]
fn the_golden_json_for_a_mixed_flock() {
    let fx = Fixture::new();
    let (flock, hc) = scenario(&fx);
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("status runs");
    assert_eq!(bufs.err.text(), "");
    let dir = flock.folder.display().to_string();
    let r = |n: &str| format!("{dir}/results/{n}.md");
    let b = |n: &str| format!("{dir}/briefs/{n}.md");
    let live_terminal = json!({"id": "t2", "state": "working", "app": "claude-code"});
    let kid = |name: &str, state: &str, terminal: Value, status: &str, delivered: bool| {
        json!({
            "name": name, "role": "child", "model": "sonnet", "state": state,
            "folder": format!("/work/{name}"), "repo": null, "branch": null,
            "brief": b(name), "terminal": terminal,
            "result": {"path": r(name), "status": status, "delivered": delivered},
        })
    };
    let expected = json!({
        "version": 1,
        "crook_version": "0.0.0-dev",
        "flocks": [{
            "slug": "demo",
            "space": "crook-demo",
            "folder": dir,
            "notes_folder": dir,
            "state": "active",
            "coordinator": {
                "session_id": crate::testutil::COORD_SID,
                "terminal": {"id": "t1", "state": "idle", "app": "claude-code"},
            },
            "children": [
                kid("live", "active", live_terminal, "working", true),
                kid("paused", "active", Value::Null, "done", false),
                kid("finished", "done", Value::Null, "failed", false),
                kid("broken", "active", Value::Null, "unreadable", false),
            ],
        }],
    });
    assert_eq!(parse(&bufs), expected);
}

#[test]
fn a_missing_result_is_missing_and_not_delivered() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    roster::add_child(&flock.folder, child("a", State::Active)).expect("add");
    let hc = FakeHc::new();
    let (out, bufs) = status_of(&fx, &hc, Some("demo"), true);
    out.expect("runs");
    let doc = parse(&bufs);
    assert_eq!(
        doc["flocks"][0]["children"][0]["result"]["status"],
        "missing"
    );
    assert_eq!(
        doc["flocks"][0]["children"][0]["result"]["delivered"],
        false
    );
    assert_eq!(doc["flocks"][0]["coordinator"]["terminal"], Value::Null);
}

#[test]
fn a_changed_result_is_not_delivered_any_more() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    roster::add_child(&flock.folder, child("a", State::Active)).expect("add");
    write_result(&flock, "a", "---\nstatus: done\n---\none\n");
    marks::mark(&flock.folder, "a", &sha256_hex(b"something else")).expect("mark");
    let (out, bufs) = status_of(&fx, &FakeHc::new(), None, true);
    out.expect("runs");
    assert_eq!(
        parse(&bufs)["flocks"][0]["children"][0]["result"]["delivered"],
        false
    );
}

#[test]
fn hc_down_still_prints_the_file_side_and_exits_zero() {
    let fx = Fixture::new();
    let (flock, hc) = scenario(&fx);
    hc.fail("version", 1);
    let before = hc.calls().len();
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("D54: not an error");
    let doc = parse(&bufs);
    assert!(
        doc["hc_error"].as_str().is_some_and(|e| !e.is_empty()),
        "{doc}"
    );
    let f = &doc["flocks"][0];
    assert_eq!(f["slug"], "demo");
    assert_eq!(f["coordinator"]["terminal"], Value::Null);
    assert!(
        f["children"]
            .as_array()
            .expect("array")
            .iter()
            .all(|c| c["terminal"].is_null())
    );
    assert_eq!(f["children"][0]["result"]["status"], "working");
    assert_eq!(f["folder"], flock.folder.display().to_string());
    // Nothing after the failed preflight asked hc anything.
    assert_eq!(hc.calls().len() - before, 1);
}

#[test]
fn a_failing_ls_is_reported_too() {
    let fx = Fixture::new();
    let (_, hc) = scenario(&fx);
    hc.fail("ls", 1);
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("runs");
    assert!(parse(&bufs)["hc_error"].is_string());
}

#[test]
fn a_bare_shell_under_the_coordinators_name_is_not_a_terminal() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    hc.run(&RunSpec {
        name: "demo".to_string(),
        cwd: flock.folder.clone(),
        labels: vec![(names::LABEL_FLOCK.to_string(), "demo".to_string())],
        parent: Parent::None,
        argv: vec!["sh".to_string()],
    })
    .expect("run");
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("runs");
    assert_eq!(
        parse(&bufs)["flocks"][0]["coordinator"]["terminal"],
        Value::Null
    );
}

#[test]
fn a_slug_limits_the_output_to_that_flock() {
    let fx = Fixture::new();
    make_flock(&fx, "alpha");
    make_flock(&fx, "beta");
    let hc = FakeHc::new();
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("runs");
    let slugs: Vec<_> = parse(&bufs)["flocks"]
        .as_array()
        .expect("array")
        .iter()
        .map(|f| f["slug"].as_str().expect("slug").to_string())
        .collect();
    assert_eq!(slugs, ["alpha", "beta"]);
    let (out, bufs) = status_of(&fx, &hc, Some("beta"), true);
    out.expect("runs");
    assert_eq!(parse(&bufs)["flocks"].as_array().expect("array").len(), 1);
    let (out, _) = status_of(&fx, &hc, Some("gamma"), true);
    assert!(out.is_err());
}

#[test]
fn a_closed_flock_says_done() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    roster::mark_all_done(&flock.folder).expect("close");
    assert_eq!(load_roster(&flock).coordinator.state, State::Done);
    let (out, bufs) = status_of(&fx, &FakeHc::new(), None, true);
    out.expect("runs");
    assert_eq!(parse(&bufs)["flocks"][0]["state"], "done");
}

#[test]
fn a_broken_flock_is_warned_about_and_the_others_are_kept() {
    let fx = Fixture::new();
    make_flock(&fx, "good");
    let bad = make_flock(&fx, "bad");
    std::fs::write(bad.folder.join("roster.json"), "{").expect("break it");
    let (out, bufs) = status_of(&fx, &FakeHc::new(), None, true);
    out.expect("runs");
    let doc = parse(&bufs);
    assert_eq!(doc["flocks"].as_array().expect("array").len(), 1);
    assert!(
        bufs.err.text().contains("broken flock"),
        "{}",
        bufs.err.text()
    );
    // Asked for by name it is an error.
    let (out, _) = status_of(&fx, &FakeHc::new(), Some("bad"), true);
    assert!(out.is_err());
}

#[test]
fn the_plain_table_for_the_same_flock() {
    let fx = Fixture::new();
    let (_, hc) = scenario(&fx);
    let (out, bufs) = status_of(&fx, &hc, None, false);
    out.expect("runs");
    let text = bufs.out.text();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "flock demo (active), coordinator: t1 idle");
    assert_eq!(
        lines[1],
        "  CHILD     ROLE   MODEL   STATE   TERMINAL    RESULT"
    );
    assert_eq!(
        lines[2],
        "  live      child  sonnet  active  t2 working  working (delivered)"
    );
    assert_eq!(
        lines[3],
        "  paused    child  sonnet  active  -           done"
    );
    assert_eq!(
        lines[4],
        "  finished  child  sonnet  done    -           failed"
    );
    assert_eq!(
        lines[5],
        "  broken    child  sonnet  active  -           unreadable"
    );
    assert_eq!(lines.len(), 6, "{text}");
}

#[test]
fn the_plain_table_says_when_hc_is_down_and_when_there_is_nothing() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    let (out, bufs) = status_of(&fx, &hc, None, false);
    out.expect("runs");
    assert_eq!(bufs.out.text(), "no flocks\n");
    hc.fail("version", 1);
    let (_, bufs) = status_of(&fx, &hc, None, false);
    assert!(
        bufs.out.text().starts_with("hc is not reachable: "),
        "{}",
        bufs.out.text()
    );
}

#[test]
fn a_failed_get_affects_only_that_terminal() {
    let fx = Fixture::new();
    let (_, hc) = scenario(&fx);
    // The coordinator's `get` is the first one status makes.
    hc.fail("get", 1);
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("runs");
    let doc = parse(&bufs);
    assert!(doc.get("hc_error").is_none(), "{doc}");
    let f = &doc["flocks"][0];
    assert_eq!(
        f["coordinator"]["terminal"],
        json!({"id": "t1", "state": "idle", "app": null})
    );
    // The child's `get` still worked.
    assert_eq!(f["children"][0]["terminal"]["app"], "claude-code");
}

#[test]
fn a_failed_get_in_one_flock_does_not_hide_the_next() {
    let fx = Fixture::new();
    let (_, hc) = scenario(&fx);
    make_flock(&fx, "zeta");
    hc.fail("get", 1);
    let before = hc.calls().len();
    let (out, bufs) = status_of(&fx, &hc, None, true);
    out.expect("runs");
    assert!(parse(&bufs).get("hc_error").is_none());
    let ls_calls = hc.calls()[before..]
        .iter()
        .filter(|c| matches!(c, crate::hc::fake::Call::Ls { .. }))
        .count();
    assert_eq!(ls_calls, 2, "both flocks were listed");
}
