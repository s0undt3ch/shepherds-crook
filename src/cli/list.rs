//! `crook list`: one line per project.

use nix::sys::signal::kill;
use nix::unistd::Pid;
use rusqlite::{Connection, OptionalExtension};

use crate::agents::{self, Agent};
use crate::store::{Mode, SCHEMA_VERSION, Store};
use crate::{Env, Error, crew, project};

use super::address;

const WHAT_CHARS: usize = 50;

/// Live when the pid answers `kill(pid, 0)` and the agent list shows that pid
/// running the recorded session, so a reused pid does not pass.
fn orchestrator_live(pid: Option<i64>, session: Option<&str>, agents: &[Agent]) -> bool {
    let (Some(pid), Some(session)) = (pid, session) else {
        return false;
    };
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    kill(Pid::from_raw(raw), None).is_ok()
        && agents
            .iter()
            .any(|a| a.pid.map(i64::from) == Some(pid) && a.session_id == session)
}

fn one_line(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(WHAT_CHARS)
        .collect()
}

fn count(c: &Connection, state: &str) -> Result<i64, Error> {
    Ok(c.query_row(
        "SELECT count(*) FROM status s JOIN children ch USING (child)
         WHERE ch.archived = 0 AND s.state = ?1",
        [state],
        |r| r.get(0),
    )?)
}

/// The columns after the slug. `agents` is `None` when `claude agents` failed,
/// which leaves the orchestrator's state unknown (`?`).
fn describe(env: Env, slug: &str, agents: Option<&[Agent]>, now: i64) -> Result<String, Error> {
    let store = Store::open(env, slug, Mode::ReadOnly)?;
    store.read(|c| {
        let version: i32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != SCHEMA_VERSION {
            return Ok(format!("run crook open {slug}"));
        }
        let record = project::get(c)?;
        let orch: Option<(Option<String>, Option<i64>)> = c
            .query_row(
                "SELECT session_id, pid FROM orchestrator WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (session, pid) = orch.unwrap_or_default();
        let orch_state = match agents {
            None => "?",
            Some(a) if orchestrator_live(pid, session.as_deref(), a) => "live",
            Some(_) => "-",
        };
        Ok(format!(
            "{}  working {} parked {} done {}  card {}  orchestrator {orch_state}",
            one_line(&record.what),
            count(c, "working")?,
            count(c, "parked")?,
            count(c, "done")?,
            if crew::card_stale(c, now)? {
                "stale"
            } else {
                "fresh"
            },
        ))
    })
}

pub fn run(env: Env) -> Result<String, Error> {
    let slugs = address::all_slugs(env);
    if slugs.is_empty() {
        return Ok("no projects".into());
    }
    let agents = agents::list(env).ok();
    let now = project::now();
    let width = slugs.iter().map(String::len).max().unwrap_or(0);
    // A project that cannot be read costs its own line, not the whole list.
    let lines: Vec<String> = slugs
        .iter()
        .map(|slug| {
            let body = describe(env, slug, agents.as_deref(), now)
                .unwrap_or_else(|e| format!("error: {e}"));
            format!("{slug:<width$}  {body}")
        })
        .collect();
    Ok(lines.join("\n"))
}
