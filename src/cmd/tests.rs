use super::*;
use crate::hc::fake::FakeHc;
use crate::testutil::{Fixture, make_flock, start_coordinator};

fn terminal(hc: &FakeHc, flock: &crate::flock::Flock, child: Option<&str>) -> String {
    use crate::hc::{Hc, Parent, RunSpec};
    let mut labels = vec![(names::LABEL_FLOCK.to_string(), flock.slug.clone())];
    let mut name = flock.slug.clone();
    if let Some(child) = child {
        labels.push((names::LABEL_CHILD.to_string(), child.to_string()));
        name = names::hc_child(&flock.slug, child);
    }
    hc.run(&RunSpec {
        name,
        cwd: flock.folder.clone(),
        labels,
        parent: Parent::None,
        argv: vec!["claude".into()],
    })
    .expect("run")
    .address
}

#[test]
fn the_flock_comes_from_the_flag_or_the_terminals_label() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let coordinator = start_coordinator(&hc, &flock);

    fx.set_env("HC_TERMINAL_ID", &coordinator);
    let (slug, identity) = resolve_slug(&fx.ctx(), &hc, None).expect("from label");
    assert_eq!(slug, "demo");
    assert_eq!(identity.expect("identity").address, coordinator);

    let (slug, _) = resolve_slug(&fx.ctx(), &hc, Some("other")).expect("flag wins");
    assert_eq!(slug, "other");
    assert!(resolve_slug(&fx.ctx(), &hc, Some("Bad")).is_err());
    let (slug, _) = resolve_slug(&fx.ctx(), &hc, Some(&flock.dir_name())).expect("folder name");
    assert_eq!(slug, "demo");
}

#[test]
fn neither_a_terminal_nor_a_flag_says_which_to_use() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    let err = resolve_slug(&fx.ctx(), &hc, None).expect_err("refused");
    assert!(err.to_string().contains("--flock"), "{err}");
    assert!(hc.calls().is_empty());
}

#[test]
fn a_terminal_without_the_label_or_a_child_terminal_is_refused() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    use crate::hc::{Hc, Parent, RunSpec};
    let plain = hc
        .run(&RunSpec {
            name: "plain".into(),
            cwd: flock.folder.clone(),
            labels: vec![],
            parent: Parent::None,
            argv: vec!["sh".into()],
        })
        .expect("run")
        .address;
    fx.set_env("HC_TERMINAL_ID", &plain);
    let err = resolve_slug(&fx.ctx(), &hc, None).expect_err("refused");
    assert!(err.to_string().contains("not a crook terminal"), "{err}");

    let child = terminal(&hc, &flock, Some("auth"));
    fx.set_env("HC_TERMINAL_ID", &child);
    let err = resolve_slug(&fx.ctx(), &hc, None).expect_err("refused");
    assert!(err.to_string().contains("belongs to a child"), "{err}");
    // --flock does not get a child's terminal past the check.
    let err = resolve_slug(&fx.ctx(), &hc, Some("demo")).expect_err("refused");
    assert!(err.to_string().contains("belongs to a child"), "{err}");
    assert!(!err.to_string().contains("--flock"), "{err}");
    assert!(refuse_child_terminal(&fx.ctx(), &hc).is_err());
}

#[test]
fn an_unknown_terminal_id_fails_unless_a_flock_was_given() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    fx.set_env("HC_TERMINAL_ID", "t99");
    assert!(resolve_slug(&fx.ctx(), &hc, None).is_err());
    assert!(resolve_slug(&fx.ctx(), &hc, Some("demo")).is_ok());
}

#[test]
fn the_parent_is_the_calling_coordinator_else_the_live_one_else_an_error() {
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    let none = parent_terminal(&hc, "demo", None).expect_err("none live");
    assert!(none.to_string().contains("no live coordinator"), "{none}");

    let coordinator = start_coordinator(&hc, &flock);
    assert_eq!(
        parent_terminal(&hc, "demo", None).expect("found"),
        coordinator
    );
    let me = Identity {
        address: "t77".to_string(),
        flock: Some("demo".to_string()),
        child: None,
    };
    assert_eq!(
        parent_terminal(&hc, "demo", Some(&me)).expect("self"),
        "t77"
    );
    let elsewhere = Identity {
        flock: Some("other".to_string()),
        ..me
    };
    assert_eq!(
        parent_terminal(&hc, "demo", Some(&elsewhere)).expect("lookup"),
        coordinator
    );
}

#[test]
fn an_unknown_slug_lists_what_exists() {
    let fx = Fixture::new();
    let msg = find_flock(&fx.ctx(), "demo").expect_err("none").to_string();
    assert!(msg.contains("no flocks yet"), "{msg}");
    make_flock(&fx, "alpha");
    let msg = find_flock(&fx.ctx(), "demo").expect_err("none").to_string();
    assert!(msg.contains("alpha"), "{msg}");
    assert!(find_flock(&fx.ctx(), "alpha").is_ok());
}

#[test]
fn a_release_build_points_at_the_current_link_and_a_debug_one_at_its_own_plugin() {
    let fx = Fixture::new();
    let ctx = fx.ctx();
    let found = install::find_own(&ctx).expect("install");
    let release = plugin_dir(ctx.env, &found, false).expect("path");
    assert_eq!(release, fx.root().join("data/crook/current/plugin"));
    assert_eq!(
        plugin_dir(ctx.env, &found, true).expect("path"),
        found.plugin_dir()
    );
}

#[test]
fn a_shell_is_no_coordinator_for_the_parent_lookup() {
    use crate::hc::{Hc, Parent, RunSpec};
    let fx = Fixture::new();
    let flock = make_flock(&fx, "demo");
    let hc = FakeHc::new();
    hc.run(&RunSpec {
        name: "demo".into(),
        cwd: flock.folder.clone(),
        labels: vec![(names::LABEL_FLOCK.to_string(), "demo".to_string())],
        parent: Parent::None,
        argv: vec!["fish".into()],
    })
    .expect("run");
    let err = parent_terminal(&hc, "demo", None).expect_err("a shell is not a parent");
    assert!(err.to_string().contains("no live coordinator"), "{err}");
}
