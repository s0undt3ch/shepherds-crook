use super::*;

fn env_in(cfg: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    let c = cfg.display().to_string();
    move |k: &str| (k == "CLAUDE_CONFIG_DIR").then(|| c.clone())
}

fn run(cfg: &Path, v: &str, managed: &[PathBuf]) -> Result<Vec<String>, Error> {
    check_with(&env_in(cfg), v, managed)
}

#[test]
fn preflight_version() {
    let t = tempfile::tempdir().unwrap();
    let e = run(t.path(), "2.1.317", &[]).unwrap_err();
    assert!(e.to_string().contains("2.1.318"), "{e}");
    assert!(run(t.path(), "2.1.318", &[]).unwrap().is_empty());
    assert!(run(t.path(), "2.1.329", &[]).unwrap().is_empty());
    let w = run(t.path(), "2.1.340", &[]).unwrap();
    assert!(
        w.iter()
            .any(|l| l.contains("2.1.340") && l.contains("2.1.329")),
        "{w:?}"
    );
    assert!(
        run(t.path(), "2.2.0", &[])
            .unwrap()
            .iter()
            .any(|l| l.contains("2.2.0"))
    );
    assert!(run(t.path(), "1.9.999", &[]).is_err());
    for bad in ["", "banana", "2.1", "2.1.x"] {
        let e = run(t.path(), bad, &[]).unwrap_err();
        assert!(e.to_string().contains("version"), "{bad}: {e}");
    }
}

fn managed(dir: &Path, file: &str, json: &str) -> Vec<PathBuf> {
    let p = dir.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, json).unwrap();
    vec![dir.to_path_buf()]
}

#[test]
fn preflight_managed() {
    let cfg = tempfile::tempdir().unwrap();
    for (file, json, key) in [
        (
            "managed-settings.json",
            r#"{"allowManagedHooksOnly": true}"#,
            "allowManagedHooksOnly",
        ),
        (
            "managed-settings.d/10-mods.json",
            r#"{"pluginConfigs":{"cc-plugin-sec-default@builtin":{"options":{"allowManagedModsOnly":true}}}}"#,
            "allowManagedModsOnly",
        ),
        (
            "managed-settings.json",
            r#"{"allowManagedModsOnly": true}"#,
            "allowManagedModsOnly",
        ),
    ] {
        let d = tempfile::tempdir().unwrap();
        let m = managed(d.path(), file, json);
        let e = run(cfg.path(), "2.1.329", &m).unwrap_err();
        assert!(e.to_string().contains(key), "{file}: {e}");
    }
    for json in [
        r#"{"allowManagedHooksOnly": false}"#,
        r#"{"model": "haiku"}"#,
    ] {
        let d = tempfile::tempdir().unwrap();
        let m = managed(d.path(), "managed-settings.json", json);
        assert!(run(cfg.path(), "2.1.329", &m).unwrap().is_empty(), "{json}");
    }
    // A directory with no managed files is fine.
    let d = tempfile::tempdir().unwrap();
    assert!(
        run(cfg.path(), "2.1.329", &[d.path().to_path_buf()])
            .unwrap()
            .is_empty()
    );
}

fn install(cfg: &Path, name: &str, hooks_json: &str, module: &str, src: &str) {
    let root = cfg.join("plugins/cache").join(name);
    std::fs::create_dir_all(root.join("hooks")).unwrap();
    std::fs::write(root.join("hooks/hooks.json"), hooks_json).unwrap();
    std::fs::write(root.join("hooks").join(module), src).unwrap();
    let file = cfg.join("plugins/installed_plugins.json");
    let mut v: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({"version": 2, "plugins": {}}));
    v["plugins"][format!("{name}@m")] =
        serde_json::json!([{"scope": "user", "installPath": root.display().to_string()}]);
    std::fs::write(file, v.to_string()).unwrap();
}

#[test]
fn preflight_mods() {
    let cfg = tempfile::tempdir().unwrap();
    let reg = r#"{"modules": ["./m.ts"]}"#;
    install(
        cfg.path(),
        "approver",
        reg,
        "m.ts",
        "on(\"tool.check\", async () => {})",
    );
    install(cfg.path(), "typer", reg, "m.ts", "on('prompt.submit', f)");
    install(cfg.path(), "spaced", reg, "m.ts", "on( 'tool.check' , f)");
    install(cfg.path(), "quiet", reg, "m.ts", "on('session.start', f)");
    install(cfg.path(), "nomods", "{}", "m.ts", "on('tool.check', f)");
    let w = run(cfg.path(), "2.1.329", &[]).unwrap();
    let joined = w.join("\n");
    for named in ["approver", "typer", "spaced"] {
        assert!(joined.contains(named), "{named}: {joined}");
    }
    assert!(
        !joined.contains("quiet") && !joined.contains("nomods"),
        "{joined}"
    );
}

#[test]
fn preflight_without_plugins_is_quiet() {
    let cfg = tempfile::tempdir().unwrap();
    assert!(run(cfg.path(), "2.1.329", &[]).unwrap().is_empty());
    std::fs::create_dir_all(cfg.path().join("plugins")).unwrap();
    std::fs::write(
        cfg.path().join("plugins/installed_plugins.json"),
        "not json",
    )
    .unwrap();
    assert!(run(cfg.path(), "2.1.329", &[]).unwrap().is_empty());
}
