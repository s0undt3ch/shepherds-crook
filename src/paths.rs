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
fn home(env: Env) -> PathBuf {
    // A missing HOME resolves under `/`, so writes fail loudly instead of landing in the cwd.
    PathBuf::from(get(env, "HOME").unwrap_or_else(|| "/".to_string()))
}

fn data_home(env: Env) -> PathBuf {
    match get(env, "XDG_DATA_HOME") {
        Some(x) => PathBuf::from(x),
        None => home(env).join(".local/share"),
    }
}

pub fn data_dir(env: Env) -> PathBuf {
    data_home(env).join("crook")
}

pub fn project_dir(env: Env, slug: &str) -> Result<PathBuf, Error> {
    valid_slug(slug)?;
    Ok(data_dir(env).join(slug))
}

pub fn plugin_dir(env: Env) -> PathBuf {
    data_home(env).join("crook-plugin").join(format!(
        "{}-{}",
        env!("CARGO_PKG_VERSION"),
        crate::launch::plugin_dir::content_hash()
    ))
}

pub fn config_file(env: Env) -> PathBuf {
    home(env).join(".config/crook/config.toml")
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
    fn data_dir_xdg() {
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/x")], data_dir),
            Path::new("/x/crook")
        );
        assert_eq!(
            with(&[("HOME", "/h")], data_dir),
            Path::new("/h/.local/share/crook")
        );
        // Empty counts as unset.
        assert_eq!(
            with(&[("XDG_DATA_HOME", ""), ("HOME", "/h")], data_dir),
            Path::new("/h/.local/share/crook")
        );
    }

    #[test]
    fn project_dir_validates_slug() {
        let p = with(&[("XDG_DATA_HOME", "/x")], |e| project_dir(e, "foo")).unwrap();
        assert_eq!(p, Path::new("/x/crook/foo"));
        assert!(with(&[("XDG_DATA_HOME", "/x")], |e| project_dir(e, "../etc")).is_err());
    }

    #[test]
    fn plugin_dir_has_version_and_hash() {
        let p = with(&[("XDG_DATA_HOME", "/x")], plugin_dir);
        assert_eq!(p.parent(), Some(Path::new("/x/crook-plugin")));
        let name = p.file_name().unwrap().to_str().unwrap();
        let prefix = format!("{}-", env!("CARGO_PKG_VERSION"));
        let hash = name.strip_prefix(&prefix).expect("version prefix");
        assert_eq!(hash.len(), 12);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn config_file_is_under_home() {
        assert_eq!(
            with(&[("HOME", "/h")], config_file),
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
