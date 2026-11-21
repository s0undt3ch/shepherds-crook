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
    if herdr::present(env)
        && let Some(pane) = t.pid.and_then(|pid| herdr::pane_of_pid(env, pid))
    {
        herdr::focus(env, &pane)?;
        return Ok(Outcome::Say(format!("{name}: focused pane {pane}")));
    }
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args: vec!["attach".into(), t.session.into()],
        env: host.base_env.clone(),
        cwd: PathBuf::from("."),
        notes: vec![format!("crook: attaching to {name}")],
    }))
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
            pid: None,
            session: format!("sid-{child}"),
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

    mod jump {
        use std::os::unix::fs::PermissionsExt;

        use super::super::*;
        use crate::agents::Agent;
        use crate::crew;
        use crate::launch::fixture::Fx;
        use crate::store::Touch;

        /// A parked child `c1` of `demo` on session `sid-c1`, and a `herdr` that
        /// logs its argv and shows claude's pid 4242 in pane `w1:p2`.
        fn world() -> Fx {
            let fx = Fx::new();
            fx.init("demo");
            fx.add_child("demo", "c1", &fx.tmp.path().join("wt"), "sid-c1", None);
            fx.store("demo")
                .write(Touch::STATUS, |tx| crew::need(tx, "c1", "q?"))
                .unwrap();
            let herdr = fx.tmp.path().join("bin/herdr");
            let log = fx.tmp.path().join("herdr.log");
            std::fs::write(
                &herdr,
                format!(
                    "#!/bin/sh\necho \"$@\" >> {}\n\
                     case \"$*\" in\n\
                     'pane list') printf '%s' '{{\"result\":{{\"panes\":[{{\"pane_id\":\"w1:p2\",\"agent\":\"claude\"}}]}}}}';;\n\
                     'pane process-info --pane w1:p2') printf '%s' '{{\"result\":{{\"process_info\":{{\"foreground_processes\":[{{\"pid\":4242}}]}}}}}}';;\n\
                     *) printf '%s' '{{\"result\":{{\"type\":\"ok\"}}}}';;\n\
                     esac\n",
                    log.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
            fx
        }

        fn agent(pid: u32) -> Agent {
            Agent {
                session_id: "sid-c1".into(),
                pid: Some(pid),
                cwd: "/w".into(),
                kind: "interactive".into(),
                state: Some("waiting".into()),
            }
        }

        fn next(fx: &Fx, in_herdr: bool, agents: Vec<Agent>) -> Outcome {
            let base = fx.env();
            let env = move |k: &str| match k {
                "HERDR_ENV" if in_herdr => Some("1".to_string()),
                _ => base(k),
            };
            let version = || Ok("2.1.329".to_string());
            let list = move || Ok(agents.clone());
            let new_id = || "unused".to_string();
            plan(&env, &fx.host(&|_| false, &list, &version, &new_id)).unwrap()
        }

        fn herdr_calls(fx: &Fx) -> Vec<String> {
            std::fs::read_to_string(fx.tmp.path().join("herdr.log"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string)
                .collect()
        }

        #[test]
        fn next_focuses_or_attaches() {
            // A pane holds the session's pid: it is focused and nothing is exec'd.
            let fx = world();
            match next(&fx, true, vec![agent(4242)]) {
                Outcome::Say(s) => assert!(s.contains("demo/c1") && s.contains("w1:p2"), "{s}"),
                Outcome::Exec(_) => panic!("attached instead of focusing"),
            }
            assert_eq!(
                herdr_calls(&fx).last().map(String::as_str),
                Some("agent focus w1:p2")
            );

            // No pane has the pid: `claude attach <session id>`.
            let fx = world();
            let Outcome::Exec(p) = next(&fx, true, vec![agent(9999)]) else {
                panic!("expected attach");
            };
            assert_eq!(p.args, ["attach", "sid-c1"]);
            assert_eq!(p.program, fx.claude());
            assert!(
                !herdr_calls(&fx)
                    .iter()
                    .any(|c| c.starts_with("agent focus"))
            );

            // Outside herdr, herdr is not asked at all.
            let fx = world();
            let Outcome::Exec(p) = next(&fx, false, vec![agent(4242)]) else {
                panic!("expected attach");
            };
            assert_eq!(p.args, ["attach", "sid-c1"]);
            assert!(herdr_calls(&fx).is_empty());

            // A parked child with no live agent attaches by its recorded session.
            let fx = world();
            let Outcome::Exec(p) = next(&fx, true, vec![]) else {
                panic!("expected attach");
            };
            assert_eq!(p.args, ["attach", "sid-c1"]);
        }

        #[test]
        fn next_with_nothing_waiting_says_so() {
            let fx = Fx::new();
            fx.init("demo");
            match next(&fx, true, vec![]) {
                Outcome::Say(s) => assert_eq!(s, "nothing waiting"),
                Outcome::Exec(_) => panic!("nothing is waiting"),
            }
        }
    }
}
