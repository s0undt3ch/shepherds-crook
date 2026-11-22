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

/// Closes the tab around `pane`, or just the pane when the tab holds others, but
/// only inside a `crook-*` workspace, where crook opens its panes (a label can be
/// missed when a rename fails or a tab falls back to a split). Any other pane,
/// such as the one `crook resume` ran in, stays open. Returns whether it closed
/// something; a pane already gone counts.
pub fn close_tab_of(env: Env, pane: &str) -> Result<bool, Error> {
    let info = match json(env, ["pane", "get", pane]) {
        Ok(v) => v,
        Err(e) if e.to_string().contains("pane_not_found") => return Ok(true),
        Err(e) => return Err(e),
    };
    let tab = str_at(&info, "/result/pane/tab_id")?.to_string();
    let ws = str_at(&info, "/result/pane/workspace_id")?.to_string();
    let tab_info = match json(env, ["tab", "get", tab.as_str()]) {
        Ok(v) => v,
        Err(e) if e.to_string().contains("tab_not_found") => return Ok(true),
        Err(e) => return Err(e),
    };
    let ws_info = match json(env, ["workspace", "get", ws.as_str()]) {
        Ok(v) => v,
        Err(e) if e.to_string().contains("workspace_not_found") => return Ok(true),
        Err(e) => return Err(e),
    };
    let ours = ws_info
        .pointer("/result/workspace/label")
        .and_then(Value::as_str)
        .is_some_and(|l| l.starts_with("crook-"));
    if !ours {
        return Ok(false);
    }
    let alone = tab_info
        .pointer("/result/tab/pane_count")
        .and_then(Value::as_u64)
        == Some(1);
    if alone {
        close_tab(env, &tab)?;
    } else {
        close(env, pane)?;
    }
    Ok(true)
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
mod tests;
