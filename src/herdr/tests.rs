use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn present_only_for_herdr_env_one() {
    let with =
        |v: Option<&'static str>| move |k: &str| v.filter(|_| k == "HERDR_ENV").map(str::to_string);
    assert!(present(&with(Some("1"))));
    for v in [None, Some(""), Some("0"), Some("true")] {
        assert!(!present(&with(v)), "{v:?}");
    }
}

/// A `herdr` on its own `PATH` that logs each call's argv and answers from
/// `replies`: `(match, stdout, exit)`, the first whose `match` is in the argv.
struct Fake {
    tmp: tempfile::TempDir,
}

impl Fake {
    fn new(replies: &[(&str, &str, i32)]) -> Fake {
        let tmp = tempfile::tempdir().unwrap();
        let mut body = String::from("#!/bin/sh\necho \"$@\" >> \"$0.raw\"\n");
        for (m, out, code) in replies {
            body.push_str(&format!(
                "case \"$*\" in *'{m}'*) printf '%s' '{out}'; exit {code};; esac\n"
            ));
        }
        body.push_str("printf '%s' '{\"result\":{\"type\":\"ok\"}}'\n");
        let bin = tmp.path().join("herdr");
        std::fs::write(&bin, body).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fake { tmp }
    }

    fn env<'a>(&'a self, extra: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k: &str| {
            if k == "PATH" {
                return Some(self.tmp.path().display().to_string());
            }
            extra
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    /// The argv of each call, joined with single spaces.
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.tmp.path().join("herdr.raw"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

const HAS_WS: (&str, &str, i32) = (
    "workspace list",
    r#"{"result":{"workspaces":[{"label":"other","workspace_id":"w1"},{"label":"crook-demo","workspace_id":"w7"}]}}"#,
    0,
);
const WS_PANES: (&str, &str, i32) = (
    "pane list --workspace w7",
    r#"{"result":{"panes":[{"pane_id":"w7:p1"}]}}"#,
    0,
);
const SPLIT: (&str, &str, i32) = (
    "pane split",
    r#"{"result":{"pane":{"pane_id":"w7:p9"},"type":"pane_info"}}"#,
    0,
);
const TAB: (&str, &str, i32) = (
    "tab create",
    r#"{"result":{"tab":{"tab_id":"w7:t4"},"root_pane":{"pane_id":"w7:p8"},"type":"tab_created"}}"#,
    0,
);
const NO_WS: (&str, &str, i32) = ("workspace list", r#"{"result":{"workspaces":[]}}"#, 0);
const MADE_WS: (&str, &str, i32) = (
    "workspace create",
    r#"{"result":{"workspace":{"workspace_id":"w9"},"tab":{"tab_id":"w9:t1"},"root_pane":{"pane_id":"w9:p1"}}}"#,
    0,
);

fn os(a: &[&str]) -> Vec<OsString> {
    a.iter().map(OsString::from).collect()
}

const LAUNCH: &str = "env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD";

fn spawn(f: &Fake, extra: &[(&str, &str)]) -> Result<(), Error> {
    spawn_tab(
        &f.env(extra),
        "demo",
        Path::new("/r/.worktrees/demo/c1"),
        "c1",
        &os(&["/abs/crook", "launch", "demo/c1"]),
    )
}

#[test]
fn tab_spawn_argv() {
    // The project's workspace exists: a new tab, no focus, the command in its root pane.
    let f = Fake::new(&[HAS_WS, TAB]);
    spawn(&f, &[]).unwrap();
    assert_eq!(
        f.calls(),
        [
            "workspace list".to_string(),
            "tab create --workspace w7 --cwd /r/.worktrees/demo/c1 --label c1 --no-focus"
                .to_string(),
            format!("pane run w7:p8 {LAUNCH} '/abs/crook' launch 'demo/c1'"),
        ]
    );
}

#[test]
fn tab_spawn_new_workspace_uses_root_tab() {
    let f = Fake::new(&[NO_WS, MADE_WS]);
    spawn(&f, &[]).unwrap();
    assert_eq!(
        f.calls(),
        [
            "workspace list".to_string(),
            "workspace create --label crook-demo --cwd /r/.worktrees/demo/c1 --no-focus"
                .to_string(),
            "tab rename w9:t1 c1".to_string(),
            format!("pane run w9:p1 {LAUNCH} '/abs/crook' launch 'demo/c1'"),
        ],
        "no second tab: the new workspace's empty root tab is renamed and used"
    );
}

#[test]
fn tab_spawn_falls_back_to_split() {
    let f = Fake::new(&[HAS_WS, ("tab create", "", 1), WS_PANES, SPLIT]);
    spawn(&f, &[]).unwrap();
    assert_eq!(
        f.calls(),
        [
            "workspace list".to_string(),
            "tab create --workspace w7 --cwd /r/.worktrees/demo/c1 --label c1 --no-focus"
                .to_string(),
            "pane list --workspace w7".to_string(),
            "pane split w7:p1 --direction right --cwd /r/.worktrees/demo/c1 --no-focus".to_string(),
            format!("pane run w7:p9 {LAUNCH} '/abs/crook' launch 'demo/c1'"),
        ]
    );
}

#[test]
fn a_failed_tab_create_is_noted_on_stderr() {
    // The note goes to this process's stderr; run the real thing as a child to see it.
    let f = Fake::new(&[HAS_WS, ("tab create", "", 1), WS_PANES, SPLIT]);
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "herdr::tests::stderr_note_child", "--nocapture"])
        .env("CROOK_TEST_HERDR_DIR", f.tmp.path())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("crook: could not make a tab") && err.contains("splitting a pane instead"),
        "{err}"
    );
}

#[test]
fn stderr_note_child() {
    let Ok(dir) = std::env::var("CROOK_TEST_HERDR_DIR") else {
        return;
    };
    let env = move |k: &str| (k == "PATH").then(|| dir.clone());
    spawn_tab(&env, "demo", Path::new("/w"), "c1", &os(&["/c"])).unwrap();
}

#[test]
fn tab_spawn_carries_only_the_xdg_paths() {
    let f = Fake::new(&[HAS_WS, TAB]);
    let extra = [
        ("XDG_DATA_HOME", "/d ata"),
        ("XDG_STATE_HOME", "/s'tate"),
        ("ANTHROPIC_API_KEY", "sk-secret"),
        ("CROOK_ROLE", "orchestrator"),
    ];
    spawn(&f, &extra).unwrap();
    let run = f.calls().pop().unwrap();
    assert_eq!(
        run,
        format!(
            "pane run w7:p8 {LAUNCH} XDG_STATE_HOME='/s'\\''tate' XDG_DATA_HOME='/d ata' \
             '/abs/crook' launch 'demo/c1'"
        )
    );
    assert!(!run.contains("sk-secret") && !run.contains("orchestrator"));
}

#[test]
fn tab_spawn_refuses_what_fish_would_read_differently() {
    let f = Fake::new(&[HAS_WS, TAB]);
    for bad in ["/a\\b/crook", "/a\nb/crook", "/a\u{1b}b"] {
        let env = f.env(&[]);
        let e = spawn_tab(
            &env,
            "demo",
            Path::new("/w"),
            "c",
            &os(&[bad, "launch", "d/c"]),
        )
        .unwrap_err();
        assert!(matches!(e, Error::Refused(_)), "{bad:?}: {e}");
    }
    let env = f.env(&[("XDG_STATE_HOME", "/s\\x")]);
    assert!(spawn_tab(&env, "demo", Path::new("/w"), "c", &os(&["/c"])).is_err());
    assert!(f.calls().is_empty(), "nothing was made: {:?}", f.calls());
}

#[test]
fn tab_spawn_closes_the_tab_when_run_fails() {
    let f = Fake::new(&[HAS_WS, TAB, ("pane run", "", 1)]);
    assert!(spawn(&f, &[]).is_err());
    assert_eq!(
        f.calls().last().map(String::as_str),
        Some("tab close w7:t4")
    );
    // A split stand-in has only its pane to close.
    let f = Fake::new(&[
        HAS_WS,
        ("tab create", "", 1),
        WS_PANES,
        SPLIT,
        ("pane run", "", 1),
    ]);
    assert!(spawn(&f, &[]).is_err());
    assert_eq!(
        f.calls().last().map(String::as_str),
        Some("pane close w7:p9")
    );
}

#[test]
fn a_failed_label_does_not_stop_the_spawn() {
    let f = Fake::new(&[NO_WS, MADE_WS, ("tab rename", "", 1)]);
    spawn(&f, &[]).unwrap();
    assert!(f.calls().last().unwrap().starts_with("pane run w9:p1"));
}

#[test]
fn tab_spawn_without_herdr_on_path_is_refused() {
    let env = |k: &str| (k == "PATH").then(|| "/nonexistent".to_string());
    let e = spawn_tab(&env, "demo", Path::new("/w"), "c", &os(&["/c"])).unwrap_err();
    assert!(e.to_string().contains("herdr not found"), "{e}");
}

#[test]
fn child_label() {
    assert_eq!(super::child_label("c3.auth"), "auth");
    assert_eq!(super::child_label("c3"), "c3");
    assert_eq!(super::child_label("exactly-12-ch"), "exactly-12-\u{2026}");
    assert_eq!(super::child_label("123456789012"), "123456789012");
    let twenty = "abcdefghijklmnopqrst";
    assert_eq!(super::child_label(twenty), "abcdefghijk\u{2026}");
    // Characters, not bytes: each of these is two bytes.
    let multi = "\u{e9}".repeat(20);
    let want = format!("{}\u{2026}", "\u{e9}".repeat(11));
    assert_eq!(super::child_label(&multi), want);
    assert_eq!(
        super::child_label(&"\u{e9}".repeat(12)),
        "\u{e9}".repeat(12)
    );
    // The cut applies to the name after the dot.
    assert_eq!(
        super::child_label("c3.abcdefghijklmnop"),
        "abcdefghijk\u{2026}"
    );
}

const PANE_GET: (&str, &str, i32) = (
    "pane get w1:p3",
    r#"{"result":{"pane":{"pane_id":"w1:p3","tab_id":"w1:t4"}}}"#,
    0,
);

#[test]
fn close_closes_the_tab() {
    let f = Fake::new(&[
        PANE_GET,
        ("tab get w1:t4", r#"{"result":{"tab":{"pane_count":1}}}"#, 0),
    ]);
    close_tab_of(&f.env(&[]), "w1:p3").unwrap();
    assert_eq!(
        f.calls(),
        ["pane get w1:p3", "tab get w1:t4", "tab close w1:t4"]
    );
}

#[test]
fn close_leaves_a_shared_tab_alone() {
    let f = Fake::new(&[
        PANE_GET,
        ("tab get w1:t4", r#"{"result":{"tab":{"pane_count":2}}}"#, 0),
    ]);
    close_tab_of(&f.env(&[]), "w1:p3").unwrap();
    assert_eq!(
        f.calls().last().map(String::as_str),
        Some("pane close w1:p3")
    );
}

#[test]
fn close_missing_tab_is_ok() {
    let gone = |what: &str| {
        let f = Fake::new(&[]);
        std::fs::write(
            f.tmp.path().join("herdr"),
            format!(
                "#!/bin/sh\ncase \"$*\" in\n\
                 'pane get'*) printf '%s' '{{\"result\":{{\"pane\":{{\"tab_id\":\"w1:t4\"}}}}}}';;\n\
                 'tab get'*) printf '%s' '{{\"result\":{{\"tab\":{{\"pane_count\":1}}}}}}';;\n\
                 *) echo '{what}' >&2; exit 1;;\nesac\n"
            ),
        )
        .unwrap();
        f
    };
    let f = gone(r#"{"error":{"code":"tab_not_found"}}"#);
    close_tab_of(&f.env(&[]), "w1:p3").unwrap();
    close_tab(&f.env(&[]), "w1:t4").unwrap();
    let f = gone("boom");
    assert!(close_tab(&f.env(&[]), "w1:t4").is_err());
}

#[test]
fn rename_and_focus_tab_argv() {
    let f = Fake::new(&[]);
    rename_tab(&f.env(&[]), "w1:t4", "\u{1F415}").unwrap();
    focus_tab(&f.env(&[]), "w1:t4").unwrap();
    assert_eq!(f.calls(), ["tab rename w1:t4 \u{1F415}", "tab focus w1:t4"]);
}

#[test]
fn orchestrator_tab_in_a_new_workspace_uses_its_root_tab_and_focuses_it() {
    let f = Fake::new(&[NO_WS, MADE_WS]);
    let tab = open_orchestrator_tab(
        &f.env(&[("ANTHROPIC_API_KEY", "sk-secret")]),
        "demo",
        Path::new("/p/demo"),
        "\u{1F415}",
        &os(&["/abs/crook", "open", "demo"]),
    )
    .unwrap();
    assert_eq!(tab, "w9:t1");
    assert_eq!(
        f.calls(),
        [
            "workspace list".to_string(),
            "workspace create --label crook-demo --cwd /p/demo --no-focus".to_string(),
            "tab rename w9:t1 \u{1F415}".to_string(),
            format!("pane run w9:p1 {LAUNCH} '/abs/crook' open demo"),
            "tab focus w9:t1".to_string(),
        ]
    );
}

#[test]
fn in_workspace_compares_the_callers_workspace() {
    let f = Fake::new(&[HAS_WS]);
    assert!(in_workspace(&f.env(&[("HERDR_WORKSPACE_ID", "w7")]), "demo").unwrap());
    assert!(!in_workspace(&f.env(&[("HERDR_WORKSPACE_ID", "w1")]), "demo").unwrap());
    assert!(!in_workspace(&f.env(&[]), "demo").unwrap());
    let f = Fake::new(&[NO_WS]);
    assert!(!in_workspace(&f.env(&[("HERDR_WORKSPACE_ID", "w7")]), "demo").unwrap());
}

#[test]
fn workspace_finds_or_creates() {
    let f = Fake::new(&[HAS_WS]);
    let env = f.env(&[]);
    let w = workspace(&env, "demo", Path::new("/w")).unwrap();
    assert_eq!(w.id.as_str(), "w7");
    assert!(w.root.is_none());
    assert_eq!(f.calls(), ["workspace list"]);

    let f = Fake::new(&[NO_WS, MADE_WS]);
    let env = f.env(&[]);
    let w = workspace(&env, "demo", Path::new("/w")).unwrap();
    let root = w.root.unwrap();
    assert_eq!(
        (w.id.as_str(), root.tab.as_str(), root.pane.as_str()),
        ("w9", "w9:t1", "w9:p1")
    );
}

fn panes_json() -> &'static str {
    r#"{"result":{"panes":[
        {"pane_id":"w1:p1"},
        {"pane_id":"w1:p2","agent":"claude"},
        {"pane_id":"w1:p3","agent":"claude"}]}}"#
}

#[test]
fn pane_of_pid_matches_foreground_processes_and_tries_claude_panes_first() {
    let info = |pid: u32| {
        format!(
            r#"{{"result":{{"process_info":{{"foreground_processes":[{{"pid":1}},{{"pid":{pid}}}]}}}}}}"#
        )
    };
    let (a, b) = (info(500), info(4242));
    let f = Fake::new(&[
        ("pane list", panes_json(), 0),
        ("process-info --pane w1:p2", &a, 0),
        ("process-info --pane w1:p3", &b, 0),
    ]);
    let env = f.env(&[]);
    assert_eq!(pane_of_pid(&env, 4242).as_deref(), Some("w1:p3"));
    // The plain pane was never asked: the claude panes came first and one matched.
    assert_eq!(
        f.calls(),
        [
            "pane list",
            "pane process-info --pane w1:p2",
            "pane process-info --pane w1:p3"
        ]
    );
    assert_eq!(pane_of_pid(&env, 77), None);
}

#[test]
fn pane_of_pid_skips_a_pane_that_fails() {
    let ok = r#"{"result":{"process_info":{"foreground_processes":[{"pid":4242}]}}}"#;
    let f = Fake::new(&[
        ("pane list", panes_json(), 0),
        ("process-info --pane w1:p2", "", 1),
        ("process-info --pane w1:p3", ok, 0),
    ]);
    let env = f.env(&[]);
    assert_eq!(pane_of_pid(&env, 4242).as_deref(), Some("w1:p3"));
}

#[test]
fn pane_for_reads_the_pid_from_claude_agents() {
    let f = Fake::new(&[
        ("pane list", panes_json(), 0),
        (
            "process-info --pane w1:p3",
            r#"{"result":{"process_info":{"foreground_processes":[{"pid":4242}]}}}"#,
            0,
        ),
    ]);
    std::fs::write(
        f.tmp.path().join("claude"),
        "#!/bin/sh\nprintf '%s' '[{\"sessionId\":\"s1\",\"pid\":4242,\"cwd\":\"/w\",\"kind\":\"interactive\"},\
         {\"sessionId\":\"bg\",\"cwd\":\"/w\",\"kind\":\"background\"}]'\n",
    )
    .unwrap();
    std::fs::set_permissions(
        f.tmp.path().join("claude"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let env = f.env(&[]);
    assert_eq!(pane_for(&env, "s1").as_deref(), Some("w1:p3"));
    // A background session has no pid, so no pane.
    assert_eq!(pane_for(&env, "bg"), None);
    assert_eq!(pane_for(&env, "unknown"), None);
}

#[test]
fn focus_runs_agent_focus() {
    let f = Fake::new(&[]);
    focus(&f.env(&[]), "w1:p3").unwrap();
    assert_eq!(f.calls(), ["agent focus w1:p3"]);
    let f = Fake::new(&[("agent focus", "", 1)]);
    assert!(focus(&f.env(&[]), "w1:p3").is_err());
}

#[test]
fn close_treats_a_missing_pane_as_closed() {
    let gone = r#"{"error":{"code":"pane_not_found"}}"#;
    let f = Fake::new(&[]);
    close(&f.env(&[]), "w1:p3").unwrap();
    assert_eq!(f.calls(), ["pane close w1:p3"]);
    // The error JSON goes to stderr in real herdr; the stand-in prints it
    // there and exits 1.
    let f = Fake::new(&[]);
    let bin = f.tmp.path().join("herdr");
    std::fs::write(&bin, format!("#!/bin/sh\necho '{gone}' >&2\nexit 1\n")).unwrap();
    close(&f.env(&[]), "w1:p3").unwrap();
    std::fs::write(&bin, "#!/bin/sh\necho 'boom' >&2\nexit 1\n").unwrap();
    assert!(close(&f.env(&[]), "w1:p3").is_err());
}

#[test]
fn notify_argv() {
    let f = Fake::new(&[]);
    notify(&f.env(&[]), "crook: demo needs you", "c1").unwrap();
    assert_eq!(
        f.calls(),
        ["notification show crook: demo needs you --body c1 --sound request"]
    );
    let f = Fake::new(&[("notification", "", 1)]);
    assert!(notify(&f.env(&[]), "t", "b").is_err());
}
