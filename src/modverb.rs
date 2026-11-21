//! `crook mod <verb>`: one JSON object in on stdin, one JSON object out on stdout.
//! Exit 0 carries `{"ok":true,...}`, exit 1 carries `{"ok":false,"error":"..."}`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::crew::{self, Outcome, Snap};
use crate::gate::{self, Entry, Identity, Role};
use crate::launch::child::{
    Launcher, SpawnReq, WorktreeReq, add_worktree, archive_and_stop, spawn_child,
};
use crate::project::{self, ItemKind};
use crate::store::{Mode, SCHEMA_VERSION, Store, Touch};
use crate::{Env, Error, herdr};

#[derive(Debug, Deserialize)]
pub struct Request {
    pub session_id: String,
    pub user_turn: bool,
    #[serde(flatten)]
    pub rest: Value,
}

/// The `ping` reply; `gen` is the file the watcher stats.
#[derive(Serialize)]
struct Ping<'a> {
    slug: &'a str,
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    child: Option<&'a str>,
    schema: i32,
    #[serde(rename = "gen")]
    gen_path: String,
}

fn ok(body: impl Serialize) -> Value {
    let mut v = serde_json::to_value(body).unwrap_or_else(|_| json!({}));
    if let Value::Object(m) = &mut v {
        m.insert("ok".into(), Value::Bool(true));
    }
    v
}

#[derive(Deserialize)]
struct SetWhatWhy {
    what: String,
    why: String,
    version: i64,
}

#[derive(Deserialize)]
struct SetStatus {
    text: String,
    version: i64,
}

#[derive(Deserialize)]
struct Add {
    kind: String,
    value: String,
}

fn args<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T, Error> {
    serde_json::from_value(req.rest.clone())
        .map_err(|e| Error::Refused(format!("bad arguments: {e}")))
}

/// Project writes need the orchestrator on a turn the user started. The
/// wording tells the model not to retry.
fn project_writer(id: &Identity, req: &Request) -> Result<(), Error> {
    if id.role != Role::Orchestrator {
        return Err(Error::Refused(
            "project writes are for the orchestrator only".into(),
        ));
    }
    if !req.user_turn {
        return Err(Error::Refused(
            "only on a turn the user started; don't retry".into(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct Need {
    question: String,
}

#[derive(Deserialize)]
struct Done {
    outcome: Outcome,
    pr_url: Option<String>,
}

#[derive(Deserialize)]
struct TaskArgs {
    child: String,
    text: String,
}

#[derive(Deserialize)]
struct SnapshotArgs {
    #[serde(default)]
    ack: BTreeMap<String, i64>,
}

#[derive(Deserialize)]
struct SpawnArgs {
    child: String,
    repo: String,
    brief_text: String,
    branch: Option<String>,
}

#[derive(Deserialize)]
struct WorktreeArgs {
    name: String,
    branch: Option<String>,
}

#[derive(Deserialize)]
struct StopArgs {
    child: String,
}

/// What `spawn`, `worktree` and `stop` run on: `git` and `claude`, this
/// process's environment and binary, and a fresh session id.
pub struct Machine<'a> {
    pub run: &'a dyn Launcher,
    pub base_env: &'a dyn Fn() -> Vec<(OsString, OsString)>,
    pub crook_bin: &'a dyn Fn() -> Result<PathBuf, Error>,
    pub new_id: &'a dyn Fn() -> String,
}

fn child_name(id: &Identity) -> Result<&str, Error> {
    match (id.role, id.child.as_deref()) {
        (Role::Child, Some(c)) => Ok(c),
        _ => Err(Error::Refused(
            "this verb is for a child session only".into(),
        )),
    }
}

fn orchestrator_only(id: &Identity) -> Result<(), Error> {
    if id.role == Role::Orchestrator {
        Ok(())
    } else {
        Err(Error::Refused(
            "this verb is for the orchestrator only".into(),
        ))
    }
}

/// Runs a child's write. The guard sits inside the transaction, so a session
/// that stops being the child between the check and the write cannot write.
/// `need_current: false` is for `snapshot` and `adopt`.
fn child_write<T: Serialize>(
    env: Env,
    id: &Identity,
    req: &Request,
    touches: Touch,
    need_current: bool,
    f: impl FnOnce(&rusqlite::Transaction, &str) -> Result<T, Error>,
) -> Result<Value, Error> {
    let child = child_name(id)?;
    let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
    let out = store.write(touches, |tx| {
        crew::guard(tx, child, &req.session_id, need_current)?;
        f(tx, child)
    })?;
    Ok(ok(out))
}

/// Raises a herdr notification for the child's write, once it has committed.
/// A failure goes to stderr: stdout is the reply, and the write already stands.
fn announce(env: Env, id: &Identity, what: &str) {
    let (true, Some(child)) = (herdr::present(env), id.child.as_deref()) else {
        return;
    };
    if let Err(e) = herdr::notify(env, &format!("crook: {} {what}", id.slug), child) {
        let _ = writeln!(std::io::stderr(), "crook: herdr notification failed: {e}");
    }
}

fn snapshot(env: Env, id: &Identity, req: &Request) -> Result<Value, Error> {
    let a: SnapshotArgs = args(req)?;
    let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
    let now = project::now();
    let snap = match id.role {
        Role::Child => {
            let child = child_name(id)?;
            store.read(|c| {
                crew::guard(c, child, &req.session_id, false)?;
                crew::snapshot(c, id, &req.session_id, now, &a.ack)
            })?
        }
        Role::Orchestrator if a.ack.is_empty() => {
            store.read(|c| crew::snapshot(c, id, &req.session_id, now, &a.ack))?
        }
        Role::Orchestrator => store.write(Touch::empty(), |tx| {
            crew::snapshot(tx, id, &req.session_id, now, &a.ack)
        })?,
        Role::None => {
            return Err(Error::Refused(
                "snapshot needs a child or the orchestrator".into(),
            ));
        }
    };
    Ok(match snap {
        Snap::Child(s) => ok(s),
        Snap::Orch(s) => ok(s),
    })
}

fn project_write(
    env: Env,
    id: &Identity,
    f: impl FnOnce(&rusqlite::Transaction) -> Result<i64, Error>,
) -> Result<Value, Error> {
    let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
    let version = store.write(Touch::PROJECT, f)?;
    Ok(ok(json!({ "version": version })))
}

fn dispatch(
    verb: &str,
    id: &Identity,
    req: &Request,
    env: Env,
    m: &Machine,
) -> Result<Value, Error> {
    match verb {
        "ping" => {
            let store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            Ok(ok(Ping {
                slug: &id.slug,
                role: match id.role {
                    Role::Orchestrator => "orchestrator",
                    Role::Child | Role::None => "child",
                },
                child: id.child.as_deref(),
                schema: SCHEMA_VERSION,
                gen_path: store.gen_path().display().to_string(),
            }))
        }
        "project-set-what-why" => {
            project_writer(id, req)?;
            let a: SetWhatWhy = args(req)?;
            project_write(env, id, |tx| {
                project::set_what_why(tx, &a.what, &a.why, a.version)
            })
        }
        "project-set-status" => {
            project_writer(id, req)?;
            let a: SetStatus = args(req)?;
            project_write(env, id, |tx| project::set_status(tx, &a.text, a.version))
        }
        "project-add" => {
            project_writer(id, req)?;
            let a: Add = args(req)?;
            let kind: ItemKind = a.kind.parse().map_err(Error::Refused)?;
            if !kind.mod_addable() {
                return Err(Error::Refused(format!(
                    "project-add takes repo, notes, link or ticket, not {kind}"
                )));
            }
            project_write(env, id, |tx| project::add(tx, kind, &a.value))
        }
        "status" => {
            let store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let view = store.read(|c| {
                if let Some(child) = id.child.as_deref() {
                    crew::guard(c, child, &req.session_id, true)?;
                }
                crew::status(c, id, req.user_turn)
            })?;
            Ok(ok(view))
        }
        "take-task" => child_write(
            env,
            id,
            req,
            Touch::INBOX | Touch::STATUS,
            true,
            |tx, child| {
                let (seq, text) = crew::take_task(tx, child)?;
                Ok(json!({ "seq": seq, "text": text }))
            },
        ),
        "need" => {
            let a: Need = args(req)?;
            let reply = child_write(env, id, req, Touch::STATUS, true, |tx, child| {
                crew::need(tx, child, &a.question)?;
                Ok(json!({}))
            })?;
            announce(env, id, "needs you");
            Ok(reply)
        }
        "done" => {
            let a: Done = args(req)?;
            let reply = child_write(
                env,
                id,
                req,
                Touch::STATUS | Touch::PROJECT,
                true,
                |tx, child| {
                    crew::done(tx, child, a.outcome, a.pr_url.as_deref())?;
                    Ok(json!({}))
                },
            )?;
            announce(env, id, "is done");
            Ok(reply)
        }
        "lift" => child_write(env, id, req, Touch::STATUS, true, |tx, child| {
            Ok(json!({ "lifted": crew::lift(tx, child)? }))
        }),
        "adopt" => {
            child_name(id)?;
            if !req.user_turn {
                return Err(Error::Refused(
                    "only on a turn the user started; don't retry".into(),
                ));
            }
            child_write(env, id, req, Touch::CHILDREN, false, |tx, child| {
                crew::adopt(tx, child, &req.session_id)?;
                Ok(json!({}))
            })
        }
        "task" => {
            orchestrator_only(id)?;
            let a: TaskArgs = args(req)?;
            let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let seq = store.write(Touch::INBOX, |tx| crew::task(tx, &a.child, &a.text))?;
            Ok(ok(json!({ "seq": seq })))
        }
        "spawn" => {
            orchestrator_only(id)?;
            let a: SpawnArgs = args(req)?;
            let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let session_id = spawn_child(
                &mut store,
                &SpawnReq {
                    env,
                    slug: id.slug.clone(),
                    child: a.child.clone(),
                    repo: a.repo,
                    brief: a.brief_text,
                    branch: a.branch,
                    session_id: (m.new_id)(),
                    base_env: (m.base_env)(),
                    crook_bin: (m.crook_bin)()?,
                },
                m.run,
            )?;
            Ok(ok(json!({ "child": a.child, "session_id": session_id })))
        }
        "worktree" => {
            let child = child_name(id)?.to_string();
            let a: WorktreeArgs = args(req)?;
            let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let path = add_worktree(
                &mut store,
                &WorktreeReq {
                    child,
                    session_id: req.session_id.clone(),
                    name: a.name,
                    branch: a.branch,
                },
                m.run,
            )?;
            Ok(ok(json!({ "path": path.display().to_string() })))
        }
        "stop" => {
            orchestrator_only(id)?;
            let a: StopArgs = args(req)?;
            let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let stopped = archive_and_stop(&mut store, &a.child, m.run)?;
            Ok(ok(
                json!({ "message": format!("{}: {}", a.child, stopped.describe()) }),
            ))
        }
        "snapshot" => snapshot(env, id, req),
        "orch-context" => {
            orchestrator_only(id)?;
            if !req.user_turn {
                return Err(Error::Refused(
                    "only on a turn the user started; don't retry".into(),
                ));
            }
            let mut store = Store::open(env, &id.slug, Mode::ReadWrite)?;
            let (lines, stale) = store.write(Touch::empty(), |tx| {
                Ok((
                    crew::orch_context(tx)?,
                    crew::card_stale(tx, project::now())?,
                ))
            })?;
            Ok(ok(json!({ "lines": lines, "stale": stale })))
        }
        other => Err(Error::Refused(format!("unknown mod verb {other:?}"))),
    }
}

fn run(args: &[String], env: Env, stdin: &mut dyn Read, m: &Machine) -> Result<Value, Error> {
    let id = gate::check(Entry::Mod, env)?;
    let verb = args
        .first()
        .ok_or_else(|| Error::Refused("usage: crook mod <verb>".into()))?;
    let mut raw = Vec::new();
    stdin.read_to_end(&mut raw)?;
    let req: Request =
        serde_json::from_slice(&raw).map_err(|e| Error::Refused(format!("bad request: {e}")))?;
    dispatch(verb, &id, &req, env, m)
}

/// Always writes one JSON object: `{"ok":true,...}` and exit 0, or
/// `{"ok":false,"error":"..."}` and exit 1.
pub fn main(args: &[String], env: Env, stdin: &mut dyn Read, stdout: &mut dyn Write) -> i32 {
    let run = crate::launch::real::RealLauncher::new(env);
    let machine = Machine {
        run: &run,
        base_env: &|| std::env::vars_os().collect(),
        crook_bin: &crate::launch::crook_bin,
        new_id: &|| uuid::Uuid::new_v4().to_string(),
    };
    main_with(args, env, stdin, stdout, &machine)
}

/// `main` on a machine the caller supplies.
pub fn main_with(
    args: &[String],
    env: Env,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    m: &Machine,
) -> i32 {
    let (reply, code) = match run(args, env, stdin, m) {
        Ok(v) => (v, 0),
        Err(e) => (json!({"ok": false, "error": e.to_string()}), 1),
    };
    let text = serde_json::to_string(&reply)
        .unwrap_or_else(|_| r#"{"ok":false,"error":"crook: could not encode reply"}"#.into());
    let _ = writeln!(stdout, "{text}");
    code
}

#[cfg(test)]
mod tests {
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
            fn call(
                &self,
                role: &str,
                child: Option<&str>,
                verb: &str,
                body: Value,
            ) -> (i32, Value) {
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
                format!(
                    r#"{{"session_id":"s","user_turn":false,"what":"a","why":"b","version":{v}}}"#
                ),
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
}
