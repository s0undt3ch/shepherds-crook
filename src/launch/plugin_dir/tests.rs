use std::collections::HashMap;
use std::path::Path;

use super::*;

fn env_for(xdg: &Path) -> HashMap<String, String> {
    HashMap::from([("XDG_DATA_HOME".to_string(), xdg.display().to_string())])
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn plugin_dir_unpacks() {
    let tmp = tempfile::tempdir().unwrap();
    let m = env_for(tmp.path());
    let env = move |k: &str| m.get(k).cloned();
    let dir = ensure(&env).unwrap();
    assert_eq!(dir, crate::paths::plugin_dir(&env));
    let name = dir.file_name().unwrap().to_str().unwrap().to_string();
    assert_eq!(
        name,
        format!("{}-{}", env!("CARGO_PKG_VERSION"), content_hash())
    );
    assert_eq!(dir.parent().unwrap(), tmp.path().join("crook-plugin"));
    assert!(dir.join(".claude-plugin/plugin.json").is_file());
    assert!(dir.join("hooks/hooks.json").is_file());
    assert!(dir.join("hooks/core/verb.ts").is_file());
    let skill = std::fs::read_to_string(dir.join("skills/crook-protocol/SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: crook-protocol\n"));

    let mut files = Vec::new();
    walk(&dir, &mut files);
    for f in &files {
        let n = f.file_name().unwrap().to_str().unwrap();
        assert!(
            !n.ends_with(".test.ts") && !n.ends_with(".spec.ts") && n != "tsconfig.json",
            "{f:?} must not be unpacked"
        );
    }

    // A second call changes nothing, not even a marker file added meanwhile.
    std::fs::write(dir.join("marker"), "x").unwrap();
    assert_eq!(ensure(&env).unwrap(), dir);
    assert!(dir.join("marker").is_file());
    let mut after = Vec::new();
    walk(&dir, &mut after);
    assert_eq!(after.len(), files.len() + 1);
    // No temp directory is left beside it.
    let siblings: Vec<_> = std::fs::read_dir(dir.parent().unwrap()).unwrap().collect();
    assert_eq!(siblings.len(), 1);
}

#[test]
fn content_hash_is_12_hex_and_stable() {
    let h = content_hash();
    assert_eq!(h.len(), 12);
    assert!(h.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(h, content_hash());
}

#[test]
fn filter_skips_test_and_tsconfig_files() {
    for skipped in [
        "a.test.ts",
        "core/b.spec.ts",
        "tsconfig.json",
        "x/tsconfig.json",
        ".claude-plugin/types/claude-code/index.d.ts",
    ] {
        assert!(!should_unpack(Path::new(skipped)), "{skipped}");
    }
    for kept in [
        "register.ts",
        "hooks.json",
        "core/verb.ts",
        ".claude-plugin/plugin.json",
    ] {
        assert!(should_unpack(Path::new(kept)), "{kept}");
    }
}
