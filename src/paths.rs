//! Every path crook derives from the environment: one function per row of the paths table in the
//! binary plan. Built by hand, not with a platform crate, because those point at
//! `~/Library/Application Support` on macOS (ADR 0026).
//!
//! As the XDG spec says, an empty or relative value counts as unset. `HOME` is the exception: it has
//! no fallback, so a missing or relative one is an error, and only for a path that needs it.

use std::path::{Path, PathBuf};

use crate::{Env, Error};

#[cfg(test)]
mod tests;

/// The shell form of the shim path that hooks and prompts use (ADR 0019, spike 22): `CROOK_BIN` when
/// set, else the mise shim. It mirrors [`mise_shim`].
pub const SHIM_SHELL_EXPR: &str =
    "\"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}\"";

/// A non-empty, absolute value only.
fn get_abs(env: Env, key: &str) -> Option<PathBuf> {
    env(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

pub fn home(env: Env) -> Result<PathBuf, Error> {
    get_abs(env, "HOME").ok_or_else(|| {
        Error::Env("HOME is not set to an absolute path, and crook needs it here".to_string())
    })
}

fn xdg(env: Env, var: &str, fallback: &str) -> Result<PathBuf, Error> {
    match get_abs(env, var) {
        Some(x) => Ok(x),
        None => Ok(home(env)?.join(fallback)),
    }
}

/// `${XDG_DATA_HOME:-~/.local/share}` (ADR 0004).
pub fn data_home(env: Env) -> Result<PathBuf, Error> {
    xdg(env, "XDG_DATA_HOME", ".local/share")
}

/// `<data home>/crook/flocks/` (ADR 0004).
pub fn flocks_root(env: Env) -> Result<PathBuf, Error> {
    Ok(data_home(env)?.join("crook/flocks"))
}

/// `<flocks root>/<yyyy-mm-dd>-<slug>/`; `dir_name` comes from `names::folder_name`.
pub fn flock_folder(env: Env, dir_name: &str) -> Result<PathBuf, Error> {
    Ok(flocks_root(env)?.join(dir_name))
}

/// The notes folder: `<notes_root>/<dir name>/` when `notes_root` is set, else the flock folder itself
/// (ADR 0004, 0012, Q18).
pub fn notes_folder(flock_folder: &Path, dir_name: &str, notes_root: Option<&Path>) -> PathBuf {
    match notes_root {
        Some(root) => root.join(dir_name),
        None => flock_folder.to_path_buf(),
    }
}

/// `${XDG_CONFIG_HOME:-~/.config}/crook/config.toml` (ADR 0026).
pub fn config_file(env: Env) -> Result<PathBuf, Error> {
    Ok(xdg(env, "XDG_CONFIG_HOME", ".config")?.join("crook/config.toml"))
}

/// `<data home>/crook/current`, pointing at the install folder (ADR 0019, Q16).
pub fn current_link(env: Env) -> Result<PathBuf, Error> {
    Ok(data_home(env)?.join("crook/current"))
}

/// `${XDG_STATE_HOME:-~/.local/state}/crook/sessions/` (Q17).
pub fn sessions_dir(env: Env) -> Result<PathBuf, Error> {
    Ok(xdg(env, "XDG_STATE_HOME", ".local/state")?.join("crook/sessions"))
}

/// The version record of one session (Q17). The id comes from Claude Code, so it is checked before it
/// becomes a file name.
pub fn session_record(env: Env, session_id: &str) -> Result<PathBuf, Error> {
    let ok = !session_id.is_empty()
        && session_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !ok {
        return Err(Error::InvalidName {
            kind: "session id",
            name: session_id.to_string(),
            reason: "only letters, digits, `-` and `_` are allowed",
        });
    }
    Ok(sessions_dir(env)?.join(session_id))
}

/// `${HERDR_APP_HOME:-~/.config/herdr-app}` (spike 17).
pub fn herdr_app_home(env: Env) -> Result<PathBuf, Error> {
    match get_abs(env, "HERDR_APP_HOME") {
        Some(x) => Ok(x),
        None => Ok(home(env)?.join(".config/herdr-app")),
    }
}

/// `<herdr-app home>/mods/crook/` (ADR 0019).
pub fn mod_install(env: Env) -> Result<PathBuf, Error> {
    Ok(herdr_app_home(env)?.join("mods/crook"))
}

/// `<mod install>/.crook-version`, written last when the mod is installed (ADR 0019).
pub fn mod_version_marker(env: Env) -> Result<PathBuf, Error> {
    Ok(mod_install(env)?.join(".crook-version"))
}

/// The mise shim: `${MISE_DATA_DIR:-${XDG_DATA_HOME:-~/.local/share}/mise}/shims/crook` (ADR 0019).
/// That is where mise itself puts its shims.
pub fn mise_shim(env: Env) -> Result<PathBuf, Error> {
    let mise_data = match get_abs(env, "MISE_DATA_DIR") {
        Some(x) => x,
        None => data_home(env)?.join("mise"),
    };
    Ok(mise_data.join("shims/crook"))
}
