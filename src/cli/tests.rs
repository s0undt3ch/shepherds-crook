use clap::{CommandFactory, Parser};

use super::{Cli, Command, Shell};

fn parse(args: &[&str]) -> Command {
    Cli::try_parse_from(std::iter::once("crook").chain(args.iter().copied()))
        .expect("parses")
        .command
}

#[test]
fn the_command_definition_is_consistent() {
    Cli::command().debug_assert();
}

#[test]
fn new_takes_a_slug_and_an_optional_task() {
    let Command::New { slug, task } = parse(&["new", "a-b", "--task", "t"]) else {
        panic!("not new");
    };
    assert_eq!((slug.as_str(), task.as_deref()), ("a-b", Some("t")));
    let Command::New { task, .. } = parse(&["new", "a-b"]) else {
        panic!("not new");
    };
    assert_eq!(task, None);
}

#[test]
fn spawn_takes_role_child_brief_and_options() {
    let Command::Spawn {
        role,
        child,
        brief,
        repo,
        model,
        flock,
    } = parse(&[
        "spawn", "r", "c", "b.md", "--repo", "/x", "--model", "haiku", "--flock", "f",
    ])
    else {
        panic!("not spawn");
    };
    assert_eq!(
        (role.as_str(), child.as_str(), brief.as_str()),
        ("r", "c", "b.md")
    );
    assert_eq!(repo.as_deref(), Some(std::path::Path::new("/x")));
    assert_eq!(
        (model.as_deref(), flock.as_deref()),
        (Some("haiku"), Some("f"))
    );
}

#[test]
fn resume_child_is_optional() {
    let Command::Resume { child, .. } = parse(&["resume"]) else {
        panic!("not resume");
    };
    assert_eq!(child, None);
}

#[test]
fn status_and_doctor_flags() {
    let Command::Status { slug, json, flock } = parse(&["status", "--json", "--flock", "f"]) else {
        panic!("not status");
    };
    assert_eq!(
        (slug.as_deref(), json, flock.as_deref()),
        (None, true, Some("f"))
    );
    let Command::Doctor { fix, json } = parse(&["doctor", "--fix"]) else {
        panic!("not doctor");
    };
    assert_eq!((fix, json), (true, false));
}

#[test]
fn completion_knows_three_shells() {
    for (name, shell) in [
        ("bash", Shell::Bash),
        ("zsh", Shell::Zsh),
        ("fish", Shell::Fish),
    ] {
        let Command::Completion(super::Completion::Print { shell: got }) =
            parse(&["completion", "print", name])
        else {
            panic!("not completion");
        };
        assert_eq!(got, shell);
    }
}

#[test]
fn hook_takes_any_event_name() {
    // A bad event must not become a clap usage error: the hook contract is exit 0 (0020).
    let Command::Hook { event } = parse(&["hook", "whatever"]) else {
        panic!("not hook");
    };
    assert_eq!(event, "whatever");
}

#[test]
fn status_slug_and_flock_conflict() {
    assert!(Cli::try_parse_from(["crook", "status", "a", "--flock", "b"]).is_err());
}

#[test]
fn only_doctor_and_setup_leave_the_current_link_alone() {
    for (args, refreshes) in [
        (&["doctor"][..], false),
        (&["doctor", "--fix"], false),
        (&["setup"], false),
        (&["status"], true),
        (&["open", "a"], true),
        (&["completion", "print", "bash"], true),
    ] {
        assert_eq!(parse(args).refreshes_link(), refreshes, "{args:?}");
    }
}
