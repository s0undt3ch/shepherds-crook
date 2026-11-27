use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::*;
use crate::testutil::{Buffers, Fixture, fake_install};

fn app_home(fx: &Fixture) -> PathBuf {
    let home = fx.root().join("app");
    fx.set_env("HERDR_APP_HOME", &home.display().to_string());
    home
}

/// Every file under `dir` with its text, by path relative to `dir`.
fn tree(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(dir).expect("read_dir").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path.strip_prefix(base).expect("under base");
                out.insert(
                    rel.display().to_string(),
                    std::fs::read_to_string(&path).expect("read"),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn setup(fx: &Fixture, install: &Install, debug: bool) -> (Result<(), Error>, Buffers) {
    let mut bufs = Buffers::default();
    let out = perform(&fx.ctx(), &mut bufs.io(), install, debug);
    (out, bufs)
}

fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| (*n).to_string()).collect()
}

#[test]
fn a_fresh_install_copies_the_shipped_files_and_writes_the_marker() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    let (out, bufs) = setup(&fx, &install, false);
    out.expect("setup works");
    let dest = home.join("mods/crook");
    assert_eq!(
        tree(&dest).keys().cloned().collect::<Vec<_>>(),
        files(&[".crook-version", "lib.js", "main.js", "mod.json"])
    );
    assert_eq!(
        std::fs::read_to_string(dest.join(".crook-version")).expect("marker"),
        "0.2.0\n"
    );
    assert!(bufs.out.text().contains("0.2.0"), "{}", bufs.out.text());
}

#[test]
fn the_app_home_defaults_to_dot_config_herdr_app() {
    let fx = Fixture::new();
    let install = fake_install(&fx, "0.2.0");
    setup(&fx, &install, false).0.expect("setup works");
    assert!(
        fx.root()
            .join("home/.config/herdr-app/mods/crook/.crook-version")
            .is_file()
    );
}

#[test]
fn herdr_app_home_is_honoured_and_nothing_lands_in_the_default() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    setup(&fx, &install, false).0.expect("setup works");
    assert!(home.join("mods/crook/mod.json").is_file());
    assert!(!fx.root().join("home/.config/herdr-app").exists());
}

#[test]
fn a_reinstall_replaces_the_folder_whole() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    setup(&fx, &fake_install(&fx, "0.1.0"), false)
        .0
        .expect("first");
    let dest = home.join("mods/crook");
    std::fs::write(dest.join("gone-in-0.2.js"), "old").expect("write");
    setup(&fx, &fake_install(&fx, "0.2.0"), false)
        .0
        .expect("second");
    assert!(!dest.join("gone-in-0.2.js").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join(".crook-version")).expect("marker"),
        "0.2.0\n"
    );
    // Nothing but the mod is left next to it, and no staging folder is left in the app home.
    assert_eq!(
        std::fs::read_dir(home.join("mods")).expect("mods").count(),
        1
    );
    let leftovers: Vec<_> = std::fs::read_dir(&home)
        .expect("home")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers, ["mods"], "{leftovers:?}");
}

#[test]
fn a_folder_without_the_marker_belongs_to_someone_else_and_is_left_alone() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let dest = home.join("mods/crook");
    std::fs::create_dir_all(&dest).expect("mkdir");
    std::fs::write(dest.join("main.js"), "mine").expect("write");
    let (out, _) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    let msg = out.expect_err("refused").to_string();
    assert!(msg.contains("someone else"), "{msg}");
    assert_eq!(
        tree(&dest),
        BTreeMap::from([("main.js".into(), "mine".into())])
    );
    // Refused before any other step: no flocks root, no staging.
    assert!(!fx.flocks_root().exists());
    assert_eq!(std::fs::read_dir(&home).expect("home").count(), 1);
}

#[test]
fn a_file_at_the_mod_path_is_not_ours_either() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    std::fs::create_dir_all(home.join("mods")).expect("mkdir");
    std::fs::write(home.join("mods/crook"), "a file").expect("write");
    let (out, _) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    assert!(out.is_err());
    assert_eq!(
        std::fs::read_to_string(home.join("mods/crook")).expect("still there"),
        "a file"
    );
}

#[test]
fn a_newer_marker_is_not_replaced() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    setup(&fx, &fake_install(&fx, "0.3.0"), false)
        .0
        .expect("first");
    let before = tree(&home.join("mods/crook"));
    let (out, _) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    let msg = out.expect_err("refused").to_string();
    assert!(msg.contains("newer") && msg.contains("0.3.0"), "{msg}");
    assert_eq!(tree(&home.join("mods/crook")), before);
}

#[test]
fn a_release_replaces_its_own_pre_release() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    setup(&fx, &fake_install(&fx, "0.1.0-rc.1"), false)
        .0
        .expect("rc");
    setup(&fx, &fake_install(&fx, "0.1.0"), false)
        .0
        .expect("release");
    assert_eq!(
        std::fs::read_to_string(home.join("mods/crook/.crook-version")).expect("marker"),
        "0.1.0\n"
    );
}

#[test]
fn an_unreadable_marker_is_refused() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let dest = home.join("mods/crook");
    std::fs::create_dir_all(&dest).expect("mkdir");
    std::fs::write(dest.join(".crook-version"), "garbage\n").expect("write");
    let msg = setup(&fx, &fake_install(&fx, "0.2.0"), false)
        .0
        .expect_err("refused")
        .to_string();
    assert!(msg.contains("garbage"), "{msg}");
}

#[test]
fn the_staged_folder_has_no_marker_until_the_copy_ends_and_is_never_under_mods() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    let seen = std::cell::RefCell::new(Vec::new());
    install_mod_with(&fx.ctx(), &install, "0.2.0", &|staging: &Path| {
        let mut state = seen.borrow_mut();
        state.push(staging.join("main.js").is_file());
        state.push(staging.join(".crook-version").exists());
        state.push(staging.starts_with(home.join("mods")));
        state.push(home.join("mods").exists());
    })
    .expect("installs");
    // Copied, not yet marked, outside `mods/`, and `mods/` itself not made before the swap.
    assert_eq!(*seen.borrow(), [true, false, false, false]);
    assert!(home.join("mods/crook/.crook-version").is_file());
}

#[test]
fn running_setup_twice_leaves_one_identical_install() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    setup(&fx, &install, false).0.expect("first");
    let first = tree(&home.join("mods/crook"));
    setup(&fx, &install, false).0.expect("second");
    assert_eq!(tree(&home.join("mods/crook")), first);
    assert_eq!(
        std::fs::read_dir(home.join("mods")).expect("mods").count(),
        1
    );
}

#[test]
fn an_install_without_a_mod_folder_fails_before_changing_anything() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    std::fs::remove_dir_all(install.mod_dir()).expect("rm");
    let msg = setup(&fx, &install, false)
        .0
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("mod"), "{msg}");
    assert!(!home.exists());
}

#[test]
fn a_release_build_moves_the_current_link_and_a_debug_build_does_not() {
    let fx = Fixture::new();
    app_home(&fx);
    let install = fake_install(&fx, "0.2.0");
    let (out, bufs) = setup(&fx, &install, true);
    out.expect("debug setup");
    let link = fx.root().join("data/crook/current");
    assert!(std::fs::symlink_metadata(&link).is_err());
    assert!(
        bufs.out.text().contains("debug build"),
        "{}",
        bufs.out.text()
    );

    setup(&fx, &install, false).0.expect("release setup");
    assert_eq!(std::fs::read_link(&link).expect("link"), install.root());
}

#[test]
fn a_link_setup_cannot_move_is_a_warning_not_a_failure() {
    let fx = Fixture::new();
    app_home(&fx);
    let link = fx.root().join("data/crook/current");
    std::fs::create_dir_all(link.parent().expect("parent")).expect("mkdir");
    std::fs::write(&link, "a plain file").expect("write");
    let (out, bufs) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    out.expect("still succeeds");
    assert!(
        bufs.err.text().contains("not a link"),
        "{}",
        bufs.err.text()
    );
}

#[test]
fn the_flocks_root_is_created() {
    let fx = Fixture::new();
    app_home(&fx);
    setup(&fx, &fake_install(&fx, "0.2.0"), false)
        .0
        .expect("setup");
    assert!(fx.flocks_root().is_dir());
}

#[test]
fn one_trust_command_is_printed_per_root_and_none_is_run() {
    let fx = Fixture::new();
    app_home(&fx);
    let notes = fx.root().join("my notes");
    fx.write_config(&format!("notes_root = \"{}\"\n", notes.display()));
    let (out, bufs) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    out.expect("setup");
    let text = bufs.out.text();
    let trust: Vec<&str> = text.lines().filter(|l| l.contains("&& claude")).collect();
    assert_eq!(trust.len(), 2, "{text}");
    assert!(trust[0].contains(&fx.flocks_root().display().to_string()));
    // A path with a space is quoted so the command pastes as printed.
    assert!(
        trust[1].contains(&format!("cd '{}' && claude", notes.display())),
        "{}",
        trust[1]
    );
    assert!(
        notes.is_dir(),
        "the notes root exists, so the command works"
    );
    // Nothing was launched, not even `claude`.
    assert!(
        fx.runner.programs().is_empty(),
        "{:?}",
        fx.runner.programs()
    );
}

#[test]
fn without_a_notes_root_only_the_flocks_root_is_listed() {
    let fx = Fixture::new();
    app_home(&fx);
    let (out, bufs) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    out.expect("setup");
    assert_eq!(
        bufs.out
            .text()
            .lines()
            .filter(|l| l.contains("&& claude"))
            .count(),
        1
    );
}

#[test]
fn a_broken_config_warns_and_does_not_stop_the_install() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    fx.write_config("nonsense = 1\n");
    let (out, bufs) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    out.expect("setup");
    assert!(home.join("mods/crook/.crook-version").is_file());
    assert!(bufs.err.text().contains("config"), "{}", bufs.err.text());
}

#[test]
fn session_records_older_than_thirty_days_are_pruned_and_newer_ones_stay() {
    let fx = Fixture::new();
    app_home(&fx);
    let dir = fx.root().join("state/crook/sessions");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let now = fx.ctx().clock.now();
    for (name, age_days) in [("old", 31u64), ("recent", 29)] {
        let path = dir.join(name);
        std::fs::write(&path, "{}").expect("write");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open")
            .set_modified(now - Duration::from_secs(age_days * 24 * 60 * 60))
            .expect("mtime");
    }
    let (out, bufs) = setup(&fx, &fake_install(&fx, "0.2.0"), false);
    out.expect("setup");
    assert!(!dir.join("old").exists());
    assert!(dir.join("recent").exists());
    assert!(bufs.out.text().contains("pruned 1"), "{}", bufs.out.text());
}

#[test]
fn a_setup_killed_between_its_renames_is_recovered_before_the_marker_checks() {
    let fx = Fixture::new();
    let home = app_home(&fx);
    setup(&fx, &fake_install(&fx, "0.3.0"), false)
        .0
        .expect("first");
    // The old folder is aside and nothing is in place, as after a crash.
    std::fs::rename(home.join("mods/crook"), home.join(".crook-mod-old")).expect("aside");
    let msg = setup(&fx, &fake_install(&fx, "0.2.0"), false)
        .0
        .expect_err("a newer mod is still refused")
        .to_string();
    assert!(msg.contains("newer"), "{msg}");
    assert_eq!(
        std::fs::read_to_string(home.join("mods/crook/.crook-version")).expect("restored"),
        "0.3.0\n"
    );

    // An older one that was set aside is replaced as usual, and nothing is left aside.
    setup(&fx, &fake_install(&fx, "0.3.0"), false)
        .0
        .expect("same version");
    std::fs::rename(home.join("mods/crook"), home.join(".crook-mod-old")).expect("aside");
    setup(&fx, &fake_install(&fx, "0.4.0"), false)
        .0
        .expect("newer binary");
    assert!(!home.join(".crook-mod-old").exists());
    assert_eq!(
        std::fs::read_to_string(home.join("mods/crook/.crook-version")).expect("marker"),
        "0.4.0\n"
    );
}
