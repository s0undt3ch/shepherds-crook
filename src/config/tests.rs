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
