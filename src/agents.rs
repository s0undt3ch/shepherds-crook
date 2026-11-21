//! `claude agents --json`: the live sessions Claude Code knows about.

use std::path::PathBuf;
use std::process::{Command, Stdio};

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

/// `claude` as the injected `PATH` finds it.
fn find_claude(env: Env) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env("PATH")?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join("claude"))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

pub fn list(env: Env) -> Result<Vec<Agent>, Error> {
    let claude =
        find_claude(env).ok_or_else(|| Error::Agents("claude not found on PATH".into()))?;
    let out = Command::new(claude)
        .args(["agents", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Agents(e.to_string()))?;
    if !out.status.success() {
        return Err(Error::Agents(format!("exited with {}", out.status)));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
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
}
