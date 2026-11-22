//! The `--settings` JSON every crook session is launched with.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::gate::Role;
use crate::{Env, Error, paths};

pub struct Opts {
    pub role: Role,
    pub state_dir: PathBuf,
    pub crook_bin: PathBuf,
}

/// Modes that may start a crook session; anything else, including the
/// permissive `bypassPermissions` and `plan`, becomes `manual`.
pub fn permission_mode(user_default: &str) -> &'static str {
    match user_default {
        "acceptEdits" => "acceptEdits",
        "auto" => "auto",
        "dontAsk" => "dontAsk",
        _ => "manual",
    }
}

/// `permissions.defaultMode` from the user's Claude Code settings, or empty.
pub fn user_default_mode(env: Env) -> String {
    let path = paths::claude_config_dir(env).join("settings.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["permissions"]["defaultMode"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn hook(bin: &str, event: &str) -> Value {
    json!({
        "type": "command",
        "command": bin,
        "args": ["hook", event],
        "onFailure": "block",
    })
}

/// Claude Code matches rules against resolved paths: `/var` on macOS is a symlink to
/// `/private/var`, and `$TMPDIR` ends in `/`, so the raw path can match nothing.
fn rule_path(p: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.components().collect())
}

pub fn build(o: &Opts) -> Result<Vec<u8>, Error> {
    let bin = o.crook_bin.to_string_lossy();
    let state = format!(
        "//{}/**",
        rule_path(&o.state_dir)
            .to_string_lossy()
            .trim_start_matches('/')
    );
    let mut deny = vec![format!("Read({state})"), format!("Edit({state})")];
    let mut allow: Vec<&str> = Vec::new();
    if o.role == Role::Child {
        allow.extend([
            "mcp__crook__take_task",
            "mcp__crook__need",
            "mcp__crook__done",
            "mcp__crook__status",
            "mcp__crook__worktree",
        ]);
        deny.extend(
            [
                "CronCreate",
                "ScheduleWakeup",
                "RemoteTrigger",
                "Skill(loop)",
                "Skill(loop *)",
            ]
            .map(str::to_owned),
        );
    }
    let v = json!({
        "disableAllHooks": false,
        "crossSessionInbound": "refuse",
        "permissions": { "allow": allow, "deny": deny },
        "hooks": {
            "PreToolUse": [{ "matcher": "*", "hooks": [hook(&bin, "PreToolUse")] }],
            "SessionStart": [{ "hooks": [hook(&bin, "SessionStart")] }],
        },
    });
    serde_json::to_vec_pretty(&v).map_err(|e| Error::Refused(e.to_string()))
}

#[cfg(test)]
mod tests;
