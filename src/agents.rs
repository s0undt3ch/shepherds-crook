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

/// A row without a session id cannot be matched to anything, so it is dropped
/// rather than failing the whole list.
pub fn parse(json: &str) -> Result<Vec<Agent>, Error> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| Error::Agents(format!("unreadable output: {e}")))?;
    Ok(rows
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Raw>(v).ok())
        .filter_map(|r| {
            Some(Agent {
                session_id: r.session_id?,
                pid: r.pid,
                cwd: r.cwd,
                kind: r.kind,
                state: r.state.or(r.status),
            })
        })
        .collect())
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
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/agents.json");

    #[test]
    fn agents_both_keys() {
        let a = parse(FIXTURE).unwrap();
        let by = |sid: &str| a.iter().find(|x| x.session_id == sid).unwrap();
        // Interactive rows carry `status`.
        assert_eq!(by("aaaa-interactive").state.as_deref(), Some("idle"));
        assert_eq!(by("aaaa-interactive").pid, Some(4242));
        assert_eq!(by("aaaa-interactive").kind, "interactive");
        // Background rows carry `state` and no pid.
        assert_eq!(by("bbbb-background").state.as_deref(), Some("blocked"));
        assert_eq!(by("bbbb-background").pid, None);
        assert_eq!(by("bbbb-background").cwd, "/work/repo/.worktrees/c2");
        assert_eq!(a.len(), 4);
    }

    #[test]
    fn state_wins_over_status_and_neither_is_none() {
        let a = parse(
            r#"[{"sessionId":"s1","cwd":"/a","kind":"k","state":"waiting","status":"idle"},
                {"sessionId":"s2","cwd":"/a","kind":"k"}]"#,
        )
        .unwrap();
        assert_eq!(a[0].state.as_deref(), Some("waiting"));
        assert_eq!(a[1].state, None);
    }

    #[test]
    fn rows_without_a_session_id_are_skipped() {
        let a = parse(r#"[{"cwd":"/a"},{"sessionId":"s","cwd":"/b"}]"#).unwrap();
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn not_an_array_is_an_error() {
        assert!(matches!(parse("{}"), Err(Error::Agents(_))));
        assert!(matches!(parse("nope"), Err(Error::Agents(_))));
    }

    #[cfg(unix)]
    #[test]
    fn list_runs_claude_from_path_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("claude");
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agents.json");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\n[ \"$1 $2\" = \"agents --json\" ] || exit 3\nexec /bin/cat {fixture}\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = tmp.path().display().to_string();
        let env = move |k: &str| (k == "PATH").then(|| p.clone());
        assert_eq!(list(&env).unwrap().len(), 4);

        let empty = tempfile::tempdir().unwrap();
        let p = empty.path().display().to_string();
        let env = move |k: &str| (k == "PATH").then(|| p.clone());
        assert!(matches!(list(&env), Err(Error::Agents(_))));
    }

    #[cfg(unix)]
    #[test]
    fn list_gives_up_on_a_hung_claude() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = tmp.path().display().to_string();
        let env = move |k: &str| (k == "PATH").then(|| p.clone());
        let t = Instant::now();
        let e = list_within(&env, Duration::from_millis(300)).unwrap_err();
        assert!(e.to_string().contains("timed out"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    #[test]
    fn large_output_does_not_block_the_child() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("claude");
        // ~200 KB, past the 64 KB pipe buffer.
        std::fs::write(
            &bin,
            "#!/bin/sh\nprintf '['\ni=0; while [ $i -lt 4000 ]; do printf '{\"sessionId\":\"s%s\",\"cwd\":\"/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"},' $i; i=$((i+1)); done\nprintf '{\"sessionId\":\"last\"}]'\n",
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = tmp.path().display().to_string();
        let env = move |k: &str| (k == "PATH").then(|| p.clone());
        assert_eq!(list(&env).unwrap().len(), 4001);
    }

    #[cfg(unix)]
    #[test]
    fn claude_version_reads_the_first_word() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("claude");
        std::fs::write(&bin, "#!/bin/sh\necho '2.1.329 (Claude Code)'\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = tmp.path().display().to_string();
        let env = move |k: &str| (k == "PATH").then(|| p.clone());
        assert_eq!(claude_version(&env).unwrap(), "2.1.329");
    }
}
