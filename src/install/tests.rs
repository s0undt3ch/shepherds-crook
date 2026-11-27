use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use super::*;
use crate::testutil::env_of;

fn manifest() -> PathBuf {
    PathBuf::from("/repo")
}

/// A folder shaped like an unpacked release: `plugin/.claude-plugin/plugin.json` with `version`.
fn install_at(root: &Path, version: Option<&str>) {
    let dir = root.join("plugin/.claude-plugin");
    std::fs::create_dir_all(&dir).expect("mkdir");
    if let Some(v) = version {
        std::fs::write(dir.join("plugin.json"), format!(r#"{{"version": "{v}"}}"#)).expect("write");
    }
}

fn canon(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().canonicalize().expect("canonical")
}

#[test]
fn the_install_folder_is_the_one_holding_plugin() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = canon(&dir);
    install_at(&root, Some("0.2.0"));
    let found = locate(&root.join("crook"), false, &manifest()).expect("found");
    assert_eq!(found.root(), root);
    assert_eq!(found.plugin_dir(), root.join("plugin"));
    assert_eq!(found.mod_dir(), root.join("mod"));
}

#[test]
fn a_debug_build_without_plugin_falls_back_to_the_repository() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = canon(&dir);
    let found = locate(&root.join("crook"), true, &manifest()).expect("found");
    assert_eq!(found.root(), manifest());
    assert_eq!(found.plugin_dir(), manifest().join("plugin"));
}

#[test]
fn a_debug_build_prefers_a_plugin_next_to_the_executable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = canon(&dir);
    install_at(&root, Some("0.2.0"));
    let found = locate(&root.join("crook"), true, &manifest()).expect("found");
    assert_eq!(found.root(), root);
}

#[test]
fn a_release_build_without_plugin_says_the_install_is_broken() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = canon(&dir);
    let msg = locate(&root.join("crook"), false, &manifest())
        .expect_err("refused")
        .to_string();
    assert!(msg.contains("install is broken"), "{msg}");
    assert!(msg.contains(&root.display().to_string()), "{msg}");
}

#[test]
fn a_file_called_plugin_is_not_a_plugin_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = canon(&dir);
    std::fs::write(root.join("plugin"), "").expect("write");
    assert!(locate(&root.join("crook"), false, &manifest()).is_err());
}

#[test]
fn find_resolves_a_linked_executable_to_the_real_install() {
    let dir = tempfile::tempdir().expect("tempdir");
    let real = canon(&dir).join("installs/0.2.0");
    std::fs::create_dir_all(&real).expect("mkdir");
    install_at(&real, Some("0.2.0"));
    std::fs::write(real.join("crook"), "").expect("write");
    let shims = canon(&dir).join("shims");
    std::fs::create_dir_all(&shims).expect("mkdir");
    std::os::unix::fs::symlink(real.join("crook"), shims.join("crook")).expect("symlink");
    let exe = shims.join("crook");
    let found = find(&move || Ok(exe.clone()), false, &manifest()).expect("found");
    assert_eq!(found.root(), real);
}

#[test]
fn find_reports_an_executable_it_cannot_locate() {
    let err =
        find(&|| Err(std::io::Error::other("no exe")), false, &manifest()).expect_err("fails");
    assert!(err.to_string().contains("no exe"), "{err}");
}

#[test]
fn versions_compare_as_three_numbers() {
    assert_eq!(compare_versions("0.2.0", "0.10.0"), Some(Ordering::Less));
    assert_eq!(
        compare_versions("1.0.0", "0.99.99"),
        Some(Ordering::Greater)
    );
    assert_eq!(compare_versions("0.2.0", "0.2.0"), Some(Ordering::Equal));
}

#[test]
fn semver_precedence_orders_releases_and_pre_releases() {
    let ascending = [
        "0.0.0-dev",
        "0.1.0-alpha",
        "0.1.0-rc.1",
        "0.1.0-rc.2",
        "0.1.0-rc.10",
        "0.1.0-rc.10.1",
        "0.1.0-rcx",
        "0.1.0",
        "0.1.1-rc.1",
        "0.1.1",
        "0.10.0",
        "1.0.0",
    ];
    for (i, lo) in ascending.iter().enumerate() {
        for hi in &ascending[i + 1..] {
            assert_eq!(
                compare_versions(lo, hi),
                Some(Ordering::Less),
                "{lo} < {hi}"
            );
            assert_eq!(
                compare_versions(hi, lo),
                Some(Ordering::Greater),
                "{hi} > {lo}"
            );
        }
        assert_eq!(compare_versions(lo, lo), Some(Ordering::Equal), "{lo}");
    }
}

#[test]
fn numeric_identifiers_rank_below_words_and_compare_by_value() {
    assert_eq!(compare_versions("1.0.0-1", "1.0.0-a"), Some(Ordering::Less));
    assert_eq!(
        compare_versions("1.0.0-rc.9", "1.0.0-rc.10"),
        Some(Ordering::Less)
    );
    assert_eq!(
        compare_versions("1.0.0-rc", "1.0.0-rc.1"),
        Some(Ordering::Less)
    );
}

#[test]
fn build_metadata_is_ignored() {
    assert_eq!(
        compare_versions("1.0.0+a", "1.0.0+b"),
        Some(Ordering::Equal)
    );
    assert_eq!(
        compare_versions("1.0.0-rc.1+x", "1.0.0-rc.1"),
        Some(Ordering::Equal)
    );
    assert_eq!(
        compare_versions("1.0.1+x", "1.0.0"),
        Some(Ordering::Greater)
    );
}

#[test]
fn anything_that_is_not_semver_is_unknown() {
    for bad in [
        "",
        "1.2",
        "1.2.3.4",
        "v1.2.3",
        "a.b.c",
        "1..3",
        "1.2.3-",
        "1.2.3-rc..1",
        "1.2.3-01",
        "1.2.3+",
        "1.2.3-rc_1",
        "banana",
        "99999999999999999999999.0.0",
    ] {
        assert_eq!(compare_versions(bad, "1.0.0"), None, "{bad:?}");
        assert_eq!(compare_versions("1.0.0", bad), None, "{bad:?}");
    }
}

#[test]
fn the_plugin_version_is_read_from_plugin_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    install_at(dir.path(), Some("0.3.1"));
    assert_eq!(plugin_version(dir.path()).expect("version"), "0.3.1");
}

#[test]
fn a_missing_or_malformed_plugin_json_is_an_error_naming_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    install_at(dir.path(), None);
    let missing = plugin_version(dir.path()).expect_err("missing").to_string();
    assert!(missing.contains("plugin.json"), "{missing}");
    let file = dir.path().join("plugin/.claude-plugin/plugin.json");
    for body in ["not json", "{}", r#"{"version": 3}"#] {
        std::fs::write(&file, body).expect("write");
        let msg = plugin_version(dir.path()).expect_err("bad").to_string();
        assert!(msg.contains("plugin.json"), "{body}: {msg}");
    }
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = canon(&dir);
        World { _dir: dir, root }
    }

    fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        let data = self.root.join("data").display().to_string();
        let home = self.root.join("home").display().to_string();
        env_of(&[("XDG_DATA_HOME", &data), ("HOME", &home)])
    }

    fn link(&self) -> PathBuf {
        self.root.join("data/crook/current")
    }

    fn install(&self, name: &str, version: Option<&str>) -> Install {
        let root = self.root.join("installs").join(name);
        install_at(&root, version);
        Install::at(root)
    }

    /// The install's executable, which has to exist for `find` to resolve it.
    fn exe(&self, inst: &Install) -> PathBuf {
        let exe = inst.root().join("crook");
        std::fs::write(&exe, "").expect("write");
        exe
    }

    fn target(&self) -> PathBuf {
        std::fs::read_link(self.link()).expect("a link")
    }
}

#[test]
fn a_missing_link_is_created_and_its_parent_made() {
    let w = World::new();
    let inst = w.install("a", Some("0.2.0"));
    assert_eq!(
        point_current(&w.env(), &inst, false).expect("ok"),
        LinkOutcome::Created
    );
    assert_eq!(w.target(), inst.root());
    assert!(w.link().join("plugin/.claude-plugin/plugin.json").exists());
}

#[test]
fn a_dangling_link_moves() {
    let w = World::new();
    std::fs::create_dir_all(w.link().parent().expect("parent")).expect("mkdir");
    std::os::unix::fs::symlink(w.root.join("installs/gone"), w.link()).expect("symlink");
    let inst = w.install("a", Some("0.2.0"));
    assert_eq!(
        point_current(&w.env(), &inst, false).expect("ok"),
        LinkOutcome::Moved
    );
    assert_eq!(w.target(), inst.root());
}

#[test]
fn an_older_target_moves() {
    let w = World::new();
    let old = w.install("old", Some("0.2.0"));
    point_current(&w.env(), &old, false).expect("ok");
    let new = w.install("new", Some("0.10.0"));
    assert_eq!(
        point_current(&w.env(), &new, false).expect("ok"),
        LinkOutcome::Moved
    );
    assert_eq!(w.target(), new.root());
}

#[test]
fn a_newer_equal_or_unknown_target_stays() {
    let w = World::new();
    let first = w.install("first", Some("0.5.0"));
    point_current(&w.env(), &first, false).expect("ok");
    for (name, version) in [
        ("lower", Some("0.4.0")),
        ("equal", Some("0.5.0")),
        ("equal with build", Some("0.5.0+build.7")),
        ("pre-release of it", Some("0.5.0-rc.1")),
        ("dev", Some("0.0.0-dev")),
        ("junk", Some("banana")),
        ("none", None),
    ] {
        let other = w.install(name, version);
        assert_eq!(
            point_current(&w.env(), &other, false).expect("ok"),
            LinkOutcome::Unchanged,
            "{name}"
        );
        assert_eq!(w.target(), first.root(), "{name}");
    }
}

#[test]
fn a_target_of_unknown_version_stays_even_for_a_newer_install() {
    let w = World::new();
    let first = w.install("first", Some("banana"));
    point_current(&w.env(), &first, false).expect("ok");
    let newer = w.install("newer", Some("9.0.0"));
    assert_eq!(
        point_current(&w.env(), &newer, false).expect("ok"),
        LinkOutcome::Unchanged
    );
    assert_eq!(w.target(), first.root());
}

#[test]
fn a_release_candidate_moves_forward_to_the_next_candidate_and_the_release() {
    let w = World::new();
    let rc1 = w.install("rc1", Some("0.1.0-rc.1"));
    point_current(&w.env(), &rc1, false).expect("ok");
    for (name, version) in [
        ("rc2", "0.1.0-rc.2"),
        ("rc10", "0.1.0-rc.10"),
        ("rel", "0.1.0"),
    ] {
        let next = w.install(name, Some(version));
        assert_eq!(
            point_current(&w.env(), &next, false).expect("ok"),
            LinkOutcome::Moved,
            "{name}"
        );
        assert_eq!(w.target(), next.root(), "{name}");
    }
}

#[test]
fn a_dev_target_is_replaced_by_any_release() {
    let w = World::new();
    let dev = w.install("dev", Some("0.0.0-dev"));
    point_current(&w.env(), &dev, false).expect("ok");
    let rc = w.install("rc", Some("0.1.0-rc.1"));
    assert_eq!(
        point_current(&w.env(), &rc, false).expect("ok"),
        LinkOutcome::Moved
    );
}

#[test]
fn pointing_at_the_install_it_already_names_changes_nothing() {
    let w = World::new();
    let inst = w.install("a", Some("0.2.0"));
    point_current(&w.env(), &inst, false).expect("ok");
    assert_eq!(
        point_current(&w.env(), &inst, false).expect("ok"),
        LinkOutcome::Unchanged
    );
}

#[test]
fn a_relative_link_target_resolves_against_the_link_folder() {
    let w = World::new();
    let old = w.install("old", Some("0.2.0"));
    std::fs::create_dir_all(w.link().parent().expect("parent")).expect("mkdir");
    let link = w.link();
    let rel = pathdiff(&old.root, link.parent().expect("parent"));
    std::os::unix::fs::symlink(rel, w.link()).expect("symlink");
    let new = w.install("new", Some("0.3.0"));
    assert_eq!(
        point_current(&w.env(), &new, false).expect("ok"),
        LinkOutcome::Moved
    );
}

/// `to` relative to `from`, for two absolute paths with a common prefix.
fn pathdiff(to: &Path, from: &Path) -> PathBuf {
    let to: Vec<_> = to.components().collect();
    let from: Vec<_> = from.components().collect();
    let common = to.iter().zip(&from).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    for c in &to[common..] {
        out.push(c);
    }
    out
}

#[test]
fn a_regular_file_at_the_link_path_is_left_alone() {
    let w = World::new();
    std::fs::create_dir_all(w.link().parent().expect("parent")).expect("mkdir");
    std::fs::write(w.link(), "mine").expect("write");
    let inst = w.install("a", Some("0.2.0"));
    let out = point_current(&w.env(), &inst, false).expect("ok");
    assert!(matches!(out, LinkOutcome::LeftAlone(_)), "{out:?}");
    assert_eq!(std::fs::read_to_string(w.link()).expect("read"), "mine");
}

#[test]
fn a_folder_at_the_link_path_is_left_alone() {
    let w = World::new();
    std::fs::create_dir_all(w.link()).expect("mkdir");
    let inst = w.install("a", Some("0.2.0"));
    let out = point_current(&w.env(), &inst, false).expect("ok");
    assert!(matches!(out, LinkOutcome::LeftAlone(_)), "{out:?}");
    assert!(w.link().is_dir());
    assert!(!w.link().is_symlink());
}

#[test]
fn a_debug_build_never_touches_the_link() {
    let w = World::new();
    let inst = w.install("a", Some("0.2.0"));
    assert_eq!(
        point_current(&w.env(), &inst, true).expect("ok"),
        LinkOutcome::DebugBuild
    );
    assert!(!w.link().exists() && !w.link().is_symlink());
    assert!(!w.link().parent().expect("parent").exists());
}

#[test]
fn the_swap_leaves_no_temporary_link_behind() {
    let w = World::new();
    let old = w.install("old", Some("0.2.0"));
    point_current(&w.env(), &old, false).expect("ok");
    let new = w.install("new", Some("0.3.0"));
    point_current(&w.env(), &new, false).expect("ok");
    let mut names: Vec<_> = std::fs::read_dir(w.link().parent().expect("parent"))
        .expect("read_dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["current", "current.lock"]);
}

#[test]
fn racing_installs_end_on_the_highest_version() {
    let w = World::new();
    let installs: Vec<Install> = (0..8)
        .map(|n| w.install(&format!("v{n}"), Some(&format!("0.{n}.0"))))
        .collect();
    std::thread::scope(|scope| {
        for inst in installs.iter().rev() {
            let env = w.env();
            scope.spawn(move || {
                point_current(&env, inst, false).expect("ok");
            });
        }
    });
    assert_eq!(w.target(), installs[7].root());
}

#[test]
fn a_missing_home_is_an_error_not_a_panic() {
    let w = World::new();
    let inst = w.install("a", Some("0.2.0"));
    assert!(point_current(&env_of(&[]), &inst, false).is_err());
}

#[test]
fn refresh_warns_instead_of_failing() {
    let w = World::new();
    std::fs::create_dir_all(w.link().parent().expect("parent")).expect("mkdir");
    std::fs::write(w.link(), "mine").expect("write");
    let inst = w.install("a", Some("0.2.0"));
    let exe = w.exe(&inst);
    let env = w.env();
    let warnings = refresh_with(&env, &move || Ok(exe.clone()), false, &manifest());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("warning:"), "{warnings:?}");

    let warnings = refresh_with(
        &env,
        &|| Err(std::io::Error::other("boom")),
        false,
        &manifest(),
    );
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("boom"));
}

#[test]
fn refresh_in_a_debug_build_does_nothing() {
    let w = World::new();
    let env = w.env();
    let warnings = refresh_with(
        &env,
        &|| Err(std::io::Error::other("boom")),
        true,
        &manifest(),
    );
    assert!(warnings.is_empty());
    assert!(!w.link().parent().expect("parent").exists());
}

#[test]
fn refresh_moves_the_link_in_a_release_build() {
    let w = World::new();
    let inst = w.install("a", Some("0.2.0"));
    let exe = w.exe(&inst);
    let env = w.env();
    let warnings = refresh_with(&env, &move || Ok(exe.clone()), false, &manifest());
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(w.target(), inst.root());
}

#[test]
fn crook_bin_is_the_variable_when_set() {
    let env = env_of(&[("CROOK_BIN", "/opt/crook")]);
    let exe = || Ok(PathBuf::from("/self/crook"));
    assert_eq!(
        crook_bin(&env, &exe, false).expect("ok").as_deref(),
        Some("/opt/crook")
    );
    assert_eq!(
        crook_bin(&env, &exe, true).expect("ok").as_deref(),
        Some("/opt/crook")
    );
}

#[test]
fn a_relative_crook_bin_is_refused() {
    let exe = || Ok(PathBuf::from("/self/crook"));
    for debug in [false, true] {
        let err = crook_bin(&env_of(&[("CROOK_BIN", "bin/crook")]), &exe, debug)
            .expect_err("refused")
            .to_string();
        assert!(
            err.contains("CROOK_BIN") && err.contains("absolute"),
            "{err}"
        );
    }
}

#[test]
fn crook_bin_is_unset_in_a_release_build() {
    let exe = || Ok(PathBuf::from("/self/crook"));
    assert_eq!(crook_bin(&env_of(&[]), &exe, false).expect("ok"), None);
    assert_eq!(
        crook_bin(&env_of(&[("CROOK_BIN", "")]), &exe, false).expect("ok"),
        None
    );
}

#[test]
fn crook_bin_is_the_own_executable_in_a_debug_build() {
    let exe = || Ok(PathBuf::from("/self/crook"));
    assert_eq!(
        crook_bin(&env_of(&[]), &exe, true).expect("ok").as_deref(),
        Some("/self/crook")
    );
    assert_eq!(
        crook_bin(&env_of(&[("CROOK_BIN", "")]), &exe, true)
            .expect("ok")
            .as_deref(),
        Some("/self/crook")
    );
    let failing = || Err(std::io::Error::other("no exe"));
    assert_eq!(crook_bin(&env_of(&[]), &failing, true).expect("ok"), None);
}
