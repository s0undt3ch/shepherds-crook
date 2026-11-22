use std::path::Path;

use super::*;

fn env_in(home: &std::path::Path) -> impl Fn(&str) -> Option<String> + use<> {
    let h = home.display().to_string();
    move |k: &str| (k == "HOME").then(|| h.clone())
}

fn write(home: &std::path::Path, text: &str) {
    let f = paths::config_file(&env_in(home));
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(f, text).unwrap();
}

#[test]
fn missing_file_is_empty() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(load(&env_in(tmp.path())).unwrap(), Config::default());
}

#[test]
fn orchestrator_label_config() {
    let tmp = tempfile::tempdir().unwrap();
    let env = env_in(tmp.path());
    assert_eq!(load(&env).unwrap().orchestrator_label(), "\u{1F415}");
    assert_eq!(ORCHESTRATOR_LABEL.chars().count(), 1);
    write(tmp.path(), "[herdr]\norchestrator_label = \"boss\"\n");
    assert_eq!(load(&env).unwrap().orchestrator_label(), "boss");
    write(tmp.path(), "[herdr]\norchestrator_label = \"  \"\n");
    assert_eq!(load(&env).unwrap().orchestrator_label(), ORCHESTRATOR_LABEL);
    write(tmp.path(), "[herdr]\n");
    assert_eq!(load(&env).unwrap().orchestrator_label(), ORCHESTRATOR_LABEL);
    write(tmp.path(), "[herdr]\nnope = 1\n");
    assert!(load(&env).is_err());
}

#[test]
fn parses_both_arrays() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "private = [\"~/vault\"]\nallow = [\"~/vault/pub\"]\n",
    );
    let c = load(&env_in(tmp.path())).unwrap();
    assert_eq!(c.private, ["~/vault"]);
    assert_eq!(c.allow, ["~/vault/pub"]);
}

#[test]
fn partial_file_defaults_the_rest() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "private = [\"a\"]\n");
    let c = load(&env_in(tmp.path())).unwrap();
    assert_eq!(c.private, ["a"]);
    assert!(c.allow.is_empty());
}

#[test]
fn unknown_key_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "privat = [\"a\"]\n");
    let e = load(&env_in(tmp.path())).unwrap_err();
    assert!(matches!(e, Error::Config { .. }), "{e}");
    assert!(e.to_string().contains("config.toml"), "{e}");
}

#[test]
fn bad_toml_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "private = [");
    assert!(matches!(
        load(&env_in(tmp.path())),
        Err(Error::Config { .. })
    ));
}

fn cfg(home: Option<&str>, diary_root: Option<&str>) -> Config {
    Config {
        home: home.map(str::to_string),
        diary_root: diary_root.map(str::to_string),
        ..Config::default()
    }
}

fn with<R>(pairs: &[(&str, &str)], f: impl FnOnce(Env) -> R) -> R {
    let m: std::collections::HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    f(&move |k: &str| m.get(k).cloned())
}

#[test]
fn home_config_reads_home_and_diary_root() {
    let t = tempfile::tempdir().unwrap();
    let f = paths::config_file(&|k: &str| (k == "HOME").then(|| t.path().display().to_string()));
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, "home = \"/h\"\ndiary_root = \"/v\"\n").unwrap();
    let c = load(&|k: &str| (k == "HOME").then(|| t.path().display().to_string())).unwrap();
    assert_eq!(c.home.as_deref(), Some("/h"));
    assert_eq!(c.diary_root.as_deref(), Some("/v"));
}

#[test]
fn home_config_default_follows_xdg_data_home() {
    let c = cfg(None, None);
    assert_eq!(
        with(&[("XDG_DATA_HOME", "/d")], |e| c.project_home(e, "demo")).unwrap(),
        Path::new("/d/crook/demo")
    );
    assert_eq!(
        with(&[("HOME", "/h")], |e| c.project_home(e, "demo")).unwrap(),
        Path::new("/h/.local/share/crook/demo")
    );
    // An empty variable counts as unset.
    assert_eq!(
        with(&[("XDG_DATA_HOME", ""), ("HOME", "/h")], |e| c
            .project_home(e, "demo"))
        .unwrap(),
        Path::new("/h/.local/share/crook/demo")
    );
}

#[test]
fn home_config_override_and_tilde() {
    let c = cfg(Some("~/projects-home"), None);
    assert_eq!(
        with(&[("HOME", "/h")], |e| c.project_home(e, "demo")).unwrap(),
        Path::new("/h/projects-home/demo")
    );
    let c = cfg(Some("/x/./y/../home/"), None);
    assert_eq!(
        with(&[], |e| c.project_home(e, "demo")).unwrap(),
        Path::new("/x/home/demo")
    );
    for bad in ["rel/dir", "", "~other/x"] {
        let c = cfg(Some(bad), None);
        assert!(
            with(&[("HOME", "/h")], |e| c.home_root(e)).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn home_config_inside_the_state_dir_is_refused() {
    for home in ["/s/crook", "/s/crook/demo", "/s/crook/a/../b"] {
        let c = cfg(Some(home), None);
        let e = with(&[("XDG_STATE_HOME", "/s")], |e| c.home_root(e)).unwrap_err();
        assert!(e.to_string().contains("state dir"), "{home}: {e}");
    }
    // The default home sits beside the state dir, not inside it.
    let c = cfg(None, None);
    assert!(with(&[("HOME", "/h")], |e| c.home_root(e)).is_ok());
    // Through a symlink too.
    let t = tempfile::tempdir().unwrap();
    let state = t.path().join("state");
    std::fs::create_dir_all(state.join("crook")).unwrap();
    std::os::unix::fs::symlink(state.join("crook"), t.path().join("link")).unwrap();
    let c = cfg(Some(&t.path().join("link/h").display().to_string()), None);
    let s = state.display().to_string();
    assert!(with(&[("XDG_STATE_HOME", &s)], |e| c.home_root(e)).is_err());
}

#[test]
fn diary_location() {
    let c = cfg(None, None);
    assert_eq!(
        with(&[("XDG_DATA_HOME", "/d")], |e| c.diary(e, "demo")).unwrap(),
        Path::new("/d/crook/demo/diary")
    );
    let c = cfg(None, Some("~/vault/projects"));
    assert_eq!(
        with(&[("HOME", "/h")], |e| c.diary(e, "demo")).unwrap(),
        Path::new("/h/vault/projects/demo")
    );
}

#[test]
fn diaries_glob_follows_the_config_and_never_fails() {
    let g = |c: &Config| with(&[("XDG_DATA_HOME", "/d")], |e| c.diaries_glob(e));
    assert_eq!(g(&cfg(None, None)).as_deref(), Some("/d/crook/*/diary"));
    assert_eq!(g(&cfg(Some("/h"), None)).as_deref(), Some("/h/*/diary"));
    assert_eq!(g(&cfg(None, Some("/v"))).as_deref(), Some("/v/*"));
    assert_eq!(g(&cfg(Some("rel"), None)), None);
}
