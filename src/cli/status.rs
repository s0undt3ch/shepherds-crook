//! `crook status`: every live child across projects, and sessions in a crook
//! worktree that run without the mod.

use rusqlite::Connection;

use crate::agents::{self, Agent};
use crate::store::{Mode, Store};
use crate::{Env, Error};

use super::address;
use super::next::Row;

pub(super) struct ChildInfo {
    pub child: String,
    pub worktree: String,
    pub session_id: String,
    pub forks: Vec<String>,
    pub state: String,
    pub changed_at: i64,
    pub untaken: i64,
}

impl ChildInfo {
    fn knows(&self, session: &str) -> bool {
        self.session_id == session || self.forks.iter().any(|f| f == session)
    }

    /// The live agent running this child: its recorded session, else a fork.
    pub(super) fn agent<'a>(&self, agents: &'a [Agent]) -> Option<&'a Agent> {
        agents
            .iter()
            .find(|a| a.session_id == self.session_id)
            .or_else(|| agents.iter().find(|a| self.knows(&a.session_id)))
    }
}

pub(super) fn load(c: &Connection) -> Result<Vec<ChildInfo>, Error> {
    let mut st = c.prepare(
        "SELECT ch.child, ch.worktree, ch.session_id, s.state, s.changed_at,
                (SELECT count(*) FROM inbox i WHERE i.child = ch.child AND i.taken = 0)
         FROM children ch JOIN status s USING (child)
         WHERE ch.archived = 0 ORDER BY ch.child",
    )?;
    let mut kids = st
        .query_map([], |r| {
            Ok(ChildInfo {
                child: r.get(0)?,
                worktree: r.get(1)?,
                session_id: r.get(2)?,
                forks: Vec::new(),
                state: r.get(3)?,
                changed_at: r.get(4)?,
                untaken: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut st = c.prepare("SELECT session_id FROM forks WHERE child = ?1")?;
    for k in &mut kids {
        k.forks = st
            .query_map([&k.child], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(kids)
}

/// Rows for `crook next`, one per live child of every readable project.
pub(super) fn rows(env: Env, agents: &[Agent]) -> Vec<Row> {
    let mut out = Vec::new();
    for slug in address::all_slugs(env) {
        let Ok(kids) = Store::open(env, &slug, Mode::ReadOnly).and_then(|s| s.read(load)) else {
            continue;
        };
        out.extend(kids.into_iter().map(|k| {
            let agent = k.agent(agents);
            Row {
                agent_state: agent.and_then(|a| a.state.clone()),
                pid: agent.and_then(|a| a.pid),
                session: agent.map_or_else(|| k.session_id.clone(), |a| a.session_id.clone()),
                slug: slug.clone(),
                child: k.child,
                state: k.state,
                changed_at: k.changed_at,
            }
        }));
    }
    out
}

fn project_lines(slug: &str, kids: &[ChildInfo], agents: Option<&[Agent]>) -> Vec<String> {
    let mut lines = Vec::new();
    for k in kids {
        let agent = match agents {
            None => "agent unknown".to_string(),
            Some(a) => match k.agent(a) {
                Some(x) => format!("agent {}", x.state.as_deref().unwrap_or("?")),
                None => "no live agent".to_string(),
            },
        };
        lines.push(format!(
            "{slug}/{}  {}  untaken {}  {agent}",
            k.child, k.state, k.untaken
        ));
    }
    for a in agents.unwrap_or_default() {
        if let Some(k) = kids
            .iter()
            .find(|k| !k.knows(&a.session_id) && agents::cwd_inside(&a.cwd, &k.worktree))
        {
            lines.push(format!(
                "{slug}/{}  session {} in {}: no crook mod",
                k.child, a.session_id, a.cwd
            ));
        }
    }
    lines
}

pub fn run(env: Env) -> Result<String, Error> {
    let slugs = address::all_slugs(env);
    if slugs.is_empty() {
        return Ok("no projects".into());
    }
    let agents = agents::list(env);
    let mut lines = Vec::new();
    if let Err(e) = &agents {
        lines.push(format!("crook: {e}"));
    }
    for slug in &slugs {
        match Store::open(env, slug, Mode::ReadOnly).and_then(|s| s.read(load)) {
            Ok(kids) => lines.extend(project_lines(slug, &kids, agents.as_deref().ok())),
            Err(e) => lines.push(format!("{slug}  error: {e}")),
        }
    }
    Ok(lines.join("\n"))
}
