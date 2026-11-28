use super::*;
use crate::cmd::spawn;
use crate::hc::fake::{Call, FakeHc};
use crate::testutil::{Buffers, Fixture, make_flock, start_coordinator, write_brief};

/// A flock `demo` with a live coordinator (the calling terminal) and one live child, `auth`.
fn world() -> (Fixture, FakeHc, String) {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    write_brief(&flock, "auth");
    let brief = flock.briefs_dir().join("auth.md").display().to_string();
    spawn::run(
        &fx.ctx(),
        &hc,
        &mut Buffers::default().io(),
        &spawn::Args {
            role: "child",
            child: "auth",
            brief: &brief,
            repo: None,
            model: None,
            flock: None,
        },
    )
    .expect("spawns");
    let child = hc
        .ls(false)
        .expect("ls")
        .into_iter()
        .find(|t| t.name.as_deref() == Some("demo.auth"))
        .expect("child")
        .address;
    (fx, hc, child)
}

fn reads(hc: &FakeHc) -> Vec<(String, u32)> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Read { id, lines } => Some((id, lines)),
            _ => None,
        })
        .collect()
}

#[test]
fn prints_the_screen_of_the_child_with_the_history_asked_for() {
    let (fx, hc, child) = world();
    let mut bufs = Buffers::default();
    run(&fx.ctx(), &hc, &mut bufs.io(), "auth", 40, None).expect("reads");
    assert_eq!(reads(&hc), [(child.clone(), 40)]);
    assert_eq!(bufs.out.text(), format!("screen of {child}\n"));
}

#[test]
fn a_blocked_child_can_still_be_read() {
    let (fx, hc, child) = world();
    hc.set_status(&child, "blocked", Some("question"));
    run(
        &fx.ctx(),
        &hc,
        &mut Buffers::default().io(),
        "auth",
        0,
        None,
    )
    .expect("reads");
    assert_eq!(reads(&hc).len(), 1);
}

#[test]
fn an_unknown_or_ended_child_is_refused_before_any_read() {
    let (fx, hc, child) = world();
    assert!(
        run(
            &fx.ctx(),
            &hc,
            &mut Buffers::default().io(),
            "nobody",
            0,
            None
        )
        .is_err()
    );
    hc.exit(&child, 0);
    assert!(
        run(
            &fx.ctx(),
            &hc,
            &mut Buffers::default().io(),
            "auth",
            0,
            None
        )
        .is_err()
    );
    assert!(reads(&hc).is_empty());
}
