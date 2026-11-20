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

pub fn build(o: &Opts) -> Result<Vec<u8>, Error> {
    let bin = o.crook_bin.to_string_lossy();
    let state = format!(
        "//{}/**",
        o.state_dir.to_string_lossy().trim_start_matches('/')
    );
    let mut deny = vec![
        format!("Read({state})"),
        format!("Edit({state})"),
        format!("Write({state})"),
    ];
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
mod tests {
    use super::*;

    fn opts(role: Role) -> Opts {
        Opts {
            role,
            state_dir: "/s/crook".into(),
            crook_bin: "/bin/crook".into(),
        }
    }

    fn built(role: Role) -> Value {
        serde_json::from_slice(&build(&opts(role)).unwrap()).unwrap()
    }

    fn list(v: &Value, k: &str) -> Vec<String> {
        v["permissions"][k]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn settings_core() {
        let v = built(Role::Orchestrator);
        assert_eq!(v["disableAllHooks"], false);
        assert_eq!(v["crossSessionInbound"], "refuse");
        let deny = list(&v, "deny");
        for t in ["Read", "Edit", "Write"] {
            assert!(deny.contains(&format!("{t}(//s/crook/**)")), "{t}");
        }
        for d in &deny {
            assert!(!d.contains("share/crook") && !d.contains("//d/"), "{d}");
        }
        assert!(v["hooks"].get("PostToolUse").is_none());
        let pre = &v["hooks"]["PreToolUse"][0];
        assert_eq!(pre["matcher"], "*");
        assert_eq!(
            pre["hooks"][0],
            json!({"type":"command","command":"/bin/crook","args":["hook","PreToolUse"],"onFailure":"block"})
        );
        assert_eq!(
            v["hooks"]["SessionStart"][0]["hooks"][0]["args"],
            json!(["hook", "SessionStart"])
        );
    }

    #[test]
    fn settings_child() {
        let v = built(Role::Child);
        let allow = list(&v, "allow");
        for t in ["take_task", "need", "done", "status", "worktree"] {
            assert!(allow.contains(&format!("mcp__crook__{t}")), "{t}");
        }
        let deny = list(&v, "deny");
        for t in [
            "CronCreate",
            "ScheduleWakeup",
            "RemoteTrigger",
            "Skill(loop)",
        ] {
            assert!(deny.iter().any(|d| d == t), "{t}");
        }
        assert!(list(&built(Role::Orchestrator), "allow").is_empty());
    }

    #[test]
    fn settings_no_memory_dir() {
        for r in [Role::Orchestrator, Role::Child] {
            assert!(
                !String::from_utf8(build(&opts(r)).unwrap())
                    .unwrap()
                    .contains("autoMemoryDirectory")
            );
        }
    }

    #[test]
    fn permission_mode_mapping() {
        for (i, o) in [
            ("bypassPermissions", "manual"),
            ("plan", "manual"),
            ("acceptEdits", "acceptEdits"),
            ("auto", "auto"),
            ("manual", "manual"),
            ("dontAsk", "dontAsk"),
            ("", "manual"),
            ("weird", "manual"),
        ] {
            assert_eq!(permission_mode(i), o, "{i}");
        }
    }

    #[test]
    fn user_default_mode_reads_settings() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().to_string_lossy().into_owned();
        let env = |k: &str| (k == "CLAUDE_CONFIG_DIR").then(|| dir.clone());
        assert_eq!(user_default_mode(&env), "");
        std::fs::write(
            d.path().join("settings.json"),
            r#"{"permissions":{"defaultMode":"auto"}}"#,
        )
        .unwrap();
        assert_eq!(user_default_mode(&env), "auto");
        std::fs::write(d.path().join("settings.json"), "not json").unwrap();
        assert_eq!(user_default_mode(&env), "");
    }
}
