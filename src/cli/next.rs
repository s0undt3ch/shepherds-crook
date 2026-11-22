//! `crook next`: jumps to the child that has waited longest.

use std::path::PathBuf;

use crate::launch::{Host, Outcome, Plan};
use crate::{Env, Error, herdr, launch};

use super::status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub slug: String,
    pub child: String,
    /// The database state: `working`, `parked` or `done`.
    pub state: String,
    pub changed_at: i64,
    /// The live agent's state, when one matches the child's session.
    pub agent_state: Option<String>,
    /// The live agent's pid, when it has one.
    pub pid: Option<u32>,
    /// The live agent's session (which may be a fork), else the child's own.
    pub session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub slug: String,
    pub child: String,
    pub pid: Option<u32>,
    pub session: String,
}

/// The oldest child that waits on the user: parked, or running an agent that is
/// `waiting` or `blocked`. A done child never counts. Ties go to the lower name.
pub fn pick(rows: &[Row]) -> Option<Target> {
    rows.iter()
        .filter(|r| r.state != "done")
        .filter(|r| {
            r.state == "parked" || matches!(r.agent_state.as_deref(), Some("waiting" | "blocked"))
        })
        .min_by(|a, b| (a.changed_at, &a.slug, &a.child).cmp(&(b.changed_at, &b.slug, &b.child)))
        .map(|r| Target {
            slug: r.slug.clone(),
            child: r.child.clone(),
            pid: r.pid,
            session: r.session.clone(),
        })
}

/// Focuses the target's herdr pane when crook runs in herdr and the session
/// has one; otherwise `claude attach <session>`. Without `claude agents` only
/// parked children can be picked, and none has a pane to find.
pub fn plan(env: Env, host: &Host) -> Result<Outcome, Error> {
    let agents = (host.agents)().unwrap_or_default();
    let Some(t) = pick(&status::rows(env, &agents)) else {
        return Ok(Outcome::Say("nothing waiting".into()));
    };
    let name = format!("{}/{}", t.slug, t.child);
    let mut notes = Vec::new();
    if herdr::present(env)
        && let Some(pane) = t.pid.and_then(|pid| herdr::pane_of_pid(env, pid))
    {
        match herdr::focus(env, &pane) {
            Ok(()) => return Ok(Outcome::Say(format!("{name}: focused pane {pane}"))),
            Err(e) => notes.push(format!(
                "crook: could not focus pane {pane}: {e}; attaching instead"
            )),
        }
    }
    notes.push(format!("crook: attaching to {name}"));
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args: vec!["attach".into(), crate::agents::short_id(&t.session).into()],
        env: host.base_env.clone(),
        cwd: PathBuf::from("."),
        notes,
    }))
}

#[cfg(test)]
mod tests;
