use std::path::{Path, PathBuf};

use super::*;
use crate::testutil::env_of;

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

fn ok(r: Result<PathBuf, Error>) -> PathBuf {
    r.expect("resolves")
}

#[test]
fn data_home_uses_the_variable_then_home() {
    assert_eq!(
        ok(data_home(&env_of(&[
            ("XDG_DATA_HOME", "/x"),
            ("HOME", "/h")
        ]))),
        p("/x")
    );
    assert_eq!(
        ok(data_home(&env_of(&[("HOME", "/h")]))),
        p("/h/.local/share")
    );
    assert_eq!(
        ok(data_home(&env_of(&[("XDG_DATA_HOME", ""), ("HOME", "/h")]))),
        p("/h/.local/share")
    );
}

#[test]
fn a_relative_xdg_value_is_ignored() {
    let env = env_of(&[("XDG_DATA_HOME", "rel/x"), ("HOME", "/h")]);
    assert_eq!(ok(data_home(&env)), p("/h/.local/share"));
    let env = env_of(&[("XDG_CONFIG_HOME", "."), ("HOME", "/h")]);
    assert_eq!(ok(config_file(&env)), p("/h/.config/crook/config.toml"));
    let env = env_of(&[("XDG_STATE_HOME", "s"), ("HOME", "/h")]);
    assert_eq!(ok(sessions_dir(&env)), p("/h/.local/state/crook/sessions"));
    let env = env_of(&[("HERDR_APP_HOME", "app"), ("HOME", "/h")]);
    assert_eq!(ok(herdr_app_home(&env)), p("/h/.config/herdr-app"));
}

#[test]
fn a_missing_empty_or_relative_home_is_an_error_when_it_is_needed() {
    for env in [
        env_of(&[]),
        env_of(&[("HOME", "")]),
        env_of(&[("HOME", "relative/home")]),
    ] {
        let msg = data_home(&env).expect_err("fails").to_string();
        assert!(msg.contains("HOME"), "{msg}");
        assert!(config_file(&env).is_err());
        assert!(sessions_dir(&env).is_err());
        assert!(herdr_app_home(&env).is_err());
        assert!(mise_shim(&env).is_err());
    }
}

#[test]
fn home_is_not_needed_when_the_variable_is_absolute() {
    assert_eq!(ok(data_home(&env_of(&[("XDG_DATA_HOME", "/x")]))), p("/x"));
    assert_eq!(
        ok(herdr_app_home(&env_of(&[("HERDR_APP_HOME", "/a")]))),
        p("/a")
    );
    assert_eq!(
        ok(config_file(&env_of(&[("XDG_CONFIG_HOME", "/c")]))),
        p("/c/crook/config.toml")
    );
}

#[test]
fn flock_paths_hang_off_the_data_home() {
    let env = env_of(&[("XDG_DATA_HOME", "/d")]);
    assert_eq!(ok(flocks_root(&env)), p("/d/crook/flocks"));
    assert_eq!(
        ok(flock_folder(&env, "2026-11-27-demo")),
        p("/d/crook/flocks/2026-11-27-demo")
    );
    assert_eq!(ok(current_link(&env)), p("/d/crook/current"));
}

#[test]
fn notes_folder_follows_notes_root() {
    let flock = p("/d/crook/flocks/2026-11-27-demo");
    assert_eq!(notes_folder(&flock, "2026-11-27-demo", None), flock);
    assert_eq!(
        notes_folder(&flock, "2026-11-27-demo", Some(Path::new("/notes"))),
        p("/notes/2026-11-27-demo")
    );
}

#[test]
fn config_file_uses_xdg_config_home_on_every_platform() {
    assert_eq!(
        ok(config_file(&env_of(&[
            ("XDG_CONFIG_HOME", "/c"),
            ("HOME", "/h")
        ]))),
        p("/c/crook/config.toml")
    );
    assert_eq!(
        ok(config_file(&env_of(&[("HOME", "/h")]))),
        p("/h/.config/crook/config.toml")
    );
    assert_eq!(
        ok(config_file(&env_of(&[
            ("XDG_CONFIG_HOME", ""),
            ("HOME", "/h")
        ]))),
        p("/h/.config/crook/config.toml")
    );
}

#[test]
fn session_records_live_under_the_state_home() {
    let set = env_of(&[("XDG_STATE_HOME", "/s"), ("HOME", "/h")]);
    assert_eq!(ok(sessions_dir(&set)), p("/s/crook/sessions"));
    assert_eq!(
        ok(session_record(&set, "abc-123_X")),
        p("/s/crook/sessions/abc-123_X")
    );
    let unset = env_of(&[("HOME", "/h")]);
    assert_eq!(
        ok(sessions_dir(&unset)),
        p("/h/.local/state/crook/sessions")
    );
    let empty = env_of(&[("XDG_STATE_HOME", ""), ("HOME", "/h")]);
    assert_eq!(
        ok(sessions_dir(&empty)),
        p("/h/.local/state/crook/sessions")
    );
}

#[test]
fn a_session_id_cannot_escape_the_sessions_folder() {
    let env = env_of(&[("HOME", "/h")]);
    for bad in ["", "..", "a/b", "../x", "a b", "a.b"] {
        assert!(session_record(&env, bad).is_err(), "{bad:?}");
    }
}

#[test]
fn herdr_app_home_and_the_mod_folder() {
    let set = env_of(&[("HERDR_APP_HOME", "/app"), ("HOME", "/h")]);
    assert_eq!(ok(herdr_app_home(&set)), p("/app"));
    assert_eq!(ok(mod_install(&set)), p("/app/mods/crook"));
    assert_eq!(
        ok(mod_version_marker(&set)),
        p("/app/mods/crook/.crook-version")
    );
    let unset = env_of(&[("HOME", "/h")]);
    assert_eq!(ok(herdr_app_home(&unset)), p("/h/.config/herdr-app"));
    let empty = env_of(&[("HERDR_APP_HOME", ""), ("HOME", "/h")]);
    assert_eq!(ok(herdr_app_home(&empty)), p("/h/.config/herdr-app"));
}

#[test]
fn the_mise_shim_follows_mise_data_dir_then_xdg_data_home_then_home() {
    let all = env_of(&[
        ("MISE_DATA_DIR", "/m"),
        ("XDG_DATA_HOME", "/x"),
        ("HOME", "/h"),
    ]);
    assert_eq!(ok(mise_shim(&all)), p("/m/shims/crook"));
    let xdg = env_of(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")]);
    assert_eq!(ok(mise_shim(&xdg)), p("/x/mise/shims/crook"));
    let home = env_of(&[("HOME", "/h")]);
    assert_eq!(ok(mise_shim(&home)), p("/h/.local/share/mise/shims/crook"));
}

#[test]
fn the_mise_shim_ignores_empty_and_relative_values() {
    let env = env_of(&[
        ("MISE_DATA_DIR", "rel"),
        ("XDG_DATA_HOME", ""),
        ("HOME", "/h"),
    ]);
    assert_eq!(ok(mise_shim(&env)), p("/h/.local/share/mise/shims/crook"));
}

#[test]
fn the_call_path_is_crook_bin_else_the_shim_never_a_link_target() {
    let env = env_of(&[("HOME", "/h"), ("MISE_DATA_DIR", "/m")]);
    assert_eq!(
        crook_call_path(&env, Some("/dev/crook")).expect("path"),
        "/dev/crook"
    );
    assert_eq!(crook_call_path(&env, None).expect("path"), "/m/shims/crook");
    assert_eq!(
        crook_call_path(&env, Some("")).expect("path"),
        "/m/shims/crook"
    );
}

#[test]
fn a_call_path_no_rule_can_match_is_refused() {
    let env = env_of(&[("HOME", "/h")]);
    for bad in [
        "relative/crook",
        "/a\"b/crook",
        "/a$b/crook",
        "/a`b`/crook",
        "/a\\b/crook",
        "/a\nb/crook",
    ] {
        assert!(crook_call_path(&env, Some(bad)).is_err(), "{bad:?}");
    }
    let quoted_home = env_of(&[("HOME", "/h\"q")]);
    assert!(crook_call_path(&quoted_home, None).is_err());
}

/// Only that the resolver accepts it: whether Claude Code's matcher takes a space in `Bash("<path>" *)` was not
/// checked (spike 27 used a plain path). A rule that does not match costs a prompt, not a wrong call.
#[test]
fn a_call_path_with_spaces_is_accepted() {
    let env = env_of(&[("HOME", "/Users/a b")]);
    assert_eq!(
        crook_call_path(&env, None).expect("path"),
        "/Users/a b/.local/share/mise/shims/crook"
    );
}
