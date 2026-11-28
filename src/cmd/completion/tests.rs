use super::*;
use crate::files::roster::{self, Child, State};
use crate::testutil::{Buffers, Fixture, make_flock};

fn add_child(flock: &crate::flock::Flock, name: &str) {
    roster::add_child(
        &flock.folder,
        Child {
            name: name.to_string(),
            role: "child".into(),
            model: "sonnet".into(),
            session_id: format!("00000000-0000-4000-8000-0000000000{:02}", name.len()),
            folder: format!("/work/{name}").into(),
            repo: None,
            branch: None,
            brief: format!("briefs/{name}.md"),
            state: State::Active,
            started: true,
        },
    )
    .expect("child");
}

fn printed(shell: Shell) -> String {
    let fx = Fixture::new();
    let mut bufs = Buffers::default();
    run(&fx.ctx(), &mut bufs.io(), &Completion::Print { shell }).expect("prints");
    bufs.out.text()
}

#[test]
fn every_shell_script_names_crook_and_no_path_to_the_build() {
    let exe = std::env::current_exe().expect("exe");
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let script = printed(shell);
        assert!(script.contains("crook"), "{shell:?}");
        assert!(!script.contains(&exe.display().to_string()), "{shell:?}");
        assert!(!script.contains("/target/"), "{shell:?}");
        assert!(!script.contains("CARGO_MANIFEST"), "{shell:?}");
        // The completer is the bare name, so a mise version folder never ends up in a cached script.
        assert!(script.contains("crook completion list slugs"), "{shell:?}");
    }
}

#[test]
fn every_shell_script_lists_every_subcommand_but_not_the_hidden_helper() {
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let script = printed(shell);
        for name in [
            "new",
            "open",
            "close",
            "spawn",
            "stop",
            "send",
            "read",
            "resume",
            "status",
            "hook",
            "completion",
            "setup",
            "doctor",
        ] {
            assert!(
                script
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .any(|w| w == name),
                "{shell:?} lacks {name}"
            );
        }
    }
}

#[test]
fn slug_candidates_come_from_the_flocks_root() {
    let fx = Fixture::new();
    assert!(candidates(&fx.ctx(), Kind::Slugs, None).is_empty());
    make_flock(&fx, "beta");
    make_flock(&fx, "alpha");
    assert_eq!(candidates(&fx.ctx(), Kind::Slugs, None), ["alpha", "beta"]);
}

#[test]
fn child_candidates_come_from_the_flocks_roster() {
    let fx = Fixture::new();
    let a = make_flock(&fx, "alpha");
    let b = make_flock(&fx, "beta");
    add_child(&a, "auth");
    add_child(&b, "billing");
    add_child(&b, "auth");
    assert_eq!(
        candidates(&fx.ctx(), Kind::Children, Some("beta")),
        ["auth", "billing"]
    );
    assert_eq!(
        candidates(&fx.ctx(), Kind::Children, Some("alpha")),
        ["auth"]
    );
    // Without a flock: every active flock's children, once each.
    assert_eq!(
        candidates(&fx.ctx(), Kind::Children, None),
        ["auth", "billing"]
    );
    assert!(candidates(&fx.ctx(), Kind::Children, Some("nope")).is_empty());
    assert!(candidates(&fx.ctx(), Kind::Children, Some("Bad Slug")).is_empty());
}

#[test]
fn the_list_helper_prints_one_candidate_per_line_and_never_fails() {
    let fx = Fixture::new();
    make_flock(&fx, "alpha");
    make_flock(&fx, "beta");
    let mut bufs = Buffers::default();
    run(
        &fx.ctx(),
        &mut bufs.io(),
        &Completion::List {
            kind: Kind::Slugs,
            flock: None,
        },
    )
    .expect("lists");
    assert_eq!(bufs.out.text(), "alpha\nbeta\n");
    // A broken environment is no candidates, not an error: it runs on every TAB.
    fx.set_env("XDG_DATA_HOME", "");
    fx.set_env("HOME", "");
    let mut bufs = Buffers::default();
    run(
        &fx.ctx(),
        &mut bufs.io(),
        &Completion::List {
            kind: Kind::Slugs,
            flock: None,
        },
    )
    .expect("still Ok");
    assert_eq!(bufs.out.text(), "");
}

const EXPECTED: [&str; 13] = [
    "new",
    "open",
    "close",
    "spawn",
    "stop",
    "send",
    "read",
    "resume",
    "status",
    "hook",
    "completion",
    "setup",
    "doctor",
];

fn sorted(mut words: Vec<&str>) -> Vec<&str> {
    words.sort_unstable();
    words
}

#[test]
fn the_command_list_inside_each_script_is_complete() {
    let mut want = EXPECTED.to_vec();
    want.sort_unstable();
    let bash = printed(Shell::Bash);
    let line = bash
        .lines()
        .find(|l| l.contains("words=\"new "))
        .expect("list");
    let inner = line.split('"').nth(1).expect("quoted");
    assert_eq!(sorted(inner.split(' ').collect()), want);

    let zsh = printed(Shell::Zsh);
    let line = zsh
        .lines()
        .find(|l| l.contains("candidates=(new "))
        .expect("list");
    let inner = line.split(['(', ')']).nth(1).expect("parenthesised");
    assert_eq!(sorted(inner.split(' ').collect()), want);

    let fish = printed(Shell::Fish);
    let line = fish
        .lines()
        .find(|l| l.contains("__fish_use_subcommand"))
        .expect("list");
    let inner = line.split('\'').nth(3).expect("quoted");
    assert_eq!(sorted(inner.split(' ').collect()), want);
}
