//! herdr, the optional terminal multiplexer: a workspace per project, a tab per
//! session, notifications, and finding a session's pane. Every call runs the `herdr` the injected
//! `PATH` finds, with a timeout, and reads its JSON on stdout.

use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::launch::child::HERDR_TIMEOUT;
use crate::launch::real::{find_exe, run_captured};
use crate::{Env, Error, agents};

/// How long looking for a session's pane may take in all: one `process-info`
/// call per pane, each with its own `HERDR_TIMEOUT`.
const PANE_SEARCH_BUDGET: Duration = Duration::from_secs(15);

/// Whether crook runs inside a herdr pane, which sets `HERDR_ENV=1`. herdr
/// installed but not the one running crook counts as absent.
pub fn present(env: Env) -> bool {
    env("HERDR_ENV").as_deref() == Some("1")
}

/// `herdr` as the injected `PATH` finds it.
pub fn find(env: Env) -> Result<PathBuf, Error> {
    find_exe(env, "herdr").ok_or_else(|| Error::Refused("herdr not found on PATH".into()))
}

fn call<S: AsRef<OsStr>>(env: Env, args: impl IntoIterator<Item = S>) -> Result<String, Error> {
    let mut cmd = Command::new(find(env)?);
    cmd.args(args);
    run_captured(cmd, HERDR_TIMEOUT, "herdr")
}

fn json<S: AsRef<OsStr>>(env: Env, args: impl IntoIterator<Item = S>) -> Result<Value, Error> {
    let out = call(env, args)?;
    serde_json::from_str(&out).map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "herdr: unreadable output: {e}"
        )))
    })
}

fn str_at<'a>(v: &'a Value, pointer: &str) -> Result<&'a str, Error> {
    v.pointer(pointer).and_then(Value::as_str).ok_or_else(|| {
        Error::Io(std::io::Error::other(format!(
            "herdr: no {pointer} in its output"
        )))
    })
}

/// A project's workspace. `root` is the empty tab a new workspace starts with,
/// so the first session can use it; an existing workspace has none to give.
pub struct Workspace {
    pub id: String,
    pub root: Option<Root>,
}

/// The tab a new workspace starts with, and its pane.
pub struct Root {
    pub tab: String,
    pub pane: String,
}

/// The workspace id of the label `crook-<slug>`, if there is one.
fn find_workspace(env: Env, slug: &str) -> Result<Option<String>, Error> {
    let label = format!("crook-{slug}");
    let list = json(env, ["workspace", "list"])?;
    Ok(list
        .pointer("/result/workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|w| w["label"].as_str() == Some(&label))
        .and_then(|w| w["workspace_id"].as_str())
        .map(str::to_string))
}

/// The workspace labelled `crook-<slug>`, made in `cwd` without focus when none exists.
pub fn workspace(env: Env, slug: &str, cwd: &Path) -> Result<Workspace, Error> {
    if let Some(id) = find_workspace(env, slug)? {
        return Ok(Workspace { id, root: None });
    }
    let label = format!("crook-{slug}");
    let made = json(
        env,
        [
            OsStr::new("workspace"),
            OsStr::new("create"),
            OsStr::new("--label"),
            OsStr::new(&label),
            OsStr::new("--cwd"),
            cwd.as_os_str(),
            OsStr::new("--no-focus"),
        ],
    )?;
    Ok(Workspace {
        id: str_at(&made, "/result/workspace/workspace_id")?.to_string(),
        root: Some(Root {
            tab: str_at(&made, "/result/tab/tab_id")?.to_string(),
            pane: str_at(&made, "/result/root_pane/pane_id")?.to_string(),
        }),
    })
}

/// Whether the calling pane is in the project's workspace, by the
/// `HERDR_WORKSPACE_ID` herdr injects.
pub fn in_workspace(env: Env, slug: &str) -> Result<bool, Error> {
    let here = env("HERDR_WORKSPACE_ID").filter(|v| !v.is_empty());
    Ok(match (here, find_workspace(env, slug)?) {
        (Some(here), Some(ws)) => here == ws,
        _ => false,
    })
}

/// The longest tab label, in characters.
const LABEL_MAX: usize = 12;

/// A child's tab label: its name (the part after the dot in `<child>.<name>`),
/// else its id, cut to 11 characters and `…` when over 12. Counts characters.
pub fn child_label(child: &str) -> String {
    let name = child.split_once('.').map_or(child, |(_, n)| n);
    if name.chars().count() <= LABEL_MAX {
        return name.to_string();
    }
    let cut: String = name.chars().take(LABEL_MAX - 1).collect();
    format!("{cut}\u{2026}")
}

/// `word` as one shell word for both `sh` and `fish`: bare when it is plain,
/// else in single quotes. A backslash or a control character cannot be quoted
/// the same way in both, so it is refused.
fn shell_word(word: &OsStr) -> Result<String, Error> {
    let w = word
        .to_str()
        .ok_or_else(|| Error::Refused(format!("{word:?} is not UTF-8")))?;
    if w.chars().any(|c| c == '\\' || c.is_control()) {
        return Err(Error::Refused(format!(
            "{w:?} has a backslash or control character: it cannot go in a pane command"
        )));
    }
    let plain = !w.is_empty()
        && w.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    Ok(if plain {
        w.to_string()
    } else {
        format!("'{}'", w.replace('\'', r"'\''"))
    })
}

/// What a pane types for `command`: it runs with no `CLAUDECODE` or `CROOK_*`
/// identity of the orchestrator that spawned it, so `crook launch` passes the
/// user-verb gate. `XDG_STATE_HOME` and `XDG_DATA_HOME` ride along when the
/// orchestrator has them, so `launch` finds the same state and home. They are
/// paths. Nothing else is typed: a token would land on screen and in history.
fn pane_command(env: Env, command: &[OsString]) -> Result<String, Error> {
    let mut words = vec!["env".to_string()];
    for var in ["CLAUDECODE", "CROOK_PROJECT", "CROOK_ROLE", "CROOK_CHILD"] {
        words.extend(["-u".to_string(), var.to_string()]);
    }
    for var in ["XDG_STATE_HOME", "XDG_DATA_HOME"] {
        if let Some(v) = env(var).filter(|v| !v.is_empty()) {
            words.push(format!("{var}={}", shell_word(OsStr::new(&v))?));
        }
    }
    for w in command {
        words.push(shell_word(w)?);
    }
    Ok(words.join(" "))
}

/// Where a session's command is typed, and what undoes the start if typing fails.
struct Slot {
    pane: String,
    /// A tab this call owns: closing it leaves nothing else behind.
    tab: Option<String>,
}

fn note(msg: &str) {
    let _ = writeln!(std::io::stderr(), "crook: {msg}");
}

/// A tab of workspace `ws` with `label`, never focused: the workspace's own
/// empty root tab when it was just made (renamed), else a new tab.
fn open_tab(env: Env, ws: &Workspace, cwd: &Path, label: &str) -> Result<Slot, Error> {
    if let Some(root) = &ws.root {
        if let Err(e) = rename_tab(env, &root.tab, label) {
            note(&format!("could not label tab {}: {e}", root.tab));
        }
        return Ok(Slot {
            pane: root.pane.clone(),
            tab: Some(root.tab.clone()),
        });
    }
    let made = json(
        env,
        [
            OsStr::new("tab"),
            OsStr::new("create"),
            OsStr::new("--workspace"),
            OsStr::new(&ws.id),
            OsStr::new("--cwd"),
            cwd.as_os_str(),
            OsStr::new("--label"),
            OsStr::new(label),
            OsStr::new("--no-focus"),
        ],
    )?;
    Ok(Slot {
        pane: str_at(&made, "/result/root_pane/pane_id")?.to_string(),
        tab: Some(str_at(&made, "/result/tab/tab_id")?.to_string()),
    })
}

/// Types `command` into a tab of the project's workspace, in `cwd` and without
/// taking focus: the root tab of a workspace just made, else a new tab. If a
/// tab cannot be made, a split of the workspace's first pane stands in, with a
/// note. A tab or pane left idle by a failed `run` is closed.
pub fn spawn_tab(
    env: Env,
    slug: &str,
    cwd: &Path,
    label: &str,
    command: &[OsString],
) -> Result<(), Error> {
    let line = pane_command(env, command)?;
    let ws = workspace(env, slug, cwd)?;
    let slot = match open_tab(env, &ws, cwd, label) {
        Ok(slot) => slot,
        Err(e) => {
            note(&format!(
                "could not make a tab ({e}); splitting a pane instead"
            ));
            split(env, &ws, cwd)?
        }
    };
    run_in(env, slot, &line)
}

fn split(env: Env, ws: &Workspace, cwd: &Path) -> Result<Slot, Error> {
    let panes = json(env, ["pane", "list", "--workspace", ws.id.as_str()])?;
    let anchor = str_at(&panes, "/result/panes/0/pane_id")?.to_string();
    let made = json(
        env,
        [
            OsStr::new("pane"),
            OsStr::new("split"),
            OsStr::new(&anchor),
            OsStr::new("--direction"),
            OsStr::new("right"),
            OsStr::new("--cwd"),
            cwd.as_os_str(),
            OsStr::new("--no-focus"),
        ],
    )?;
    Ok(Slot {
        pane: str_at(&made, "/result/pane/pane_id")?.to_string(),
        tab: None,
    })
}

fn run_in(env: Env, slot: Slot, line: &str) -> Result<(), Error> {
    if let Err(e) = call(env, ["pane", "run", slot.pane.as_str(), line]) {
        let _ = match &slot.tab {
            Some(tab) => close_tab(env, tab),
            None => close(env, &slot.pane),
        };
        return Err(e);
    }
    Ok(())
}

/// Types the orchestrator's `command` into a tab of the project's workspace
/// labelled `label`, and focuses it. Returns the tab. The workspace is made in
/// `cwd` when none exists; its empty root tab is the orchestrator's.
pub fn open_orchestrator_tab(
    env: Env,
    slug: &str,
    cwd: &Path,
    label: &str,
    command: &[OsString],
) -> Result<String, Error> {
    let line = pane_command(env, command)?;
    let ws = workspace(env, slug, cwd)?;
    let slot = open_tab(env, &ws, cwd, label)?;
    let tab = slot.tab.clone().unwrap_or_default();
    run_in(env, slot, &line)?;
    focus_tab(env, &tab)?;
    Ok(tab)
}

/// Renames a tab.
pub fn rename_tab(env: Env, tab: &str, label: &str) -> Result<(), Error> {
    call(env, ["tab", "rename", tab, label]).map(drop)
}

/// Brings a tab into view.
pub fn focus_tab(env: Env, tab: &str) -> Result<(), Error> {
    call(env, ["tab", "focus", tab]).map(drop)
}

/// Closes a tab. One that is already gone is fine.
pub fn close_tab(env: Env, tab: &str) -> Result<(), Error> {
    match call(env, ["tab", "close", tab]) {
        Err(e) if e.to_string().contains("tab_not_found") => Ok(()),
        r => r.map(drop),
    }
}

/// Closes what hosts `pane`: its tab when the pane is alone in it (a child's
/// own tab), else just the pane, so a split into a shared tab leaves the rest
/// of that tab alone. Already gone is fine.
pub fn close_tab_of(env: Env, pane: &str) -> Result<(), Error> {
    let info = match json(env, ["pane", "get", pane]) {
        Ok(v) => v,
        Err(e) if e.to_string().contains("pane_not_found") => return Ok(()),
        Err(e) => return Err(e),
    };
    let tab = str_at(&info, "/result/pane/tab_id")?.to_string();
    let alone = match json(env, ["tab", "get", tab.as_str()]) {
        Ok(v) => v.pointer("/result/tab/pane_count").and_then(Value::as_u64) == Some(1),
        Err(e) if e.to_string().contains("tab_not_found") => return Ok(()),
        Err(e) => return Err(e),
    };
    if alone {
        close_tab(env, &tab)
    } else {
        close(env, pane)
    }
}

/// The pane whose foreground processes include `pid`, looked up live. Panes
/// herdr recognises as `claude` go first.
pub fn pane_of_pid(env: Env, pid: u32) -> Option<String> {
    let list = json(env, ["pane", "list"]).ok()?;
    let mut panes: Vec<(bool, &str)> = list
        .pointer("/result/panes")?
        .as_array()?
        .iter()
        .filter_map(|p| {
            Some((
                p["agent"].as_str() != Some("claude"),
                p["pane_id"].as_str()?,
            ))
        })
        .collect();
    panes.sort_by_key(|(not_claude, _)| *not_claude);
    let start = Instant::now();
    for (_, pane) in panes {
        if start.elapsed() >= PANE_SEARCH_BUDGET {
            return None;
        }
        let Ok(info) = json(env, ["pane", "process-info", "--pane", pane]) else {
            continue;
        };
        let running = info
            .pointer("/result/process_info/foreground_processes")
            .and_then(Value::as_array);
        if running
            .into_iter()
            .flatten()
            .any(|p| p["pid"].as_u64() == Some(u64::from(pid)))
        {
            return Some(pane.to_string());
        }
    }
    None
}

/// The pane running `session_id`: the one holding the session's `claude` pid
/// from `claude agents --json`. `None` for an unknown session or one with no pane.
pub fn pane_for(env: Env, session_id: &str) -> Option<String> {
    let pid = agents::list(env)
        .ok()?
        .into_iter()
        .find(|a| a.session_id == session_id)?
        .pid?;
    pane_of_pid(env, pid)
}

/// Brings `pane`, which hosts a `claude`, into view.
pub fn focus(env: Env, pane: &str) -> Result<(), Error> {
    call(env, ["agent", "focus", pane]).map(drop)
}

/// Closes `pane`. One that is already gone is fine.
pub fn close(env: Env, pane: &str) -> Result<(), Error> {
    match call(env, ["pane", "close", pane]) {
        Err(e) if e.to_string().contains("pane_not_found") => Ok(()),
        r => r.map(drop),
    }
}

/// Shows a herdr notification with the request sound.
pub fn notify(env: Env, title: &str, body: &str) -> Result<(), Error> {
    call(
        env,
        [
            "notification",
            "show",
            title,
            "--body",
            body,
            "--sound",
            "request",
        ],
    )
    .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn present_only_for_herdr_env_one() {
        let with = |v: Option<&'static str>| {
            move |k: &str| v.filter(|_| k == "HERDR_ENV").map(str::to_string)
        };
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

        fn env<'a>(
            &'a self,
            extra: &'a [(&'a str, &'a str)],
        ) -> impl Fn(&str) -> Option<String> + 'a {
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
                "pane split w7:p1 --direction right --cwd /r/.worktrees/demo/c1 --no-focus"
                    .to_string(),
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
}
