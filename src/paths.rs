use std::path::{Path, PathBuf};

use crate::{Env, Error};

/// The file crook writes in each project home it makes, naming the project:
/// `wipe` touches only a home that carries it, and `init` adopts only such a home.
pub const HOME_MARKER: &str = ".crook";

fn marker_text(slug: &str) -> String {
    format!("crook project home: {slug}\n")
}

/// Whether `home` is the home crook made for `slug`.
pub fn is_crook_home(home: &Path, slug: &str) -> bool {
    std::fs::read_to_string(home.join(HOME_MARKER)).is_ok_and(|t| t == marker_text(slug))
}

/// Writes the marker that makes `home` crook's home for `slug`.
pub fn mark_home(home: &Path, slug: &str) -> Result<(), Error> {
    std::fs::write(home.join(HOME_MARKER), marker_text(slug))?;
    Ok(())
}

/// Makes `home` for `slug`, with its marker, when nothing is there yet. An
/// existing directory is left as it is, so it never becomes crook's to wipe.
pub fn make_home(home: &Path, slug: &str) -> Result<(), Error> {
    if std::fs::symlink_metadata(home).is_ok() {
        return Ok(());
    }
    std::fs::create_dir_all(home)?;
    mark_home(home, slug)
}

pub fn valid_slug(s: &str) -> Result<(), Error> {
    let ok = !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidSlug(s.to_string()))
    }
}

/// Non-empty value only: an empty variable counts as unset, as in the XDG spec.
fn get(env: Env, key: &str) -> Option<String> {
    env(key).filter(|v| !v.is_empty())
}

/// Built by hand: the `dirs` crate gives `~/Library/Application Support` on macOS.
pub(crate) fn home(env: Env) -> PathBuf {
    // A missing HOME resolves under `/`, so writes fail loudly instead of landing in the cwd.
    PathBuf::from(get(env, "HOME").unwrap_or_else(|| "/".to_string()))
}

pub(crate) fn data_home(env: Env) -> PathBuf {
    match get(env, "XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => home(env).join(".local/share"),
    }
}

/// `${XDG_STATE_HOME:-~/.local/state}/crook`: crook's private per-project state.
pub fn state_dir(env: Env) -> PathBuf {
    let base = match get(env, "XDG_STATE_HOME") {
        Some(x) => PathBuf::from(x),
        None => home(env).join(".local/state"),
    };
    base.join("crook")
}

pub fn project_dir(env: Env, slug: &str) -> Result<PathBuf, Error> {
    valid_slug(slug)?;
    Ok(state_dir(env).join(slug))
}

pub fn plugin_dir(env: Env) -> PathBuf {
    data_home(env).join("crook-plugin").join(format!(
        "{}-{}",
        env!("CARGO_PKG_VERSION"),
        crate::launch::plugin_dir::content_hash()
    ))
}

pub fn config_file(env: Env) -> PathBuf {
    let base = match get(env, "XDG_CONFIG_HOME") {
        Some(x) => PathBuf::from(x),
        None => home(env).join(".config"),
    };
    base.join("crook/config.toml")
}

pub fn claude_config_dir(env: Env) -> PathBuf {
    match get(env, "CLAUDE_CONFIG_DIR") {
        Some(c) => PathBuf::from(c),
        None => home(env).join(".claude"),
    }
}

#[cfg(test)]
mod tests;
