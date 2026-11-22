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
mod tests;
