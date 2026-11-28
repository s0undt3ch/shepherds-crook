use std::path::Path;

use super::*;
use crate::files::roster::Roster;
use crate::testutil::env_of;

fn data(dir: &Path) -> impl Fn(&str) -> Option<String> + use<> {
    env_of(&[("XDG_DATA_HOME", dir.to_str().expect("utf8"))])
}

/// A flock with its roster, as `new` leaves it.
fn make(env: Env, config: &Config, date: &str, slug: &str) -> Flock {
    let flock = create(env, config, date, slug).expect("creates");
    roster::create(&flock.folder, &Roster::new(slug, "sid", &flock.folder)).expect("roster");
    flock
}

#[test]
fn create_lays_out_the_folder_under_the_data_home() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let flock = create(&env, &Config::default(), "2026-11-27", "demo").expect("creates");
    assert_eq!(
        flock.folder,
        dir.path().join("crook/flocks/2026-11-27-demo")
    );
    assert!(flock.folder.join("briefs").is_dir());
    assert!(flock.folder.join("results").is_dir());
    assert!(flock.folder.join(".lock").is_file());
    assert_eq!(flock.dir_name(), "2026-11-27-demo");
}

#[test]
fn find_returns_the_flock_by_slug() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let made = make(&env, &Config::default(), "2026-11-27", "demo");
    make(&env, &Config::default(), "2026-11-28", "other");
    assert_eq!(find(&env, "demo").expect("finds"), made);
}

#[test]
fn find_names_an_unknown_slug_and_rejects_a_bad_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let msg = find(&env, "nope").expect_err("fails").to_string();
    assert!(msg.contains("nope"), "{msg}");
    assert!(find(&env, "../x").is_err());
}

#[test]
fn a_slug_that_ends_another_slug_is_not_confused_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    make(&env, &Config::default(), "2026-11-27", "a-b");
    assert!(find(&env, "b").is_err());
    make(&env, &Config::default(), "2026-11-27", "b");
    assert_eq!(find(&env, "b").expect("finds").slug, "b");
}

#[test]
fn new_refuses_a_slug_any_folder_uses_even_on_another_date() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    create(&env, &Config::default(), "2026-11-27", "demo").expect("creates");
    let msg = create(&env, &Config::default(), "2026-12-01", "demo")
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("2026-11-27-demo"), "{msg}");
    assert!(!dir.path().join("crook/flocks/2026-12-01-demo").exists());
}

#[test]
fn two_folders_with_one_slug_is_an_error_naming_both() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let root = dir.path().join("crook/flocks");
    std::fs::create_dir_all(root.join("2026-11-27-demo")).expect("mkdir");
    std::fs::create_dir_all(root.join("2026-12-01-demo")).expect("mkdir");
    let msg = find(&env, "demo").expect_err("fails").to_string();
    assert!(
        msg.contains("2026-11-27-demo") && msg.contains("2026-12-01-demo"),
        "{msg}"
    );
}

#[test]
fn list_is_sorted_and_skips_files_and_other_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    assert!(list(&env).expect("lists").is_empty());
    let root = dir.path().join("crook/flocks");
    create(&env, &Config::default(), "2026-12-01", "b").expect("creates");
    create(&env, &Config::default(), "2026-11-27", "a").expect("creates");
    std::fs::create_dir_all(root.join("scratch")).expect("mkdir");
    std::fs::write(root.join("2026-01-01-file"), "").expect("write");
    let slugs: Vec<_> = list(&env)
        .expect("lists")
        .into_iter()
        .map(|f| f.slug)
        .collect();
    assert_eq!(slugs, ["a", "b"]);
}

#[test]
fn the_notes_folder_is_the_flock_folder_without_notes_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let flock = create(&env, &Config::default(), "2026-11-27", "demo").expect("creates");
    assert_eq!(flock.notes_folder(&Config::default()), flock.folder);
    assert!(!flock.folder.join("notes").exists());
}

#[test]
fn with_notes_root_the_notes_folder_is_made_and_left_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let notes = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let config = Config {
        notes_root: Some(notes.path().to_path_buf()),
        ..Config::default()
    };
    let flock = create(&env, &config, "2026-11-27", "demo").expect("creates");
    let folder = notes.path().join("2026-11-27-demo");
    assert_eq!(flock.notes_folder(&config), folder);
    assert!(folder.is_dir());
    assert_eq!(std::fs::read_dir(&folder).expect("lists").count(), 0);
    assert!(!flock.folder.join("README.md").exists());
}

#[test]
fn a_bad_slug_or_date_is_refused_before_anything_is_made() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    assert!(create(&env, &Config::default(), "2026-11-27", "Bad").is_err());
    assert!(create(&env, &Config::default(), "yesterday", "demo").is_err());
    assert!(!dir.path().join("crook").exists());
}

#[test]
fn a_folder_without_a_roster_is_a_broken_flock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    create(&env, &Config::default(), "2026-11-27", "demo").expect("creates");
    let msg = find(&env, "demo").expect_err("fails").to_string();
    assert!(
        msg.contains("broken flock") && msg.contains("roster.json"),
        "{msg}"
    );
}

#[test]
fn a_roster_naming_another_flock_is_a_broken_flock() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let flock = create(&env, &Config::default(), "2026-11-27", "demo").expect("creates");
    roster::create(&flock.folder, &Roster::new("other", "sid", &flock.folder)).expect("roster");
    let msg = find(&env, "demo").expect_err("fails").to_string();
    assert!(
        msg.contains("broken flock") && msg.contains("other"),
        "{msg}"
    );
}

#[test]
fn a_failed_create_does_not_burn_the_slug() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    let bad = Config {
        notes_root: Some("/dev/null/notes".into()),
        ..Config::default()
    };
    create(&env, &bad, "2026-11-27", "demo").expect_err("notes folder cannot be made");
    assert!(!dir.path().join("crook/flocks/2026-11-27-demo").exists());
    assert!(list(&env).expect("lists").is_empty());
    create(&env, &Config::default(), "2026-11-27", "demo").expect("the retry works");
}

#[test]
fn a_folder_name_stands_for_its_slug_and_a_real_slug_wins() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = data(dir.path());
    make(&env, &Config::default(), "2026-11-27", "login-bug");
    assert_eq!(
        slug_for(&env, "2026-11-27-login-bug").expect("maps"),
        "login-bug"
    );
    assert_eq!(slug_for(&env, "login-bug").expect("same"), "login-bug");
    assert_eq!(
        slug_for(&env, "2026-11-28-login-bug").expect("passes"),
        "2026-11-28-login-bug"
    );
    make(&env, &Config::default(), "2026-11-27", "2026-11-27-ab");
    make(&env, &Config::default(), "2026-11-28", "ab");
    assert_eq!(
        slug_for(&env, "2026-11-27-ab").expect("slug wins"),
        "2026-11-27-ab"
    );
}
