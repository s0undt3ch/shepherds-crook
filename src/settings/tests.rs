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
