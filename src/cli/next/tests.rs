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
        assert_eq!(p.args, ["attach", "sid"]);
        assert_eq!(p.program, fx.claude());
        assert!(
            !herdr_calls(&fx)
                .iter()
                .any(|c| c.starts_with("agent focus"))
        );

        // The pane is found but focus fails: attach, with the reason as a note.
        let fx = world();
        std::fs::write(
            fx.tmp.path().join("bin/herdr"),
            std::fs::read_to_string(fx.tmp.path().join("bin/herdr"))
                .unwrap()
                .replace(
                    "*) printf",
                    "'agent focus'*) echo 'no such agent' >&2; exit 1;;\n*) printf",
                ),
        )
        .unwrap();
        let Outcome::Exec(p) = next(&fx, true, vec![agent(4242)]) else {
            panic!("expected attach after a failed focus");
        };
        assert_eq!(p.args, ["attach", "sid"]);
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("could not focus pane w1:p2")),
            "{:?}",
            p.notes
        );

        // Outside herdr, herdr is not asked at all.
        let fx = world();
        let Outcome::Exec(p) = next(&fx, false, vec![agent(4242)]) else {
            panic!("expected attach");
        };
        assert_eq!(p.args, ["attach", "sid"]);
        assert!(herdr_calls(&fx).is_empty());

        // A parked child with no live agent attaches by its recorded session.
        let fx = world();
        let Outcome::Exec(p) = next(&fx, true, vec![]) else {
            panic!("expected attach");
        };
        assert_eq!(p.args, ["attach", "sid"]);
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
