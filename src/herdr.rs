//! herdr, the optional terminal multiplexer: a pane per child, notifications,
//! and finding a session's pane. Every call runs the `herdr` the injected
//! `PATH` finds, with a timeout, and reads its JSON on stdout.

use std::ffi::{OsStr, OsString};
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

/// The id of the workspace labelled `crook-<slug>`, made when none exists.
pub fn workspace(env: Env, slug: &str) -> Result<String, Error> {
    let label = format!("crook-{slug}");
    let list = json(env, ["workspace", "list"])?;
    let found = list
        .pointer("/result/workspaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|w| w["label"].as_str() == Some(&label))
        .and_then(|w| w["workspace_id"].as_str());
    if let Some(id) = found {
        return Ok(id.to_string());
    }
    let made = json(
        env,
        ["workspace", "create", "--label", &label, "--no-focus"],
    )?;
    Ok(str_at(&made, "/result/workspace/workspace_id")?.to_string())
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

/// Splits a pane beside the calling one, in `cwd` and without taking focus,
/// and types `command` into it. `--current` puts it in the workspace of the
/// orchestrator that spawned it, so `_ws` is not used to place it. A pane left
/// empty by a failed `run` is closed.
pub fn spawn_pane(env: Env, _ws: &str, cwd: &Path, command: &[OsString]) -> Result<(), Error> {
    let line = pane_command(env, command)?;
    let made = json(
        env,
        [
            OsStr::new("pane"),
            OsStr::new("split"),
            OsStr::new("--current"),
            OsStr::new("--direction"),
            OsStr::new("right"),
            OsStr::new("--cwd"),
            cwd.as_os_str(),
            OsStr::new("--no-focus"),
        ],
    )?;
    let pane = str_at(&made, "/result/pane/pane_id")?.to_string();
    if let Err(e) = call(env, ["pane", "run", pane.as_str(), line.as_str()]) {
        let _ = close(env, &pane);
        return Err(e);
    }
    Ok(())
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

    const SPLIT: (&str, &str, i32) = (
        "pane split",
        r#"{"result":{"pane":{"pane_id":"w1:p9"},"type":"pane_info"}}"#,
        0,
    );

    fn os(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn spawn_pane_argv() {
        let f = Fake::new(&[SPLIT]);
        let env = f.env(&[]);
        spawn_pane(
            &env,
            "w1",
            Path::new("/r/.worktrees/demo/c1"),
            &os(&["/abs/crook", "launch", "demo/c1"]),
        )
        .unwrap();
        assert_eq!(
            f.calls(),
            [
                "pane split --current --direction right --cwd /r/.worktrees/demo/c1 --no-focus",
                "pane run w1:p9 env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD \
                 '/abs/crook' launch 'demo/c1'",
            ]
        );
    }

    #[test]
    fn spawn_pane_carries_only_the_xdg_paths() {
        let f = Fake::new(&[SPLIT]);
        let env = f.env(&[
            ("XDG_DATA_HOME", "/d ata"),
            ("XDG_STATE_HOME", "/s'tate"),
            ("ANTHROPIC_API_KEY", "sk-secret"),
            ("CROOK_ROLE", "orchestrator"),
        ]);
        spawn_pane(
            &env,
            "w1",
            Path::new("/w"),
            &os(&["/abs/crook", "launch", "demo/c1"]),
        )
        .unwrap();
        let run = f.calls().remove(1);
        assert_eq!(
            run,
            "pane run w1:p9 env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD \
             XDG_STATE_HOME='/s'\\''tate' XDG_DATA_HOME='/d ata' '/abs/crook' launch 'demo/c1'"
        );
        assert!(!run.contains("sk-secret") && !run.contains("orchestrator"));
    }

    #[test]
    fn spawn_pane_refuses_what_fish_would_read_differently() {
        let f = Fake::new(&[SPLIT]);
        for bad in ["/a\\b/crook", "/a\nb/crook", "/a\u{1b}b"] {
            let env = f.env(&[]);
            let e =
                spawn_pane(&env, "w1", Path::new("/w"), &os(&[bad, "launch", "d/c"])).unwrap_err();
            assert!(matches!(e, Error::Refused(_)), "{bad:?}: {e}");
        }
        let env = f.env(&[("XDG_STATE_HOME", "/s\\x")]);
        assert!(spawn_pane(&env, "w1", Path::new("/w"), &os(&["/c"])).is_err());
        assert!(f.calls().is_empty(), "nothing was split: {:?}", f.calls());
    }

    #[test]
    fn spawn_pane_closes_the_pane_when_run_fails() {
        let f = Fake::new(&[SPLIT, ("pane run", "", 1)]);
        let env = f.env(&[]);
        assert!(spawn_pane(&env, "w1", Path::new("/w"), &os(&["/c", "launch", "d/c"])).is_err());
        assert_eq!(
            f.calls().last().map(String::as_str),
            Some("pane close w1:p9")
        );
    }

    #[test]
    fn spawn_pane_without_herdr_on_path_is_refused() {
        let env = |k: &str| (k == "PATH").then(|| "/nonexistent".to_string());
        let e = spawn_pane(&env, "w", Path::new("/w"), &os(&["/c"])).unwrap_err();
        assert!(e.to_string().contains("herdr not found"), "{e}");
    }

    #[test]
    fn workspace_finds_or_creates() {
        let list = r#"{"result":{"workspaces":[{"label":"other","workspace_id":"w1"},{"label":"crook-demo","workspace_id":"w7"}]}}"#;
        let f = Fake::new(&[("workspace list", list, 0)]);
        let env = f.env(&[]);
        assert_eq!(workspace(&env, "demo").unwrap(), "w7");
        assert_eq!(f.calls(), ["workspace list"]);

        let empty = r#"{"result":{"workspaces":[]}}"#;
        let made = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
        let f = Fake::new(&[("workspace list", empty, 0), ("workspace create", made, 0)]);
        let env = f.env(&[]);
        assert_eq!(workspace(&env, "demo").unwrap(), "w9");
        assert_eq!(
            f.calls(),
            [
                "workspace list",
                "workspace create --label crook-demo --no-focus"
            ]
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
