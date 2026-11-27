//! `config.toml` (ADR 0026). Every command reads its settings through this module only.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Env, Error, paths};

#[cfg(test)]
mod tests;

pub const DEFAULT_CHILD_MODEL: &str = "sonnet";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Where the human-facing notes go, always absolute (ADR 0004).
    pub notes_root: Option<PathBuf>,
    /// The model a child gets when `spawn` is given none (Q9).
    pub default_child_model: String,
    /// `None` means the maintainer's default model (Q32).
    pub coordinator_model: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            notes_root: None,
            default_child_model: DEFAULT_CHILD_MODEL.to_string(),
            coordinator_model: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    notes_root: Option<String>,
    default_child_model: Option<String>,
    coordinator_model: Option<String>,
}

/// Reads the config file. A missing file means every default.
pub fn load(env: Env) -> Result<Config, Error> {
    let path = paths::config_file(env)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text, &path, paths::home(env).ok().as_deref()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(Error::File { path, source }),
    }
}

/// `path` only labels errors; `home` expands a leading `~/`, and is an error to need when it is `None`.
pub fn parse(text: &str, path: &Path, home: Option<&Path>) -> Result<Config, Error> {
    let err = |message: String| Error::Config {
        path: path.to_path_buf(),
        message,
    };
    let raw: Raw = toml::from_str(text).map_err(|e| err(e.to_string()))?;
    // An empty string reads as unset, so a key can be blanked without deleting it.
    let non_empty = |v: Option<String>| v.filter(|s| !s.is_empty());
    let notes_root = match non_empty(raw.notes_root) {
        None => None,
        Some(value) => {
            let expanded =
                match value.strip_prefix("~/") {
                    Some(rest) => match home {
                        Some(home) => home.join(rest),
                        None => return Err(err(
                            "notes_root starts with `~/`, but HOME is not set to an absolute path"
                                .to_string(),
                        )),
                    },
                    None => PathBuf::from(&value),
                };
            if !expanded.is_absolute() {
                return Err(err(format!(
                    "notes_root must be an absolute path or start with `~/`, not {value:?}"
                )));
            }
            Some(expanded)
        }
    };
    Ok(Config {
        notes_root,
        default_child_model: non_empty(raw.default_child_model)
            .unwrap_or_else(|| DEFAULT_CHILD_MODEL.to_string()),
        coordinator_model: non_empty(raw.coordinator_model),
    })
}
