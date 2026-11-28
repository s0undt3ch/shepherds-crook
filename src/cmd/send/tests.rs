use super::*;
use crate::cmd::spawn;
use crate::flock::Flock;
use crate::hc::fake::{Call, FakeHc};
use crate::hc::{Parent, RunSpec};
use crate::testutil::{Buffers, Fixture, make_flock, start_coordinator, write_brief};

struct World {
    fx: Fixture,
    hc: FakeHc,
    child: String,
}

/// A flock `demo` with a live coordinator (the calling terminal) and one live child, `auth`.
fn world() -> World {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);
    fx.set_env("HC_TERMINAL_ID", &coordinator);
    let child = spawn_child(&fx, &hc, &flock, "auth");
    World { fx, hc, child }
}

fn spawn_child(fx: &Fixture, hc: &FakeHc, flock: &Flock, name: &str) -> String {
    write_brief(flock, name);
    let brief = flock
        .briefs_dir()
        .join(format!("{name}.md"))
        .display()
        .to_string();
    spawn::run(
        &fx.ctx(),
        hc,
        &mut Buffers::default().io(),
        &spawn::Args {
            role: "child",
            child: name,
            brief: &brief,
            repo: None,
            model: None,
            flock: Some(&flock.slug),
        },
    )
    .expect("spawns");
    hc.ls(false)
        .expect("ls")
        .into_iter()
        .find(|t| t.name.as_deref() == Some(&format!("{}.{name}", flock.slug)))
        .expect("the child's terminal")
        .address
}

fn send(w: &World, bufs: &mut Buffers, child: &str, text: &str) -> Result<(), Error> {
    run(&w.fx.ctx(), &w.hc, &mut bufs.io(), child, text, None)
}

fn sends(hc: &FakeHc) -> Vec<(String, String)> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Send { to, text } => Some((to, text)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_line_goes_to_the_terminal_of_the_child_as_given() {
    let w = world();
    let mut bufs = Buffers::default();
    send(&w, &mut bufs, "auth", "Also check the login page").expect("sends");
    assert_eq!(
        sends(&w.hc),
        [(w.child.clone(), "Also check the login page".to_string())]
    );
    assert_eq!(bufs.out.text(), "sent to demo.auth\n");
}

#[test]
fn a_blocked_child_gets_nothing_and_the_exit_code_is_hcs_9() {
    let w = world();
    w.hc.set_status(&w.child, "blocked", Some("question"));
    let err = send(&w, &mut Buffers::default(), "auth", "hello").expect_err("blocked");
    assert_eq!(err.exit_code(), 9);
    assert!(err.to_string().contains("blocked"), "{err}");
}

#[test]
fn an_ended_terminal_exits_with_hcs_11() {
    let w = world();
    w.hc.force_send(crate::hc::SendOutcome::Finished);
    let err = send(&w, &mut Buffers::default(), "auth", "hello").expect_err("ended");
    assert_eq!(err.exit_code(), 11);
    assert!(err.to_string().contains("crook resume auth"), "{err}");
}

#[test]
fn a_child_with_no_live_terminal_is_refused_before_any_send() {
    let w = world();
    w.hc.exit(&w.child, 0);
    let err = send(&w, &mut Buffers::default(), "auth", "hello").expect_err("not live");
    assert_eq!(err.exit_code(), 1);
    assert!(sends(&w.hc).is_empty());
}

#[test]
fn a_name_not_in_the_roster_is_refused_before_any_send() {
    let w = world();
    let err = send(&w, &mut Buffers::default(), "nobody", "hello").expect_err("unknown");
    assert!(err.to_string().contains("no child named"), "{err}");
    assert!(sends(&w.hc).is_empty());
}

#[test]
fn another_flocks_child_of_the_same_name_is_never_reached() {
    let w = world();
    let other = make_flock(&w.fx, "other");
    start_coordinator(&w.hc, &other);
    let theirs = spawn_child(&w.fx, &w.hc, &other, "auth");
    send(&w, &mut Buffers::default(), "auth", "hello").expect("sends");
    assert_eq!(sends(&w.hc), [(w.child.clone(), "hello".to_string())]);
    assert_ne!(w.child, theirs);
}

#[test]
fn a_terminal_named_like_the_child_without_the_flocks_label_is_not_it() {
    let w = world();
    w.hc.exit(&w.child, 0);
    w.hc.run(&RunSpec {
        name: "demo.auth".into(),
        cwd: w.fx.root(),
        labels: vec![],
        parent: Parent::None,
        argv: vec!["sh".into()],
    })
    .expect("run");
    assert!(send(&w, &mut Buffers::default(), "auth", "rm -rf ~").is_err());
    assert!(sends(&w.hc).is_empty());
}

#[test]
fn empty_text_is_refused() {
    let w = world();
    assert!(send(&w, &mut Buffers::default(), "auth", "  ").is_err());
    assert!(sends(&w.hc).is_empty());
}

#[test]
fn a_child_cannot_send() {
    let w = world();
    w.fx.set_env("HC_TERMINAL_ID", &w.child);
    assert!(send(&w, &mut Buffers::default(), "auth", "hello").is_err());
    assert!(sends(&w.hc).is_empty());
}
