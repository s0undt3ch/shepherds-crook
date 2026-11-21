//! The embedded mod, unpacked to the data dir at launch.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use include_dir::{Dir, DirEntry, include_dir};

use crate::{Env, Error, paths};

static PLUGIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/plugin");

/// Files the mod does not need at run time. The engine lays its own
/// `.claude-plugin/types` there when it loads the mod, so the copy a developer
/// keeps for `tsc` is never shipped.
fn should_unpack(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    !(name.ends_with(".test.ts")
        || name.ends_with(".spec.ts")
        || name == "tsconfig.json"
        || rel.starts_with(".claude-plugin/types"))
}

fn collect<'a>(dir: &'a Dir<'static>, out: &mut Vec<(&'a Path, &'a [u8])>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, out),
            DirEntry::File(f) if should_unpack(f.path()) => out.push((f.path(), f.contents())),
            DirEntry::File(_) => {}
        }
    }
}

/// The files that get unpacked, in path order so the hash does not depend on
/// the order the embedding macro walked the directory.
fn files() -> Vec<(&'static Path, &'static [u8])> {
    let mut out = Vec::new();
    collect(&PLUGIN, &mut out);
    out.sort_by_key(|(p, _)| *p);
    out
}

/// FNV-1a: it needs no dependency and the digest only has to change when the
/// mod does, not resist an attacker.
fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

/// First 12 hex digits of a hash over the unpacked files' paths and bytes,
/// computed once per process.
pub fn content_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for (path, bytes) in files() {
            fnv1a(&mut h, path.to_string_lossy().as_bytes());
            fnv1a(&mut h, &[0]);
            fnv1a(&mut h, &(bytes.len() as u64).to_le_bytes());
            fnv1a(&mut h, bytes);
        }
        format!("{h:016x}")[..12].to_string()
    })
}

/// Unpacks the embedded mod into `paths::plugin_dir` when it is missing. It
/// writes a temp directory and renames it into place, so a reader never sees
/// a half-written mod and two launches racing is harmless.
pub fn ensure(env: Env) -> Result<PathBuf, Error> {
    let dir = paths::plugin_dir(env);
    if dir.is_dir() {
        return Ok(dir);
    }
    let parent = dir
        .parent()
        .ok_or_else(|| std::io::Error::other("plugin dir has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(".tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = write_all(&tmp) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &dir) {
        let _ = std::fs::remove_dir_all(&tmp);
        // Another process won the race.
        if !dir.is_dir() {
            return Err(e.into());
        }
    }
    Ok(dir)
}

fn write_all(root: &Path) -> Result<(), Error> {
    for (rel, bytes) in files() {
        let target = root.join(rel);
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(target, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
}
