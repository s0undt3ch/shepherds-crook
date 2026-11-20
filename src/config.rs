//! The user's config, `$XDG_CONFIG_HOME/crook/config.toml`.

use serde::Deserialize;

use crate::{Env, Error, paths};

/// Reference lists that apply to every project.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub private: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
}

/// A missing file gives an empty config; any other read or parse failure is an error.
pub fn load(env: Env) -> Result<Config, Error> {
    let path = paths::config_file(env);
    let fail = |msg: String| Error::Config {
        path: path.display().to_string(),
        msg,
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(e) => return Err(fail(e.to_string())),
    };
    toml::from_str(&text).map_err(|e| fail(e.to_string()))
}

#[cfg(test)]
mod tests {
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
}
