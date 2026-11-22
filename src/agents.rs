//! `claude agents --json`: the live sessions Claude Code knows about.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{Env, Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub session_id: String,
    pub pid: Option<u32>,
    pub cwd: String,
    pub kind: String,
    pub state: Option<String>,
}

/// Interactive rows carry `status` and background rows carry `state`.
#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
    pid: Option<u32>,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    kind: String,
    state: Option<String>,
    status: Option<String>,
}

/// Fails closed: a row that does not read as expected fails the whole list,
/// so a live session is never taken for a gone one. A row without a session id
/// is kept with an empty one, which matches no recorded session but still
/// shows its `cwd` to the checks that look for sessions inside a worktree.
pub fn parse(json: &str) -> Result<Vec<Agent>, Error> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| Error::Agents(format!("unreadable output: {e}")))?;
    rows.into_iter()
        .enumerate()
        .map(|(i, v)| {
            let r = serde_json::from_value::<Raw>(v)
                .map_err(|e| Error::Agents(format!("unreadable row {i}: {e}")))?;
            Ok(Agent {
                session_id: r.session_id.unwrap_or_default(),
                pid: r.pid,
                cwd: r.cwd,
                kind: r.kind,
                state: r.state.or(r.status),
            })
        })
        .collect()
}

/// Whether `cwd` is `root` or below it, compared as written and then
/// canonicalised, because `/tmp` is `/private/tmp` on macOS.
pub(crate) fn cwd_inside(cwd: &str, root: &str) -> bool {
    if std::path::Path::new(cwd).starts_with(root) {
        return true;
    }
    matches!(
        (std::fs::canonicalize(cwd), std::fs::canonicalize(root)),
        (Ok(c), Ok(w)) if c.starts_with(&w)
    )
}

/// How long `claude agents --json` and `claude --version` may run.
pub const CLAUDE_TIMEOUT: Duration = Duration::from_secs(5);

/// `claude` as the injected `PATH` finds it.
pub(crate) fn find_claude(env: Env) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env("PATH")?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("claude"))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Runs `cmd` with no stdin and returns its stdout, or an error when it fails
/// or runs past `timeout` (then it is killed and reaped). Stdout is read on a
/// thread, so a child with a lot of output never blocks on a full pipe.
pub(crate) fn run_timeout(mut cmd: Command, timeout: Duration) -> Result<String, Error> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Agents(e.to_string()))?;
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| Error::Agents("no stdout".into()))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Agents(format!(
                    "timed out after {} s",
                    timeout.as_secs_f32()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(Error::Agents(e.to_string())),
        }
    };
    let buf = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(Error::Agents(format!("exited with {status}")));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub fn list(env: Env) -> Result<Vec<Agent>, Error> {
    list_within(env, CLAUDE_TIMEOUT)
}

fn list_within(env: Env, timeout: Duration) -> Result<Vec<Agent>, Error> {
    let claude =
        find_claude(env).ok_or_else(|| Error::Agents("claude not found on PATH".into()))?;
    let mut cmd = Command::new(claude);
    cmd.args(["agents", "--json"]);
    parse(&run_timeout(cmd, timeout)?)
}

/// The `claude --version` number, such as `2.1.329`.
pub fn claude_version(env: Env) -> Result<String, Error> {
    let claude =
        find_claude(env).ok_or_else(|| Error::Agents("claude not found on PATH".into()))?;
    let mut cmd = Command::new(claude);
    cmd.arg("--version");
    let out = run_timeout(cmd, CLAUDE_TIMEOUT)?;
    Ok(out
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string())
}

#[cfg(test)]
mod tests;
