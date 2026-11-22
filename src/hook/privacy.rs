//! `PreToolUse`: the privacy check on outbound tool calls. A call that names a
//! private path asks; the hit goes to the `asks` log. Quoted private text is not
//! checked: the protocol tells sessions not to quote private material.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::privacy::{Set, find_paths_by};
use crate::project::{self, Record};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config};

/// Tools whose input stays in this machine's sessions, or that only read.
const EXEMPT: [&str; 6] = ["Read", "Glob", "Grep", "Skill", "TodoWrite", "ToolSearch"];
/// Built-in subagent types that run in this machine's Claude Code. Any other
/// type may be another provider's model or a plugin's agent, so it is checked.
const LOCAL_AGENT_TYPES: [&str; 6] = [
    "general-purpose",
    "Explore",
    "Plan",
    "claude-code-guide",
    "statusline-setup",
    "fork",
];
/// The `asks` insert gives up after this, so a held lock cannot push the hook past
/// its budget. A lost row never changes the decision and is reported on stderr.
const ASKS_BUSY: Duration = Duration::from_millis(100);
/// References counted and listed per call; the rest of a huge input is not worth the work.
const MAX_REFS: usize = 512;
/// The whole check stops this long after it starts, leaving room in the 500 ms budget.
const CHECK_BY: Duration = Duration::from_millis(400);
/// The `asks` insert waits only until this long after the check starts.
const ASKS_BY: Duration = Duration::from_millis(480);
const TOO_LARGE: &str = "crook: private reference check: input too large to check";

/// Tools that never reach the privacy check, whatever they name.
pub fn exempt_tool(tool: &str, input: &Value) -> bool {
    EXEMPT.contains(&tool)
        || tool.starts_with("mcp__crook__")
        || (tool == "Agent" && local_agent(input))
}

/// A built-in subagent type, run locally. Only a known local `isolation`
/// counts; any other value, or one that is not a string, is checked.
fn local_agent(input: &Value) -> bool {
    let local = input
        .get("isolation")
        .is_none_or(|v| v.as_str() == Some("worktree"));
    let built_in = input
        .get("subagent_type")
        .is_none_or(|v| v.as_str().is_some_and(|t| LOCAL_AGENT_TYPES.contains(&t)));
    local && built_in
}

/// The file a write tool changes, joined to `cwd`.
fn write_target(tool: &str, input: &Value, cwd: &Path) -> Option<PathBuf> {
    let field = match tool {
        "Write" | "Edit" => "file_path",
        "NotebookEdit" => "notebook_path",
        _ => return None,
    };
    // `join` keeps an absolute path, and a relative `cwd` stays relative: not private.
    let t = cwd.join(input[field].as_str()?);
    t.is_absolute().then_some(t)
}

/// The reason to ask with, when the call names a private path. Logs the ask.
/// Errors are the caller's to turn into a hook error.
pub fn check(
    env: Env,
    slug: &str,
    session_id: Option<&str>,
    tool: &str,
    input: &Value,
    cwd: &Path,
    rec: &Record,
) -> Result<Option<String>, Error> {
    if exempt_tool(tool, input) {
        return Ok(None);
    }
    let started = Instant::now();
    let deadline = started + CHECK_BY;
    let cfg = config::load(env)?;
    let set = Set::load(rec, &cfg, env, slug)?;
    // Judged on where the write lands: a link out of the notes is not the notes.
    if write_target(tool, input, cwd).is_some_and(|t| set.holds(&t)) {
        return Ok(None);
    }
    let Some(hits) = find_paths_by(tool, input, cwd, &set, deadline) else {
        return Ok(Some(TOO_LARGE.to_string()));
    };
    let refs = distinct(hits.into_iter().map(|h| h.text).collect());
    let Some(first) = refs.first() else {
        return Ok(None);
    };
    let others = refs.len() - 1;
    log_ask(env, slug, session_id, tool, first, others, started);
    let mut reason = format!("crook: private reference: {first}");
    if others > 0 {
        reason.push_str(&format!(" (+{others} more)"));
    }
    Ok(Some(reason))
}

/// How long the `asks` insert may wait for the lock, started at `started` and
/// asked at `now`: `None` when the budget is spent.
fn asks_wait(started: Instant, now: Instant) -> Option<Duration> {
    let left = (started + ASKS_BY).saturating_duration_since(now);
    (!left.is_zero()).then(|| left.min(ASKS_BUSY))
}

/// Drops a reference that sits inside another one: a path found under two
/// spellings of a private root (`/var/..` inside `/private/var/..`) is one
/// reference. Quadratic, so only the first `MAX_REFS` are kept.
fn distinct(mut refs: Vec<String>) -> Vec<String> {
    refs.truncate(MAX_REFS);
    let all = refs.clone();
    refs.retain(|r| {
        !all.iter()
            .any(|o| o.len() > r.len() && o.contains(r.as_str()))
    });
    refs
}

/// Its own short-lived connection, waiting only as long as the budget leaves:
/// a lost row never changes the decision.
fn log_ask(
    env: Env,
    slug: &str,
    session: Option<&str>,
    tool: &str,
    hit: &str,
    others: usize,
    started: Instant,
) {
    let done = match asks_wait(started, Instant::now()) {
        Some(busy) => Store::open_with_busy(env, slug, Mode::ReadWrite, busy),
        None => Err(Error::Refused("no time left in the hook's budget".into())),
    }
    .and_then(|mut s| {
        s.write(Touch::empty(), |tx| {
            tx.execute(
                "INSERT INTO asks(at, session_id, tool, hit, \"others\") VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    project::now(),
                    session,
                    tool,
                    hit,
                    i64::try_from(others).unwrap_or(i64::MAX),
                ),
            )?;
            Ok(())
        })
    });
    if let Err(e) = done {
        let _ = writeln!(
            std::io::stderr(),
            "crook hook: lost asks row (tool {tool}, hit {hit}, +{others}): {e}"
        );
    }
}

#[cfg(test)]
mod tests;
