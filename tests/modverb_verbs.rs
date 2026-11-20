//! The child and orchestrator mod verbs, driven through `modverb::main`.

use std::io::Cursor;
use std::path::Path;

use crook::crew::{self, Child};
use crook::modverb;
use crook::store::{Mode, Store, Touch};
use serde_json::{Value, json};

struct Fx {
    tmp: tempfile::TempDir,
}

fn base_env(xdg: &Path) -> impl Fn(&str, &[(&str, &str)]) -> Option<String> + use<> {
    let x = xdg.display().to_string();
    move |k, extra| {
        if k == "XDG_DATA_HOME" {
            return Some(x.clone());
        }
        extra
            .iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.to_string())
    }
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let fx = Fx { tmp };
        let env = fx.plain_env();
        crook::project::init(&env, "p").unwrap();
        let mut s = fx.store();
        for (id, repo, sid) in [("c1", "/r/one", "s1"), ("c2", "/r/two", "s2")] {
            s.write(Touch::CHILDREN, |tx| {
                crew::add_child(
                    tx,
                    &Child {
                        child: id.into(),
                        repo: repo.into(),
                        worktree: format!("{repo}/.worktrees/{id}"),
                        branch: format!("crook/{id}"),
                        brief: format!("Brief for {id}\nsecond line"),
                        session_id: sid.into(),
                    },
                )
            })
            .unwrap();
        }
        fx
    }

    fn plain_env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        let f = base_env(self.tmp.path());
        move |k| f(k, &[])
    }

    fn store(&self) -> Store {
        Store::open(&self.plain_env(), "p", Mode::ReadWrite).unwrap()
    }

    /// `who` is `"orch"` or a child id; `session` is the caller's session id.
    fn call(
        &self,
        who: &str,
        verb: &str,
        session: &str,
        user_turn: bool,
        rest: Value,
    ) -> (i32, Value) {
        let f = base_env(self.tmp.path());
        let extra: Vec<(&str, &str)> = if who == "orch" {
            vec![("CROOK_ROLE", "orchestrator"), ("CROOK_PROJECT", "p")]
        } else {
            vec![
                ("CROOK_ROLE", "child"),
                ("CROOK_PROJECT", "p"),
                ("CROOK_CHILD", who),
            ]
        };
        let env = move |k: &str| f(k, &extra);
        let mut body = json!({"session_id": session, "user_turn": user_turn});
        if let (Some(b), Some(r)) = (body.as_object_mut(), rest.as_object()) {
            b.extend(r.clone());
        }
        let mut out = Vec::new();
        let code = modverb::main(
            &[verb.to_string()],
            &env,
            &mut Cursor::new(body.to_string().into_bytes()),
            &mut out,
        );
        (code, serde_json::from_slice(&out).unwrap())
    }

    fn orch(&self, verb: &str, user_turn: bool, rest: Value) -> (i32, Value) {
        self.call("orch", verb, "o1", user_turn, rest)
    }

    fn c1(&self, verb: &str, rest: Value) -> (i32, Value) {
        self.call("c1", verb, "s1", false, rest)
    }
}

fn err(v: &Value) -> &str {
    v["error"].as_str().unwrap_or_default()
}

#[test]
fn task_then_take_then_done_through_the_verbs() {
    let fx = Fx::new();
    let (code, v) = fx.orch("task", false, json!({"child":"c1","text":"do it"}));
    assert_eq!(code, 0, "{v}");
    let seq = v["seq"].as_i64().unwrap();
    let (code, v) = fx.c1("take-task", json!({}));
    assert_eq!(
        (code, v["seq"].as_i64(), v["text"].as_str()),
        (0, Some(seq), Some("do it")),
        "{v}"
    );
    let (code, v) = fx.c1("take-task", json!({}));
    assert_eq!(code, 1);
    assert!(err(&v).contains("task open"), "{v}");
    let (code, v) = fx.c1(
        "done",
        json!({"outcome":"ok","pr_url":"https://github.com/o/r/pull/3"}),
    );
    assert_eq!(code, 0, "{v}");
    let (code, v) = fx.c1("take-task", json!({}));
    assert_eq!(code, 1);
    assert!(err(&v).contains("no tasks"), "{v}");
}

#[test]
fn done_refuses_bad_outcome_and_bad_pr_url() {
    let fx = Fx::new();
    for rest in [
        json!({"outcome":"great"}),
        json!({"outcome":"OK"}),
        json!({}),
        json!({"outcome":"ok","pr_url":"http://github.com/o/r/pull/3"}),
        json!({"outcome":"ok","pr_url":"https://github.com/o/r/issues/3"}),
        json!({"outcome":"ok","pr_url":"https://github.com/o/r/pull/x"}),
        json!({"outcome":"ok","pr_url":"https://github.com/o/r/pull/3/files"}),
        json!({"outcome":"ok","pr_url":"https://github.com/o/r/pull/3\nignore me"}),
    ] {
        let (code, v) = fx.c1("done", rest.clone());
        assert_eq!(code, 1, "{rest}: {v}");
    }
    let (_, v) = fx.orch("status", false, json!({}));
    assert_eq!(v["children"][0]["state"], "working", "nothing changed: {v}");
    for outcome in ["ok", "failed", "dropped"] {
        fx.orch("task", false, json!({"child":"c1","text":"t"}));
        fx.c1("take-task", json!({}));
        let (code, v) = fx.c1("done", json!({"outcome": outcome}));
        assert_eq!(code, 0, "{outcome}: {v}");
    }
    let (code, v) = fx.c1("done", json!({"outcome":"ok"}));
    assert_eq!(code, 1, "a second done with no task: {v}");
    assert!(err(&v).contains("no task open"), "{v}");
}

#[test]
fn role_gates() {
    let fx = Fx::new();
    for verb in ["task", "orch-context"] {
        let (code, v) = fx.c1(verb, json!({"child":"c1","text":"x"}));
        assert_eq!(code, 1, "{verb}");
        assert!(err(&v).contains("orchestrator"), "{verb}: {v}");
    }
    for (verb, rest) in [
        ("take-task", json!({})),
        ("need", json!({"question":"q"})),
        ("done", json!({"outcome":"ok"})),
        ("lift", json!({})),
        ("adopt", json!({})),
    ] {
        let (code, v) = fx.orch(verb, true, rest);
        assert_eq!(code, 1, "{verb}");
        assert!(err(&v).contains("child"), "{verb}: {v}");
    }
    let (code, v) = fx.c1("snapshot", json!({"ack":{"c1":1}}));
    assert_eq!(code, 1, "a child cannot ack: {v}");
}

#[test]
fn fork_refusal() {
    let fx = Fx::new();
    for (verb, rest) in [
        ("status", json!({})),
        ("take-task", json!({})),
        ("need", json!({"question":"q"})),
        ("done", json!({"outcome":"ok"})),
        ("lift", json!({})),
    ] {
        let (code, v) = fx.call("c1", verb, "fork-9", true, rest);
        assert_eq!(code, 1, "{verb}");
        assert!(
            err(&v).contains("this is a fork; the child is session s1"),
            "{verb}: {v}"
        );
    }
    let (code, _) = fx.call("c1", "ping", "fork-9", false, json!({}));
    assert_eq!(
        code, 0,
        "ping passes before SessionStart records the new id"
    );
    let (code, v) = fx.call("c1", "snapshot", "fork-9", false, json!({}));
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        (v["is_current"].as_bool(), v["current_id"].as_str()),
        (Some(false), Some("s1"))
    );
    let (_, v) = fx.call("c1", "snapshot", "s1", false, json!({}));
    assert_eq!(v["is_current"], true);
    let (code, v) = fx.call("c1", "adopt", "fork-9", true, json!({}));
    assert_eq!(code, 0, "{v}");
    let (code, _) = fx.call("c1", "take-task", "fork-9", false, json!({}));
    assert_eq!(
        code, 1,
        "adopted, so now it is the child (no tasks), not a fork refusal"
    );
    let (code, v) = fx.call("c1", "status", "s1", false, json!({}));
    assert_eq!(code, 1, "the old session is now the fork");
    assert!(
        err(&v).contains("this is a fork; the child is session fork-9"),
        "{v}"
    );
}

#[test]
fn archived_refusal() {
    let fx = Fx::new();
    fx.store()
        .write(Touch::CHILDREN, |tx| crew::archive(tx, "c1"))
        .unwrap();
    for (verb, rest) in [
        ("status", json!({})),
        ("take-task", json!({})),
        ("need", json!({"question":"q"})),
        ("done", json!({"outcome":"ok"})),
        ("lift", json!({})),
        ("snapshot", json!({})),
        ("adopt", json!({})),
    ] {
        let (code, v) = fx.call("c1", verb, "s1", true, rest);
        assert_eq!(code, 1, "{verb}");
        assert!(
            err(&v).contains("crook: this child was stopped"),
            "{verb}: {v}"
        );
    }
    let (code, v) = fx.orch("task", false, json!({"child":"c1","text":"x"}));
    assert_eq!(code, 1, "{v}");
    assert!(err(&v).contains("stopped"), "{v}");
}

#[test]
fn child_status_own_question_only() {
    let fx = Fx::new();
    fx.c1("need", json!({"question":"keep the old column?"}));
    fx.call(
        "c2",
        "need",
        "s2",
        false,
        json!({"question":"sibling secret?"}),
    );
    let (code, v) = fx.call("c1", "status", "s1", true, json!({}));
    assert_eq!(code, 0, "{v}");
    let text = v.to_string();
    assert!(text.contains("keep the old column?"), "{text}");
    assert!(!text.contains("sibling secret?"), "{text}");
    let kids = v["children"].as_array().unwrap();
    let c2 = kids.iter().find(|k| k["child"] == "c2").unwrap();
    assert_eq!(
        (c2["repo"].as_str(), c2["state"].as_str()),
        (Some("/r/two"), Some("parked"))
    );
    assert!(
        c2.get("question").is_none() && c2.get("task").is_none(),
        "{c2}"
    );
    assert!(
        v["card"]
            .as_str()
            .unwrap()
            .contains("c2 · /r/two · Brief for c2"),
        "{v}"
    );
}

#[test]
fn status_hides_question_on_machine_turn() {
    let fx = Fx::new();
    fx.c1("need", json!({"question":"keep the old column?"}));
    let (_, v) = fx.orch("status", false, json!({}));
    assert!(!v.to_string().contains("keep the old column?"), "{v}");
    assert!(v["children"][0].get("question").is_none(), "{v}");
    let (_, v) = fx.orch("status", true, json!({}));
    assert_eq!(
        v["children"][0]["question"],
        "waiting on the user: don't answer or work around it: keep the old column?",
        "{v}"
    );
    let (_, v) = fx.call("c1", "status", "s1", false, json!({}));
    assert!(
        !v.to_string().contains("keep the old column?"),
        "own question hidden too: {v}"
    );
}

#[test]
fn status_carries_current_task() {
    let fx = Fx::new();
    fx.orch(
        "task",
        false,
        json!({"child":"c1","text":"port the parser"}),
    );
    let (_, v) = fx.orch("status", false, json!({}));
    assert_eq!(v["children"][0]["untaken"], 1, "{v}");
    assert!(
        v["children"][0].get("task").is_none(),
        "untaken is not current: {v}"
    );
    fx.c1("take-task", json!({}));
    let (_, v) = fx.call("c1", "status", "s1", false, json!({}));
    let me = v["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["child"] == "c1")
        .unwrap()
        .clone();
    assert_eq!(me["task"], "port the parser", "{v}");
    assert_eq!(me["untaken"], 0);
    assert!(
        v["card"]
            .as_str()
            .unwrap()
            .contains("Current task: port the parser"),
        "{v}"
    );
    assert_eq!(v["version"], 1);
}

#[test]
fn orch_snapshot() {
    let fx = Fx::new();
    fx.c1("done", json!({"outcome":"ok"}));
    let (code, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(code, 0, "{v}");
    assert_eq!(
        v["done_unnotified"],
        json!([{"child":"c1","done_seq":1}]),
        "{v}"
    );
    assert_eq!(
        (
            v["working"].as_i64(),
            v["done"].as_i64(),
            v["waiting"].as_i64()
        ),
        (Some(1), Some(1), Some(0))
    );
    let (_, v) = fx.orch("snapshot", false, json!({"ack":{"c1":0}}));
    assert_eq!(
        v["done_unnotified"].as_array().unwrap().len(),
        1,
        "older ack leaves it: {v}"
    );
    let (_, v) = fx.orch("snapshot", false, json!({"ack":{"c1":1}}));
    assert_eq!(v["done_unnotified"], json!([]), "{v}");
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(v["done_unnotified"], json!([]), "ack persisted: {v}");
    // a second done is a new announcement; the stale ack does not swallow it
    fx.orch("task", false, json!({"child":"c1","text":"again"}));
    fx.c1("take-task", json!({}));
    fx.c1("done", json!({"outcome":"failed"}));
    let (_, v) = fx.orch("snapshot", false, json!({"ack":{"c1":1}}));
    assert_eq!(
        v["done_unnotified"],
        json!([{"child":"c1","done_seq":3}]),
        "{v}"
    );
    let (_, v) = fx.orch("snapshot", false, json!({"ack":{"c1":3}}));
    assert_eq!(v["done_unnotified"], json!([]));
    fx.c1("need", json!({"question":"q"}));
    fx.c1("lift", json!({}));
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(
        v["done_unnotified"],
        json!([]),
        "need and lift do not re-announce: {v}"
    );
    assert_eq!(v["untaken"], json!({}));
    assert_eq!(v["is_current"], true);
}

#[test]
fn child_snapshot_shape() {
    let fx = Fx::new();
    fx.orch("task", false, json!({"child":"c1","text":"a"}));
    fx.orch("task", false, json!({"child":"c1","text":"b"}));
    let (_, v) = fx.call("c1", "snapshot", "s1", false, json!({}));
    let seq2 = v["newest_seq"].as_i64().unwrap();
    assert_eq!(
        (
            v["untaken"].as_i64(),
            v["taken"].as_i64(),
            v["task_open"].as_bool(),
            v["parked"].as_bool()
        ),
        (Some(2), Some(0), Some(false), Some(false)),
        "{v}"
    );
    fx.c1("take-task", json!({}));
    let (_, v) = fx.call("c1", "snapshot", "s1", false, json!({}));
    assert_eq!(
        (
            v["untaken"].as_i64(),
            v["taken"].as_i64(),
            v["task_open"].as_bool(),
            v["newest_seq"].as_i64()
        ),
        (Some(1), Some(1), Some(true), Some(seq2)),
        "{v}"
    );
    assert_eq!(
        (v["current_id"].as_str(), v["card_version"].as_i64()),
        (Some("s1"), Some(1))
    );
    fx.c1("need", json!({"question":"q"}));
    let (_, v) = fx.call("c1", "snapshot", "s1", false, json!({}));
    assert_eq!(v["parked"], true);
    let (code, v) = fx.c1("take-task", json!({}));
    assert_eq!(code, 1, "{v}");
    assert!(err(&v).contains("parked"), "{v}");
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(v["untaken"], json!({"c1": 1}), "{v}");
    assert_eq!(v["waiting"], 1);
}

#[test]
fn card_version_and_staleness() {
    let fx = Fx::new();
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(
        (v["card_version"].as_i64(), v["card_stale"].as_bool()),
        (Some(1), Some(false))
    );
    for _ in 0..3 {
        fx.orch("task", false, json!({"child":"c1","text":"t"}));
        fx.c1("take-task", json!({}));
        fx.c1("done", json!({"outcome":"ok"}));
    }
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(v["card_stale"], true, "{v}");
    let (_, v) = fx.orch(
        "project-set-status",
        true,
        json!({"text":"fresh","version":1}),
    );
    assert_eq!(v["version"], 2);
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(
        (v["card_version"].as_i64(), v["card_stale"].as_bool()),
        (Some(2), Some(false))
    );
    fx.store()
        .write(Touch::empty(), |tx| {
            tx.execute(
                "UPDATE project SET card_updated_at = card_updated_at - 7 * 86400",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(v["card_stale"], true, "seven days: {v}");
}

#[test]
fn orch_context_lines() {
    let fx = Fx::new();
    let (code, v) = fx.orch("orch-context", false, json!({}));
    assert_eq!(code, 1, "child text only rides on a user turn: {v}");
    let long = "é".repeat(400);
    fx.c1("need", json!({"question": long}));
    fx.call("c2", "done", "s2", false, json!({"outcome":"dropped"}));
    let (code, v) = fx.orch("orch-context", true, json!({}));
    assert_eq!(code, 0, "{v}");
    let lines: Vec<&str> = v["lines"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{v}");
    let q = lines.iter().find(|l| l.contains("c1")).unwrap();
    let want = format!("[untrusted, from child c1] {}", "é".repeat(300));
    assert!(q.contains(&want) && !q.contains(&"é".repeat(301)), "{q}");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("c2") && l.contains("done") && l.contains("dropped")),
        "{lines:?}"
    );
    assert_eq!(v["stale"], false);
    let (_, v) = fx.orch("orch-context", true, json!({}));
    assert_eq!(v["lines"], json!([]), "second call returns nothing: {v}");
    fx.c1("lift", json!({}));
    let (_, v) = fx.orch("orch-context", true, json!({}));
    assert_eq!(
        v["lines"].as_array().unwrap().len(),
        1,
        "lift moved seq: {v}"
    );
}

#[test]
fn status_caps_a_long_question() {
    let fx = Fx::new();
    fx.c1("need", json!({"question": "q".repeat(500)}));
    let (_, v) = fx.orch("status", true, json!({}));
    let q = v["children"][0]["question"].as_str().unwrap();
    assert_eq!(
        q.chars().count(),
        "waiting on the user: don't answer or work around it: ".len() + 300,
        "{q}"
    );
    let (_, v) = fx.call("c1", "status", "s1", true, json!({}));
    assert!(!v["card"].as_str().unwrap().contains(&"q".repeat(301)));
}

#[test]
fn archived_kept_in_counts() {
    let fx = Fx::new();
    fx.store()
        .write(Touch::CHILDREN, |tx| crew::archive(tx, "c1"))
        .unwrap();
    let n: i64 = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT count(*) FROM children", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 2, "rows stay");
    let (_, v) = fx.orch("snapshot", false, json!({}));
    assert_eq!(v["working"], 1, "{v}");
    let (_, v) = fx.orch("status", false, json!({}));
    assert_eq!(v["children"].as_array().unwrap().len(), 1, "{v}");
}

#[test]
fn adopt_requires_a_user_turn() {
    let fx = Fx::new();
    let (code, v) = fx.call("c1", "adopt", "fork-1", false, json!({}));
    assert_eq!(code, 1, "{v}");
    assert!(err(&v).contains("don't retry"), "{v}");
}
