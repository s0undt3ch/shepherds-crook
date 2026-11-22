//! The user's config, `$XDG_CONFIG_HOME/crook/config.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Env, Error, paths, privacy, project};

/// Reference lists that apply to every project, and where projects live.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub private: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    /// Parent of every project home; default `${XDG_DATA_HOME:-~/.local/share}/crook`.
    pub home: Option<String>,
    /// When set, a project's diary is `<diary_root>/<slug>` instead of `<home>/<slug>/diary`.
    pub diary_root: Option<String>,
    #[serde(default)]
    pub herdr: HerdrConfig,
}

/// The tab label of an orchestrator: a dog, one code point.
pub const ORCHESTRATOR_LABEL: &str = "\u{1F415}";

/// Plain text for a tab bar that cannot draw `ORCHESTRATOR_LABEL`: the only
/// numeronym crook uses. Set it as `[herdr] orchestrator_label = "o10r"`.
pub const ORCHESTRATOR_LABEL_TEXT: &str = "o10r";

/// `[herdr]` in config.toml.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HerdrConfig {
    /// The label of the orchestrator's herdr tab; blank counts as unset.
    pub orchestrator_label: Option<String>,
}

/// `~` and `~/x` from the home directory; anything else as written.
fn expand(env: Env, text: &str) -> String {
    match text.strip_prefix('~') {
        Some("") => paths::home(env).display().to_string(),
        Some(rest) if rest.starts_with('/') => {
            format!("{}{rest}", paths::home(env).display())
        }
        _ => text.to_string(),
    }
}

fn absolute(env: Env, key: &str, text: &str) -> Result<PathBuf, Error> {
    let abs = expand(env, text);
    if text.trim().is_empty() || !abs.starts_with('/') {
        return Err(Error::Refused(format!(
            "config `{key}` must be an absolute path (or start with ~/): {text:?}"
        )));
    }
    project::clean_path(&abs).map(PathBuf::from)
}

impl Config {
    /// The directory that holds every project home. Refuses a relative value
    /// and one inside crook's state dir, whose tree sessions cannot touch.
    pub fn home_root(&self, env: Env) -> Result<PathBuf, Error> {
        let root = match &self.home {
            Some(h) => absolute(env, "home", h)?,
            None => paths::data_home(env).join("crook"),
        };
        refuse_inside_state(env, "home", &root)?;
        Ok(root)
    }

    /// `<home>/<slug>`: the orchestrator's cwd.
    pub fn project_home(&self, env: Env, slug: &str) -> Result<PathBuf, Error> {
        paths::valid_slug(slug)?;
        Ok(self.home_root(env)?.join(slug))
    }

    /// `<diary_root>/<slug>` when `diary_root` is set, else `<home>/<slug>/diary`.
    pub fn diary(&self, env: Env, slug: &str) -> Result<PathBuf, Error> {
        paths::valid_slug(slug)?;
        match &self.diary_root {
            Some(d) => Ok(absolute(env, "diary_root", d)?.join(slug)),
            None => Ok(self.project_home(env, slug)?.join("diary")),
        }
    }

    /// The label for an orchestrator's herdr tab.
    pub fn orchestrator_label(&self) -> &str {
        self.herdr
            .orchestrator_label
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(ORCHESTRATOR_LABEL)
    }

    /// The glob that makes every project's diary private, whichever project
    /// is open. Lenient: a bad `home` or `diary_root` gives `None` here, since
    /// `open` and `init` refuse it with a message and the hook must not fail.
    pub fn diaries_glob(&self, env: Env) -> Option<String> {
        let glob = match &self.diary_root {
            Some(d) => format!("{}/*", absolute(env, "diary_root", d).ok()?.display()),
            None => {
                let home = match &self.home {
                    Some(h) => absolute(env, "home", h).ok()?,
                    None => paths::data_home(env).join("crook"),
                };
                format!("{}/*/diary", home.display())
            }
        };
        Some(glob)
    }
}

/// Compares lexically and as the kernel resolves each path, so a symlink into
/// the state dir does not get past.
fn refuse_inside_state(env: Env, key: &str, p: &Path) -> Result<(), Error> {
    let state = paths::state_dir(env);
    let inside = |a: &Path, b: &Path| a.starts_with(b);
    if inside(p, &state) || inside(&privacy::resolve_path(p), &privacy::resolve_path(&state)) {
        return Err(Error::Refused(format!(
            "config `{key}` {} is inside crook's state dir {}, which sessions cannot read",
            p.display(),
            state.display()
        )));
    }
    Ok(())
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
mod tests;
