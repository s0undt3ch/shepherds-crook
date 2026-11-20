//! `PreToolUse`: park denial for children, and the parked-target asks for
//! the orchestrator's `stop` and `spawn`.

use rusqlite::Connection;
use serde_json::Value;

use crate::Error;
use crate::crew;
use crate::gate::{Identity, Role};

/// The one tool a parked child may still call.
const STATUS_TOOL: &str = "mcp__crook__status";

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Deny(String),
    Ask(String),
}

/// `None` is "no decision". A parked child is denied unless it is a recorded
/// fork: the recorded child session and any unrecorded session are denied.
pub fn check(
    c: &Connection,
    id: &Identity,
    session_id: Option<&str>,
    tool: &str,
    tool_input: &Value,
) -> Result<Option<Decision>, Error> {
    match (id.role, id.child.as_deref()) {
        (Role::Child, Some(child)) => child_check(c, child, session_id, tool),
        (Role::Orchestrator, _) => orchestrator_check(c, tool, tool_input),
        _ => Ok(None),
    }
}

fn child_check(
    c: &Connection,
    child: &str,
    session_id: Option<&str>,
    tool: &str,
) -> Result<Option<Decision>, Error> {
    if !crew::parked(c, child)? || tool == STATUS_TOOL {
        return Ok(None);
    }
    let allowed = match session_id {
        Some(s) => !crew::is_current(c, child, s)?.0 && crew::is_fork(c, child, s)?,
        None => false,
    };
    if allowed {
        return Ok(None);
    }
    Ok(Some(Decision::Deny(format!(
        "crook: {child} is parked on a question for the user. Only mcp__crook__status works until they answer; don't work around it."
    ))))
}

fn orchestrator_check(
    c: &Connection,
    tool: &str,
    tool_input: &Value,
) -> Result<Option<Decision>, Error> {
    match tool {
        "mcp__crook__stop" => {
            let Some(child) = tool_input["child"].as_str() else {
                return Ok(None);
            };
            if crew::parked_lenient(c, child)? {
                return Ok(Some(Decision::Ask(format!(
                    "crook: {child} is parked on a question for the user; stopping it drops that question"
                ))));
            }
        }
        "mcp__crook__spawn" => {
            let Some(repo) = tool_input["repo"].as_str() else {
                return Ok(None);
            };
            if let Some(child) = crew::parked_in_repo(c, repo)? {
                return Ok(Some(Decision::Ask(format!(
                    "crook: {child} is parked in this repo on a question for the user"
                ))));
            }
        }
        _ => {}
    }
    Ok(None)
}
