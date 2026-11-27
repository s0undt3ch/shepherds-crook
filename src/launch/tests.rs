use std::path::Path;

use super::*;

const SID: &str = "11111111-2222-4333-8444-555555555555";

fn common<'a>(bin: Option<&'a str>, notes: &'a Path) -> Common<'a> {
    Common {
        plugin_dir: Path::new("/data/crook/current/plugin"),
        crook_bin: bin,
        version: "0.1.0",
        flock_folder: flock(),
        notes_folder: notes,
    }
}

fn flock() -> &'static Path {
    Path::new("/data/crook/flocks/2026-11-27-demo")
}

fn coord<'a>(
    common: Common<'a>,
    session: Session<'a>,
    model: Option<&'a str>,
    task: Option<&'a str>,
) -> Coordinator<'a> {
    Coordinator {
        common,
        slug: "demo",
        cwd: flock(),
        session,
        model,
        task,
    }
}

fn kid<'a>(common: Common<'a>, session: Session<'a>) -> Child<'a> {
    Child {
        common,
        slug: "demo",
        name: "auth",
        role: "child",
        model: "sonnet",
        session,
        cwd: Path::new("/repo/.worktrees/demo-auth"),
        brief: Path::new("/data/crook/flocks/2026-11-27-demo/briefs/auth.md"),
        result: Path::new("/data/crook/flocks/2026-11-27-demo/results/auth.md"),
        parent: "t1",
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_string()).collect()
}

fn labels(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[test]
fn golden_coordinator_first_start() {
    let spec = coordinator(&coord(
        common(Some("/opt/crook"), flock()),
        Session::New(SID),
        None,
        Some("fix the login bug"),
    ));
    assert_eq!(spec.name, "demo");
    assert_eq!(spec.cwd, flock());
    assert_eq!(spec.parent, Parent::None);
    assert_eq!(
        spec.labels,
        labels(&[
            ("crook.flock", "demo"),
            ("crook.version", "0.1.0"),
            ("herdr.space", "crook-demo"),
            ("herdr.tab", "crook-demo-coord"),
        ])
    );
    let mut expected = strs(&[
        "env",
        "CROOK_BIN=/opt/crook",
        "claude",
        "--plugin-dir",
        "/data/crook/current/plugin",
        "--add-dir",
        "/data/crook/flocks/2026-11-27-demo",
        "--settings",
    ]);
    expected.push(settings_json());
    expected.extend(strs(&["--agent", "crook:coordinator", "--session-id", SID]));
    expected.push(
        "You are the coordinator of the crook flock demo. Its folder is /data/crook/flocks/2026-11-27-demo and \
         its notes folder is /data/crook/flocks/2026-11-27-demo. Load the crook-coordinator skill and follow \
         it.\n\nThe maintainer's first task: fix the login bug"
            .to_string(),
    );
    assert_eq!(spec.argv, expected);
}

#[test]
fn golden_coordinator_resume_has_no_prompt_and_the_notes_folder() {
    let notes = Path::new("/notes/2026-11-27-demo");
    let spec = coordinator(&coord(
        common(None, notes),
        Session::Resume(SID),
        Some("opus"),
        Some("ignored on a resume"),
    ));
    let mut expected = strs(&[
        "claude",
        "--plugin-dir",
        "/data/crook/current/plugin",
        "--add-dir",
        "/data/crook/flocks/2026-11-27-demo",
        "--add-dir",
        "/notes/2026-11-27-demo",
        "--settings",
    ]);
    expected.push(settings_json());
    expected.extend(strs(&[
        "--agent",
        "crook:coordinator",
        "--model",
        "opus",
        "--resume",
        SID,
    ]));
    assert_eq!(spec.argv, expected);
}

#[test]
fn the_first_prompt_without_a_task_is_only_the_fixed_text() {
    let spec = coordinator(&coord(common(None, flock()), Session::New(SID), None, None));
    let prompt = spec.argv.last().expect("prompt");
    assert!(prompt.starts_with("You are the coordinator of the crook flock demo."));
    assert!(prompt.ends_with("follow it."));
    assert!(!prompt.contains("first task"));
}

#[test]
fn golden_child_first_start() {
    let spec = child(&kid(common(Some("/opt/crook"), flock()), Session::New(SID)));
    assert_eq!(spec.name, "demo.auth");
    assert_eq!(spec.cwd, Path::new("/repo/.worktrees/demo-auth"));
    assert_eq!(spec.parent, Parent::Terminal("t1".to_string()));
    assert_eq!(
        spec.labels,
        labels(&[
            ("crook.flock", "demo"),
            ("crook.version", "0.1.0"),
            ("crook.child", "auth"),
        ])
    );
    let mut expected = strs(&[
        "env",
        "CROOK_BIN=/opt/crook",
        "claude",
        "--plugin-dir",
        "/data/crook/current/plugin",
        "--add-dir",
        "/data/crook/flocks/2026-11-27-demo",
        "--agent",
        "crook:child",
        "--model",
        "sonnet",
        "--session-id",
        SID,
    ]);
    expected.push(
        "Read /data/crook/flocks/2026-11-27-demo/briefs/auth.md. Write your result to \
         /data/crook/flocks/2026-11-27-demo/results/auth.md. Follow the crook-child skill: create the result \
         file first and keep its status and progress up to date, because your Stop hook reports it to the \
         coordinator."
            .to_string(),
    );
    assert_eq!(spec.argv, expected);
}

#[test]
fn golden_child_resume_names_both_paths_and_the_restart() {
    let spec = child(&kid(common(None, flock()), Session::Resume(SID)));
    let n = spec.argv.len();
    assert_eq!(spec.argv[n - 3..n - 1], strs(&["--resume", SID]));
    let prompt = &spec.argv[n - 1];
    assert!(prompt.starts_with("You were restarted."));
    assert!(prompt.contains("finish without redoing completed work"));
    assert!(prompt.contains("/briefs/auth.md") && prompt.contains("/results/auth.md"));
}

fn position(argv: &[String], what: &str) -> usize {
    argv.iter()
        .position(|a| a == what)
        .unwrap_or_else(|| panic!("{what} missing from {argv:?}"))
}

#[test]
fn every_add_dir_comes_before_agent_and_the_session_flag_is_last_but_the_prompt() {
    let notes = Path::new("/notes/x");
    for spec in [
        coordinator(&coord(common(None, notes), Session::New(SID), None, None)),
        coordinator(&coord(
            common(None, notes),
            Session::Resume(SID),
            Some("opus"),
            None,
        )),
        child(&kid(common(None, notes), Session::New(SID))),
        child(&kid(common(None, notes), Session::Resume(SID))),
    ] {
        let agent = position(&spec.argv, "--agent");
        for (i, a) in spec.argv.iter().enumerate() {
            if a == "--add-dir" {
                assert!(i + 1 < agent, "{:?}", spec.argv);
            }
        }
        let session = spec
            .argv
            .iter()
            .position(|a| a == "--session-id" || a == "--resume")
            .expect("session flag");
        assert!(session > agent);
        // The session id is the one value, so at most the prompt follows it.
        assert!(spec.argv.len() <= session + 3, "{:?}", spec.argv);
    }
}

#[test]
fn plugin_dir_is_the_given_link_path_and_ends_in_current_plugin() {
    let spec = child(&kid(common(None, flock()), Session::New(SID)));
    let i = position(&spec.argv, "--plugin-dir");
    assert!(spec.argv[i + 1].ends_with("current/plugin"));
}

#[test]
fn agent_and_model_rules() {
    let c = coordinator(&coord(common(None, flock()), Session::New(SID), None, None));
    assert!(!c.argv.contains(&"--model".to_string()));
    let c = coordinator(&coord(
        common(None, flock()),
        Session::New(SID),
        Some("opus"),
        None,
    ));
    assert_eq!(c.argv[position(&c.argv, "--model") + 1], "opus");
    let k = child(&kid(common(None, flock()), Session::New(SID)));
    assert_eq!(k.argv[position(&k.argv, "--agent") + 1], "crook:child");
    assert!(k.argv.contains(&"--model".to_string()));
    let mut other = kid(common(None, flock()), Session::New(SID));
    other.role = "reviewer";
    let k = child(&other);
    assert_eq!(k.argv[position(&k.argv, "--agent") + 1], "crook:reviewer");
}

#[test]
fn settings_appear_for_open_only() {
    let c = coordinator(&coord(common(None, flock()), Session::New(SID), None, None));
    assert!(c.argv.contains(&"--settings".to_string()));
    let k = child(&kid(common(None, flock()), Session::New(SID)));
    assert!(!k.argv.contains(&"--settings".to_string()));
    let json: serde_json::Value = serde_json::from_str(&settings_json()).expect("json");
    let rule = json["permissions"]["allow"][0].as_str().expect("rule");
    assert!(rule.starts_with("Bash(") && rule.contains(paths::SHIM_SHELL_EXPR));
}

#[test]
fn env_crook_bin_only_when_given() {
    let with = child(&kid(common(Some("/b/crook"), flock()), Session::New(SID)));
    assert_eq!(
        with.argv[..3],
        strs(&["env", "CROOK_BIN=/b/crook", "claude"])
    );
    let without = child(&kid(common(None, flock()), Session::New(SID)));
    assert_eq!(without.argv[0], "claude");
}

#[test]
fn no_notify_and_no_outlive_parent() {
    for spec in [
        coordinator(&coord(common(None, flock()), Session::New(SID), None, None)),
        child(&kid(common(None, flock()), Session::New(SID))),
    ] {
        for flag in ["--no-notify", "--outlive-parent"] {
            assert!(!spec.argv.contains(&flag.to_string()));
        }
    }
}
