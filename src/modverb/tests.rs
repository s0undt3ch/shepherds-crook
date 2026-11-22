use std::collections::HashMap;
use std::io::Cursor;

use super::*;

struct Fixture {
    _tmp: tempfile::TempDir,
    vars: HashMap<String, String>,
}

fn fixture(role: &str) -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let mut vars = HashMap::from([
        (
            "XDG_STATE_HOME".to_string(),
            tmp.path().display().to_string(),
        ),
        ("CROOK_ROLE".to_string(), role.to_string()),
        ("CROOK_PROJECT".to_string(), "p".to_string()),
    ]);
    if role == "child" {
        vars.insert("CROOK_CHILD".to_string(), "c1".to_string());
    }
    Fixture { _tmp: tmp, vars }
}

fn call(f: &Fixture, args: &[&str], input: &str) -> (i32, Value) {
    let env = |k: &str| f.vars.get(k).cloned();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut out = Vec::new();
    let code = main(&args, &env, &mut Cursor::new(input.as_bytes()), &mut out);
    let text = String::from_utf8(out).unwrap();
    let v: Value = serde_json::from_str(text.trim()).expect("stdout is one JSON object");
    (code, v)
}

fn migrate(f: &Fixture) {
    let env = |k: &str| f.vars.get(k).cloned();
    Store::open(&env, "p", Mode::Migrate).unwrap();
}

const GOOD: &str = r#"{"session_id":"s1","user_turn":false}"#;

mod machine {
    use super::super::*;
    use crate::agents::Agent;
    use crate::launch::child::fake::{Call, Fake};
    use crate::launch::fixture::Fx;
    use crate::project::ItemKind;

    struct M {
        fx: Fx,
        repo: PathBuf,
        fake: Fake,
    }

    fn m() -> M {
        let fx = Fx::new();
        fx.init("demo");
        let repo = std::fs::canonicalize(fx.tmp.path()).unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, ItemKind::Repo, &repo.display().to_string())
            })
            .unwrap();
        M {
            fx,
            repo,
            fake: Fake::default(),
        }
    }

    impl M {
        fn call(&self, role: &str, child: Option<&str>, verb: &str, body: Value) -> (i32, Value) {
            let base = self.fx.env();
            let (role, child) = (role.to_string(), child.map(str::to_string));
            let env = move |k: &str| match k {
                "CROOK_ROLE" => Some(role.clone()),
                "CROOK_PROJECT" => Some("demo".into()),
                "CROOK_CHILD" => child.clone(),
                _ => base(k),
            };
            let machine = Machine {
                run: &self.fake,
                base_env: &Vec::new,
                crook_bin: &|| Ok(PathBuf::from("/abs/crook")),
                new_id: &|| "new-sid".to_string(),
            };
            let mut out = Vec::new();
            let args = [verb.to_string()];
            let input = body.to_string();
            let code = main_with(
                &args,
                &env,
                &mut std::io::Cursor::new(input.into_bytes()),
                &mut out,
                &machine,
            );
            (code, serde_json::from_slice(&out).unwrap())
        }

        fn spawn(&self) -> (i32, Value) {
            self.call(
                "orchestrator",
                None,
                "spawn",
                json!({"session_id":"o","user_turn":false,"child":"c1",
                       "repo": self.repo.display().to_string(), "brief_text":"do it"}),
            )
        }
    }

    #[test]
    fn spawn_then_worktree_then_stop() {
        let m = m();
        let (code, v) = m.spawn();
        assert_eq!(
            (code, v["session_id"].as_str()),
            (0, Some("new-sid")),
            "{v}"
        );
        let wt = m.repo.join(".worktrees/demo/c1");
        assert!(matches!(&m.fake.calls()[0], Call::Add(_, p, _) if *p == wt));

        // The child's own worktree.
        let (code, v) = m.call(
            "child",
            Some("c1"),
            "worktree",
            json!({"session_id":"new-sid","user_turn":false,"name":"fix","path":"/etc"}),
        );
        let want = m.repo.join(".worktrees/demo/c1.fix");
        assert_eq!(
            (code, v["path"].as_str()),
            (0, Some(want.display().to_string().as_str())),
            "{v}"
        );
        // An orchestrator cannot make one.
        let (code, v) = m.call(
            "orchestrator",
            None,
            "worktree",
            json!({"session_id":"o","user_turn":false,"name":"x"}),
        );
        assert_eq!(code, 1);
        assert!(v["error"].as_str().unwrap().contains("child"), "{v}");

        // The tool's stop: archive and stop, no worktree removed.
        let fake = Fake {
            agents: vec![Agent {
                session_id: "new-sid".into(),
                pid: None,
                cwd: "/x".into(),
                kind: "background".into(),
                state: None,
            }],
            ..Fake::default()
        };
        let m = M { fake, ..m };
        let (code, v) = m.call(
            "orchestrator",
            None,
            "stop",
            json!({"session_id":"o","user_turn":false,"child":"c1"}),
        );
        assert_eq!(code, 0, "{v}");
        assert!(v["message"].as_str().unwrap().contains("c1"), "{v}");
        assert_eq!(m.fake.calls(), [Call::StopBg("new-sid".into())]);
        let rows =
            m.fx.store("demo")
                .read(|c| crew::worktrees(c, Some("c1")))
                .unwrap();
        assert_eq!(rows.len(), 2, "the tool leaves the worktrees");
    }

    #[test]
    fn spawn_stop_and_worktree_are_role_gated() {
        let m = m();
        let body = json!({"session_id":"s","user_turn":true,"child":"c1","repo":"/r",
                          "brief_text":"b","name":"x"});
        for verb in ["spawn", "stop"] {
            let (code, v) = m.call("child", Some("c1"), verb, body.clone());
            assert_eq!(code, 1, "{verb}");
            assert!(v["error"].as_str().unwrap().contains("orchestrator"), "{v}");
        }
        let (code, _) = m.call(
            "orchestrator",
            None,
            "spawn",
            json!({"session_id":"s","user_turn":false}),
        );
        assert_eq!(code, 1, "bad arguments");
        assert!(m.fake.calls().is_empty());
    }
}

#[test]
fn ping_reports_identity_and_gen_path() {
    let f = fixture("child");
    migrate(&f);
    let (code, v) = call(&f, &["ping"], GOOD);
    assert_eq!(code, 0);
    let gen_path = format!("{}/crook/p/gen", f.vars["XDG_STATE_HOME"]);
    assert_eq!(
        v,
        json!({"ok":true,"slug":"p","role":"child","child":"c1","schema":1,"gen":gen_path})
    );
}

#[test]
fn ping_as_orchestrator_has_no_child() {
    let f = fixture("orchestrator");
    migrate(&f);
    let (code, v) = call(&f, &["ping"], GOOD);
    assert_eq!(code, 0);
    assert_eq!(v["role"], "orchestrator");
    assert!(v.get("child").is_none());
}

#[test]
fn missing_database_says_run_crook_open_and_creates_nothing() {
    let f = fixture("child");
    let (code, v) = call(&f, &["ping"], GOOD);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert!(v["error"].as_str().unwrap().contains("run crook open p"));
    assert_eq!(
        std::fs::read_dir(&f.vars["XDG_STATE_HOME"])
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn bad_input_is_a_json_error() {
    let f = fixture("child");
    migrate(&f);
    for input in [
        "",
        "not json",
        "[1]",
        "{}",
        r#"{"session_id":1,"user_turn":false}"#,
        r#"{"session_id":"s","user_turn":"no"}"#,
        "\u{0}\u{ff}",
    ] {
        let (code, v) = call(&f, &["ping"], input);
        assert_eq!(code, 1, "{input:?}");
        assert_eq!(v["ok"], false, "{input:?}");
        assert!(
            v["error"].as_str().is_some_and(|e| !e.is_empty()),
            "{input:?}"
        );
    }
}

#[test]
fn invalid_utf8_stdin_is_a_json_error() {
    let f = fixture("child");
    let env = |k: &str| f.vars.get(k).cloned();
    let mut out = Vec::new();
    let code = main(
        &["ping".to_string()],
        &env,
        &mut Cursor::new(vec![0xff, 0xfe]),
        &mut out,
    );
    assert_eq!(code, 1);
    let v: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
}

#[test]
fn unknown_or_missing_verb_is_a_json_error() {
    let f = fixture("child");
    migrate(&f);
    let (code, v) = call(&f, &["nope"], GOOD);
    assert_eq!(code, 1);
    assert!(v["error"].as_str().unwrap().contains("nope"));
    let (code, v) = call(&f, &[], GOOD);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
}

#[test]
fn gate_refusal_is_a_json_error() {
    let mut f = fixture("child");
    f.vars.insert("CLAUDECODE".into(), "1".into());
    let (code, v) = call(&f, &["ping"], GOOD);
    assert_eq!(code, 1);
    assert!(v["error"].as_str().unwrap().contains("CLAUDECODE"));
}

#[test]
fn old_schema_says_run_crook_open() {
    let f = fixture("child");
    migrate(&f);
    let db = format!("{}/crook/p/crook.db", f.vars["XDG_STATE_HOME"]);
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.pragma_update(None, "user_version", 0).unwrap();
    drop(conn);
    let (code, v) = call(&f, &["ping"], GOOD);
    assert_eq!(code, 1);
    assert!(v["error"].as_str().unwrap().contains("run crook open p"));
}

fn init_project(f: &Fixture) {
    let env = |k: &str| f.vars.get(k).cloned();
    crate::project::init(&env, "p").unwrap();
}

fn version(f: &Fixture) -> i64 {
    let env = |k: &str| f.vars.get(k).cloned();
    let s = Store::open(&env, "p", Mode::ReadOnly).unwrap();
    s.read(|c| Ok(crate::project::get(c)?.version)).unwrap()
}

const USER: &str = r#""session_id":"s1","user_turn":true"#;

#[test]
fn mod_set_refuses_machine_turn() {
    let f = fixture("orchestrator");
    init_project(&f);
    let v = version(&f);
    for (verb, body) in [
        (
            "project-set-status",
            format!(r#"{{"session_id":"s","user_turn":false,"text":"x","version":{v}}}"#),
        ),
        (
            "project-set-what-why",
            format!(r#"{{"session_id":"s","user_turn":false,"what":"a","why":"b","version":{v}}}"#),
        ),
        (
            "project-add",
            r#"{"session_id":"s","user_turn":false,"kind":"link","value":"https://x.test"}"#
                .to_string(),
        ),
    ] {
        let (code, out) = call(&f, &[verb], &body);
        assert_eq!(code, 1, "{verb}");
        assert!(
            out["error"]
                .as_str()
                .unwrap()
                .contains("only on a turn the user started; don't retry"),
            "{verb}: {out}"
        );
    }
    assert_eq!(version(&f), v, "nothing changed");
}

#[test]
fn mod_set_refuses_children() {
    let f = fixture("child");
    init_project(&f);
    let v = version(&f);
    let body = format!(r#"{{{USER},"text":"x","version":{v}}}"#);
    let (code, out) = call(&f, &["project-set-status"], &body);
    assert_eq!(code, 1);
    assert!(
        out["error"].as_str().unwrap().contains("orchestrator"),
        "{out}"
    );
    let body = format!(r#"{{{USER},"kind":"link","value":"u"}}"#);
    let (code, _) = call(&f, &["project-add"], &body);
    assert_eq!(code, 1);
    assert_eq!(version(&f), v);
}

#[test]
fn project_verbs_write_on_a_user_turn() {
    let f = fixture("orchestrator");
    init_project(&f);
    let v = version(&f);
    let body = format!(r#"{{{USER},"what":"w","why":"y","version":{v}}}"#);
    let (code, out) = call(&f, &["project-set-what-why"], &body);
    assert_eq!((code, out["version"].as_i64()), (0, Some(v + 1)), "{out}");
    let body = format!(r#"{{{USER},"text":"st","version":{}}}"#, v + 1);
    let (code, out) = call(&f, &["project-set-status"], &body);
    assert_eq!((code, out["version"].as_i64()), (0, Some(v + 2)), "{out}");
    let body = format!(r#"{{{USER},"kind":"notes","value":"/n/notes"}}"#);
    let (code, out) = call(&f, &["project-add"], &body);
    assert_eq!((code, out["version"].as_i64()), (0, Some(v + 3)), "{out}");
    let env = |k: &str| f.vars.get(k).cloned();
    let s = Store::open(&env, "p", Mode::ReadOnly).unwrap();
    s.read(|c| {
        let r = crate::project::get(c)?;
        assert_eq!(
            (r.what.as_str(), r.why.as_str(), r.status_text.as_str()),
            ("w", "y", "st")
        );
        assert_eq!(
            (r.notes, r.private),
            (vec!["/n/notes".to_string()], vec!["/n/notes".to_string()])
        );
        Ok(())
    })
    .unwrap();
    assert!(
        std::fs::metadata(format!("{}/crook/p/gen", f.vars["XDG_STATE_HOME"])).is_ok(),
        "a project write bumps gen"
    );
}

#[test]
fn stale_version_is_refused_and_bad_args_are_json_errors() {
    let f = fixture("orchestrator");
    init_project(&f);
    let v = version(&f);
    let body = format!(r#"{{{USER},"text":"x","version":{}}}"#, v - 1);
    let (code, out) = call(&f, &["project-set-status"], &body);
    assert_eq!(code, 1);
    assert!(out["error"].as_str().unwrap().contains("changed"), "{out}");
    for (verb, body) in [
        ("project-set-status", format!(r#"{{{USER}}}"#)),
        (
            "project-add",
            format!(r#"{{{USER},"kind":"memory","value":"x"}}"#),
        ),
        (
            "project-add",
            format!(r#"{{{USER},"kind":"repo","value":"rel"}}"#),
        ),
        (
            "project-add",
            format!(r#"{{{USER},"kind":"private","value":"/x"}}"#),
        ),
        (
            "project-add",
            format!(r#"{{{USER},"kind":"allow","value":"x"}}"#),
        ),
    ] {
        let (code, out) = call(&f, &[verb], &body);
        assert_eq!(code, 1, "{verb} {body}");
        assert_eq!(out["ok"], false);
    }
    assert_eq!(version(&f), v);
}
