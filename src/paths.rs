use std::path::PathBuf;

use crate::{Env, Error};

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
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;

    fn with<R>(pairs: &[(&str, &str)], f: impl FnOnce(Env) -> R) -> R {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        f(&move |k: &str| m.get(k).cloned())
    }

    #[test]
    fn valid_slug_accepts_and_rejects() {
        for ok in ["foo", "a-1"] {
            assert!(valid_slug(ok).is_ok(), "{ok}");
        }
        for bad in ["Foo", "a/b", "..", ""] {
            assert!(valid_slug(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn state_dir_xdg() {
        assert_eq!(
            with(&[("XDG_STATE_HOME", "/x")], state_dir),
            Path::new("/x/crook")
        );
        assert_eq!(
            with(&[("HOME", "/h")], state_dir),
            Path::new("/h/.local/state/crook")
        );
        // Empty counts as unset.
        assert_eq!(
            with(&[("XDG_STATE_HOME", ""), ("HOME", "/h")], state_dir),
            Path::new("/h/.local/state/crook")
        );
        // The data home does not move the state dir.
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/d"), ("HOME", "/h")], state_dir),
            Path::new("/h/.local/state/crook")
        );
    }

    #[test]
    fn project_dir_validates_slug() {
        let p = with(&[("XDG_STATE_HOME", "/x")], |e| project_dir(e, "foo")).unwrap();
        assert_eq!(p, Path::new("/x/crook/foo"));
        assert!(with(&[("XDG_STATE_HOME", "/x")], |e| project_dir(e, "../etc")).is_err());
        let p = with(&[("XDG_DATA_HOME", "/x"), ("HOME", "/h")], |e| {
            project_dir(e, "foo")
        })
        .unwrap();
        assert_eq!(p, Path::new("/h/.local/state/crook/foo"));
    }

    #[test]
    fn plugin_dir_has_version_and_hash() {
        let p = with(&[("XDG_DATA_HOME", "/x")], plugin_dir);
        assert_eq!(p.parent(), Some(Path::new("/x/crook-plugin")));
        let moved = with(
            &[("XDG_DATA_HOME", "/x"), ("XDG_STATE_HOME", "/s")],
            plugin_dir,
        );
        assert_eq!(moved, p);
        let name = p.file_name().unwrap().to_str().unwrap();
        let prefix = format!("{}-", env!("CARGO_PKG_VERSION"));
        let hash = name.strip_prefix(&prefix).expect("version prefix");
        assert_eq!(hash.len(), 12);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn config_file_xdg_or_home() {
        assert_eq!(
            with(&[("XDG_CONFIG_HOME", "/c")], config_file),
            Path::new("/c/crook/config.toml")
        );
        assert_eq!(
            with(&[("HOME", "/h")], config_file),
            Path::new("/h/.config/crook/config.toml")
        );
        // Empty counts as unset.
        assert_eq!(
            with(&[("XDG_CONFIG_HOME", ""), ("HOME", "/h")], config_file),
            Path::new("/h/.config/crook/config.toml")
        );
    }

    #[test]
    fn claude_config_dir_env_or_home() {
        assert_eq!(
            with(&[("CLAUDE_CONFIG_DIR", "/c")], claude_config_dir),
            Path::new("/c")
        );
        assert_eq!(
            with(&[("HOME", "/h")], claude_config_dir),
            Path::new("/h/.claude")
        );
    }
}
