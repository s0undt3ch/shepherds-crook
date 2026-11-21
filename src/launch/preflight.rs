//! Checks `crook open` makes before it starts `claude`.

use std::path::{Path, PathBuf};

use crate::{Env, Error, paths};

/// The lowest Claude Code `crook open` starts, and the newest it was tested on.
const FLOOR: (u32, u32, u32) = (2, 1, 318);
const TESTED: (u32, u32, u32) = (2, 1, 329);

const MOD_EVENTS: [&str; 2] = ["tool.check", "prompt.submit"];

/// Where the OS-level managed settings files live.
const MANAGED_DIRS: [&str; 2] = [
    "/Library/Application Support/ClaudeCode",
    "/etc/claude-code",
];

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.trim().split('.');
    let n = (
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
        it.next()?.parse().ok()?,
    );
    it.next().is_none().then_some(n)
}

fn dotted((a, b, c): (u32, u32, u32)) -> String {
    format!("{a}.{b}.{c}")
}

/// The OS-level managed-settings directories.
pub fn default_dirs() -> Vec<PathBuf> {
    MANAGED_DIRS.iter().map(PathBuf::from).collect()
}

/// Warnings, or a refusal: too old a Claude Code, or a policy that switches
/// the mod or its hooks off. Looks at the real managed-settings locations.
pub fn check(env: Env, claude_version: &str) -> Result<Vec<String>, Error> {
    check_with(env, claude_version, &default_dirs())
}

/// `check` with the managed-settings directories given. MDM profiles and
/// server-managed settings are not files, so they are not seen here.
pub fn check_with(
    env: Env,
    claude_version: &str,
    managed_dirs: &[PathBuf],
) -> Result<Vec<String>, Error> {
    let mut warnings = Vec::new();
    let v = parse_version(claude_version).ok_or_else(|| {
        Error::Refused(format!(
            "cannot read the Claude Code version from {claude_version:?}"
        ))
    })?;
    if v < FLOOR {
        return Err(Error::Refused(format!(
            "Claude Code {} is older than {}, the oldest crook supports",
            dotted(v),
            dotted(FLOOR)
        )));
    }
    if v > TESTED {
        warnings.push(format!(
            "Claude Code {} is newer than {}, the version crook was tested on",
            dotted(v),
            dotted(TESTED)
        ));
    }
    for dir in managed_dirs {
        for (file, doc) in managed_docs(dir) {
            if let Some(key) = switches_off(&doc) {
                return Err(Error::Refused(format!(
                    "{key} is set in {}: crook's mod and hook cannot run under it",
                    file.display()
                )));
            }
        }
    }
    warnings.extend(approving_mods(env));
    Ok(warnings)
}

/// `managed-settings.json` and `managed-settings.d/*.json` under `dir`, parsed.
/// A file that cannot be read or parsed is skipped: Claude Code reports it itself.
fn managed_docs(dir: &Path) -> Vec<(PathBuf, serde_json::Value)> {
    let mut files = vec![dir.join("managed-settings.json")];
    if let Ok(rd) = std::fs::read_dir(dir.join("managed-settings.d")) {
        let mut drop_ins: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        drop_ins.sort();
        files.extend(drop_ins);
    }
    files
        .into_iter()
        .filter_map(|f| {
            let doc = serde_json::from_str(&std::fs::read_to_string(&f).ok()?).ok()?;
            Some((f, doc))
        })
        .collect()
}

/// `allowManagedHooksOnly` is a top-level key; `allowManagedModsOnly` is an
/// option of the built-in guard under `pluginConfigs` (a top-level spelling is
/// refused too, as the safer reading).
fn switches_off(doc: &serde_json::Value) -> Option<&'static str> {
    if doc["allowManagedHooksOnly"] == true {
        return Some("allowManagedHooksOnly");
    }
    let guard = &doc["pluginConfigs"]["cc-plugin-sec-default@builtin"]["options"];
    if guard["allowManagedModsOnly"] == true || doc["allowManagedModsOnly"] == true {
        return Some("allowManagedModsOnly");
    }
    None
}

/// Whitespace removed, so `on( 'tool.check' ,` and `on("tool.check",` both match.
fn hooks_an_event(src: &str) -> bool {
    let squeezed: String = src.chars().filter(|c| !c.is_whitespace()).collect();
    MOD_EVENTS.iter().any(|e| {
        squeezed.contains(&format!("on(\"{e}\"")) || squeezed.contains(&format!("on('{e}'"))
    })
}

/// A warning per installed plugin whose mod hooks `tool.check` or
/// `prompt.submit`: without managed settings such a mod can approve a call a
/// `PreToolUse` hook asked about. Plugins come from `installed_plugins.json`;
/// the cache folder also holds old versions, which would give false warnings.
fn approving_mods(env: Env) -> Vec<String> {
    let root = paths::claude_config_dir(env);
    let Some(installed) = std::fs::read_to_string(root.join("plugins/installed_plugins.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return Vec::new();
    };
    let Some(plugins) = installed["plugins"].as_object() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, installs) in plugins {
        let hit = installs
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i["installPath"].as_str())
            .any(|path| mod_hooks_event(Path::new(path)));
        if hit {
            out.push(format!(
                "installed plugin {name} has a mod that hooks tool.check or prompt.submit: \
                 it can approve a call that crook's hook asked about"
            ));
        }
    }
    out.sort();
    out
}

fn mod_hooks_event(install: &Path) -> bool {
    let hooks = install.join("hooks");
    let Some(doc) = std::fs::read_to_string(hooks.join("hooks.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
    else {
        return false;
    };
    doc["modules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m.as_str())
        .any(|m| std::fs::read_to_string(hooks.join(m)).is_ok_and(|src| hooks_an_event(&src)))
}

#[cfg(test)]
mod tests {
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
}
