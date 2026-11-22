use std::collections::HashMap;
use std::path::Path;

use super::*;

fn env_in(dir: &Path, extra: &[(&str, String)]) -> impl Fn(&str) -> Option<String> + use<> {
    let d = dir.display().to_string();
    let mut m: HashMap<String, String> = HashMap::from([
        ("HOME".into(), format!("{d}/h")),
        ("XDG_DATA_HOME".into(), format!("{d}/data")),
        ("XDG_STATE_HOME".into(), format!("{d}/state")),
        ("XDG_CONFIG_HOME".into(), format!("{d}/cfg")),
    ]);
    for (k, v) in extra {
        m.insert((*k).into(), v.clone());
    }
    move |k: &str| m.get(k).cloned()
}

fn config(dir: &Path, toml: &str) {
    let f = dir.join("cfg/crook/config.toml");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, toml).unwrap();
}

fn notes(env: Env) -> Vec<String> {
    Store::open(env, "demo", Mode::ReadOnly)
        .unwrap()
        .read(project::get)
        .unwrap()
        .notes
}

#[test]
fn init_home() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path(), &[]);
    run(&env, "demo").unwrap();
    let home = t.path().join("data/crook/demo");
    assert!(home.join("diary").is_dir());
    assert!(!home.join("repos").exists());
    assert_eq!(notes(&env), [home.join("diary").display().to_string()]);
}

#[test]
fn init_home_with_diary_root() {
    let t = tempfile::tempdir().unwrap();
    let vault = t.path().join("vault/projects");
    config(
        t.path(),
        &format!("diary_root = {:?}\n", vault.display().to_string()),
    );
    let env = env_in(t.path(), &[]);
    run(&env, "demo").unwrap();
    let home = t.path().join("data/crook/demo");
    assert!(home.is_dir());
    assert!(!home.join("diary").exists());
    assert!(vault.join("demo").is_dir());
    assert_eq!(notes(&env), [vault.join("demo").display().to_string()]);
}

#[test]
fn init_home_in_a_private_location_is_refused() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().display().to_string();
    config(
        t.path(),
        &format!("home = \"{base}/vault/crook\"\nprivate = [\"{base}/vault\"]\n"),
    );
    let env = env_in(t.path(), &[]);
    let e = run(&env, "demo").unwrap_err();
    assert!(e.to_string().contains("private location"), "{e}");
    assert!(!t.path().join("vault").exists());
    assert!(!t.path().join("state/crook/demo").exists());
}

#[test]
fn init_refuses_an_existing_project_without_touching_dirs() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path(), &[]);
    run(&env, "demo").unwrap();
    std::fs::remove_dir_all(t.path().join("data")).unwrap();
    let e = run(&env, "demo").unwrap_err();
    assert!(e.to_string().contains("already exists"), "{e}");
    assert!(!t.path().join("data").exists());
}

#[test]
fn init_marks_the_home_it_makes() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path(), &[]);
    run(&env, "demo").unwrap();
    assert!(paths::is_crook_home(
        &t.path().join("data/crook/demo"),
        "demo"
    ));
    // An empty directory is made crook's too.
    let empty = t.path().join("data/crook/other");
    std::fs::create_dir_all(&empty).unwrap();
    run(&env, "other").unwrap();
    assert!(paths::is_crook_home(&empty, "other"));
}

#[test]
fn init_adopts_only_a_home_crook_made_for_the_slug() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path(), &[]);
    run(&env, "demo").unwrap();
    let home = t.path().join("data/crook/demo");
    std::fs::write(home.join("diary/n.md"), "n").unwrap();
    std::fs::remove_dir_all(t.path().join("state")).unwrap();
    run(&env, "demo").unwrap();
    assert!(home.join("diary/n.md").is_file());

    // The same home claimed for another slug, or a directory with no marker.
    for marker in [Some("crook project home: other\n"), None] {
        let _ = std::fs::remove_dir_all(t.path().join("state"));
        match marker {
            Some(m) => std::fs::write(home.join(paths::HOME_MARKER), m).unwrap(),
            None => std::fs::remove_file(home.join(paths::HOME_MARKER)).unwrap(),
        }
        let e = run(&env, "demo").unwrap_err();
        assert!(e.to_string().contains("crook did not make it"), "{e}");
        assert!(!t.path().join("state/crook/demo").exists());
        assert!(home.join("diary/n.md").is_file());
    }
}

#[test]
fn init_refuses_a_bad_home() {
    let t = tempfile::tempdir().unwrap();
    let env = env_in(t.path(), &[]);
    config(t.path(), "home = \"relative/dir\"\n");
    assert!(run(&env, "demo").is_err());
    let inside = format!("home = \"{}/state/crook/x\"\n", t.path().display());
    config(t.path(), &inside);
    let e = run(&env, "demo").unwrap_err();
    assert!(e.to_string().contains("state dir"), "{e}");
}
