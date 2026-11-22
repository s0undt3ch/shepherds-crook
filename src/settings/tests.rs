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
    for t in ["Read", "Edit"] {
        assert!(deny.contains(&format!("{t}(//s/crook/**)")), "{t}");
    }
    assert!(
        !deny.iter().any(|d| d.starts_with("Write(")),
        "Write rules never match"
    );
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

#[test]
fn settings_state_rule_is_resolved() {
    let d = tempfile::tempdir().unwrap();
    let real = std::fs::canonicalize(d.path()).unwrap();
    std::fs::create_dir(real.join("crook")).unwrap();
    std::os::unix::fs::symlink(&real, real.join("link")).unwrap();
    let o = Opts {
        role: Role::Orchestrator,
        state_dir: real.join("link//crook"),
        crook_bin: "/bin/crook".into(),
    };
    let v: Value = serde_json::from_slice(&build(&o).unwrap()).unwrap();
    let want = format!("Read(/{}/**)", real.join("crook").display());
    assert!(list(&v, "deny").contains(&want), "{:?}", list(&v, "deny"));

    let missing = Opts {
        state_dir: "/no//such/crook".into(),
        ..o
    };
    let v: Value = serde_json::from_slice(&build(&missing).unwrap()).unwrap();
    assert!(list(&v, "deny").contains(&"Read(//no/such/crook/**)".to_owned()));
}
