//! `crook next`: picks the child that has waited longest. Task 20 adds the jump.

use crate::{Env, Error, agents};

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub slug: String,
    pub child: String,
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
        })
}

/// Without `claude agents` only parked children can be picked.
pub fn run(env: Env) -> Result<String, Error> {
    let agents = agents::list(env).unwrap_or_default();
    Ok(match pick(&status::rows(env, &agents)) {
        Some(t) => format!("{}/{}", t.slug, t.child),
        None => "nothing waiting".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(child: &str, state: &str, at: i64, agent: Option<&str>) -> Row {
        Row {
            slug: "p".into(),
            child: child.into(),
            state: state.into(),
            changed_at: at,
            agent_state: agent.map(Into::into),
        }
    }

    #[test]
    fn next_picks_oldest() {
        let t = |rows: &[Row]| pick(rows).map(|t| t.child);
        // A parked child and a blocked agent: the older wins, either way round.
        let rows = [
            row("parked", "parked", 100, Some("idle")),
            row("blocked", "working", 200, Some("blocked")),
        ];
        assert_eq!(t(&rows).as_deref(), Some("parked"));
        let rows = [
            row("parked", "parked", 300, None),
            row("waiting", "working", 200, Some("waiting")),
        ];
        assert_eq!(t(&rows).as_deref(), Some("waiting"));
    }

    #[test]
    fn next_skips_the_rest() {
        let rows = [
            row("busy", "working", 1, Some("running")),
            row("no-agent", "working", 2, None),
            row("finished", "done", 3, Some("blocked")),
        ];
        assert_eq!(pick(&rows), None);
        assert_eq!(pick(&[]), None);
    }

    #[test]
    fn next_ties_break_on_name() {
        let rows = [row("b", "parked", 5, None), row("a", "parked", 5, None)];
        assert_eq!(pick(&rows).map(|t| t.child).as_deref(), Some("a"));
    }
}
