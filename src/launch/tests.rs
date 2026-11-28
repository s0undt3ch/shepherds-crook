use std::path::Path;

use super::*;

const SID: &str = "11111111-2222-4333-8444-555555555555";
const CROOK: &str = "/home/u/.local/share/mise/shims/crook";

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
        crook: CROOK,
        plugin_target: None,
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
        "CROOK_BIN=/home/u/.local/share/mise/shims/crook",
        "claude",
        "--plugin-dir",
        "/data/crook/current/plugin",
        "--add-dir",
        "/data/crook/flocks/2026-11-27-demo",
        "--settings",
        concat!(
            r#"{"permissions":{"allow":["Bash(\"/home/u/.local/share/mise/shims/crook\" *)","#,
            r#""Edit(//data/crook/flocks/2026-11-27-demo/briefs/**)","#,
            r#""Edit(//data/crook/flocks/2026-11-27-demo/README.md)","#,
            r#""Edit(//data/crook/flocks/2026-11-27-demo/notes/**)","#,
            r#""Read(//data/crook/current/plugin/**)"]}}"#
        ),
    ]);
    expected.extend(strs(&["--agent", "crook:coordinator", "--session-id", SID]));
    expected.push(
        "You are the coordinator of the crook flock with the slug demo. Its folder is /data/crook/flocks/2026-11-27-demo and \
         its notes folder is /data/crook/flocks/2026-11-27-demo. Run every crook command as \
         \"/home/u/.local/share/mise/shims/crook\" <subcommand> ..., with that exact quoted path, as its own \
         Bash command: nothing before it and no pipe, `&&` or `;` after it. Only that form runs without asking \
         the maintainer. Load the crook-coordinator skill and follow it.\n\nThe maintainer's first task: fix \
         the login bug"
            .to_string(),
    );
    assert_eq!(spec.argv, expected);
}

#[test]
fn golden_coordinator_resume_has_no_prompt_and_the_notes_folder() {
    let notes = Path::new("/notes/2026-11-27-demo");
    let c = coord(
        common(None, notes),
        Session::Resume(SID),
        Some("opus"),
        Some("ignored on a resume"),
    );
    let spec = coordinator(&c);
    let mut expected = strs(&[
        "env",
        &format!("CROOK_BIN={CROOK}"),
        "claude",
        "--plugin-dir",
        "/data/crook/current/plugin",
        "--add-dir",
        "/data/crook/flocks/2026-11-27-demo",
        "--add-dir",
        "/notes/2026-11-27-demo",
        "--settings",
    ]);
    expected.push(settings_json(&c));
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
    assert!(prompt.starts_with("You are the coordinator of the crook flock with the slug demo."));
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
}

/// ADR 0027's confirmation: no variable anywhere in the settings, and the one rule names the path the
/// first prompt tells the model to call.
#[test]
fn the_allow_rule_is_the_literal_path_the_first_prompt_names() {
    let c = coordinator(&coord(common(None, flock()), Session::New(SID), None, None));
    let settings = &c.argv[position(&c.argv, "--settings") + 1];
    assert!(!settings.contains('$'), "{settings}");
    let json: serde_json::Value = serde_json::from_str(settings).expect("json");
    let rules = json["permissions"]["allow"].as_array().expect("allow");
    let bash: Vec<_> = rules
        .iter()
        .filter(|r| r.as_str().is_some_and(|r| r.starts_with("Bash(")))
        .collect();
    assert_eq!(bash, [&serde_json::json!(format!("Bash(\"{CROOK}\" *)"))]);
    let prompt = c.argv.last().expect("prompt");
    assert!(
        prompt.contains(&format!("\"{CROOK}\" <subcommand>")),
        "{prompt}"
    );
}

#[test]
fn the_coordinator_always_passes_its_path_as_crook_bin_even_over_another() {
    for bin in [None, Some("/opt/other-crook")] {
        let c = coordinator(&coord(
            common(bin, flock()),
            Session::Resume(SID),
            None,
            None,
        ));
        assert_eq!(
            c.argv[..3],
            strs(&["env", &format!("CROOK_BIN={CROOK}"), "claude"])
        );
    }
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

/// The plugin evals' stub announces the same rule as the real hook, or the evals grade different text.
#[test]
fn the_eval_stub_says_the_same_call_rule() {
    let stub = include_str!("../../plugin/evals/_lib/crook-stub.sh");
    let expected = call_rule("%s").replace('"', "\\\"");
    assert!(stub.contains(&format!("rule='{expected}'")), "{expected}");
}

/// ADR 0028: the briefs in the flock folder, and the README and notes in the notes folder, wherever it is.
#[test]
fn the_writes_allowed_are_the_briefs_readme_and_notes() {
    let notes = Path::new("/notes/2026-11-27-demo");
    let c = coordinator(&coord(
        common(None, notes),
        Session::Resume(SID),
        None,
        None,
    ));
    let settings: serde_json::Value =
        serde_json::from_str(&c.argv[position(&c.argv, "--settings") + 1]).expect("json");
    let edits: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .expect("allow")
        .iter()
        .filter_map(|r| r.as_str().filter(|r| r.starts_with("Edit(")))
        .collect();
    assert_eq!(
        edits,
        [
            "Edit(//data/crook/flocks/2026-11-27-demo/briefs/**)",
            "Edit(//notes/2026-11-27-demo/README.md)",
            "Edit(//notes/2026-11-27-demo/notes/**)",
        ]
    );
}

#[test]
fn a_folder_a_rule_would_misread_gets_no_edit_rule() {
    for bad in [
        "/data/a*b",
        "/data/a?b",
        "/data/a[1]",
        "/data/a(b)",
        "/data/a{b}",
        "/data/a\\b",
        "/data/a!b",
    ] {
        assert_eq!(edit_rule(Path::new(bad), "briefs/**"), None, "{bad}");
    }
    assert_eq!(edit_rule(Path::new("relative/x"), "briefs/**"), None);
    assert_eq!(
        edit_rule(Path::new("/Users/a b/x"), "briefs/**").as_deref(),
        Some("Edit(//Users/a b/x/briefs/**)")
    );
}

#[test]
fn a_child_gets_no_edit_rules() {
    let k = child(&kid(common(None, flock()), Session::New(SID)));
    assert!(!k.argv.iter().any(|a| a.contains("Edit(")));
}

/// D127: the skill's own files, by the plugin link and by where it points when `crook open` runs.
#[test]
fn reads_of_the_plugin_are_allowed_by_link_and_by_target() {
    let target = Path::new("/data/crook/install/1.2.0/plugin");
    let mut c = coord(common(None, flock()), Session::Resume(SID), None, None);
    c.plugin_target = Some(target);
    let settings: serde_json::Value = serde_json::from_str(&settings_json(&c)).expect("json");
    let reads: Vec<&str> = settings["permissions"]["allow"]
        .as_array()
        .expect("allow")
        .iter()
        .filter_map(|r| r.as_str().filter(|r| r.starts_with("Read(")))
        .collect();
    assert_eq!(
        reads,
        [
            "Read(//data/crook/current/plugin/**)",
            "Read(//data/crook/install/1.2.0/plugin/**)"
        ]
    );
    c.plugin_target = Some(Path::new("/data/crook/current/plugin"));
    assert_eq!(
        settings_json(&c).matches("Read(").count(),
        1,
        "the same path once"
    );
}
