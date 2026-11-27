use std::path::Path;

use super::*;
use crate::testutil::env_of;

fn parse_ok(text: &str) -> Config {
    parse(text, Path::new("/c/config.toml"), Some(Path::new("/h"))).expect("parses")
}

fn parse_err(text: &str) -> String {
    parse(text, Path::new("/c/config.toml"), Some(Path::new("/h")))
        .expect_err("fails")
        .to_string()
}

#[test]
fn a_missing_file_means_every_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let env = env_of(&[("XDG_CONFIG_HOME", dir.path().to_str().expect("utf8"))]);
    assert_eq!(load(&env).expect("loads"), Config::default());
    assert_eq!(Config::default().default_child_model, "sonnet");
}

#[test]
fn load_reads_the_file_under_xdg_config_home() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("crook")).expect("mkdir");
    std::fs::write(
        dir.path().join("crook/config.toml"),
        "default_child_model = \"haiku\"\n",
    )
    .expect("write");
    let env = env_of(&[("XDG_CONFIG_HOME", dir.path().to_str().expect("utf8"))]);
    assert_eq!(load(&env).expect("loads").default_child_model, "haiku");
}

#[test]
fn an_unreadable_path_is_an_error_naming_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A folder where the file should be.
    std::fs::create_dir_all(dir.path().join("crook/config.toml")).expect("mkdir");
    let env = env_of(&[("XDG_CONFIG_HOME", dir.path().to_str().expect("utf8"))]);
    let msg = load(&env).expect_err("fails").to_string();
    assert!(msg.contains("config.toml"), "{msg}");
}

#[test]
fn an_empty_file_means_every_default() {
    assert_eq!(parse_ok(""), Config::default());
}

#[test]
fn notes_root_absolute_is_kept() {
    assert_eq!(
        parse_ok("notes_root = \"/notes\"").notes_root.as_deref(),
        Some(Path::new("/notes"))
    );
}

#[test]
fn notes_root_expands_a_leading_tilde_slash_to_home() {
    assert_eq!(
        parse_ok("notes_root = \"~/notes/crook\"")
            .notes_root
            .as_deref(),
        Some(Path::new("/h/notes/crook"))
    );
}

#[test]
fn notes_root_relative_is_an_error_naming_the_file() {
    for bad in ["notes", "./notes", "~notes", "~"] {
        let msg = parse_err(&format!("notes_root = \"{bad}\""));
        assert!(msg.contains("/c/config.toml"), "{msg}");
        assert!(msg.contains("notes_root"), "{msg}");
    }
}

#[test]
fn an_empty_notes_root_reads_as_unset() {
    assert_eq!(parse_ok("notes_root = \"\"").notes_root, None);
}

#[test]
fn default_child_model_is_sonnet_unset_and_the_value_when_set() {
    assert_eq!(parse_ok("").default_child_model, "sonnet");
    assert_eq!(
        parse_ok("default_child_model = \"\"").default_child_model,
        "sonnet"
    );
    assert_eq!(
        parse_ok("default_child_model = \"opus\"").default_child_model,
        "opus"
    );
}

#[test]
fn coordinator_model_unset_and_empty_both_give_none() {
    assert_eq!(parse_ok("").coordinator_model, None);
    assert_eq!(parse_ok("coordinator_model = \"\"").coordinator_model, None);
    assert_eq!(
        parse_ok("coordinator_model = \"haiku\"").coordinator_model,
        Some("haiku".to_string())
    );
}

#[test]
fn an_unknown_key_is_an_error_naming_the_key() {
    let msg = parse_err("notes_rot = \"/x\"");
    assert!(msg.contains("notes_rot"), "{msg}");
    assert!(msg.contains("/c/config.toml"), "{msg}");
}

#[test]
fn bad_toml_is_an_error_naming_the_file() {
    let msg = parse_err("notes_root = ");
    assert!(msg.contains("/c/config.toml"), "{msg}");
}

#[test]
fn a_value_of_the_wrong_type_is_an_error() {
    assert!(parse_err("coordinator_model = 3").contains("/c/config.toml"));
}

#[test]
fn a_tilde_notes_root_without_a_home_is_an_error() {
    let err = parse("notes_root = \"~/n\"", Path::new("/c/config.toml"), None)
        .expect_err("fails")
        .to_string();
    assert!(err.contains("HOME"), "{err}");
    assert!(parse("notes_root = \"/n\"", Path::new("/c/config.toml"), None).is_ok());
}
