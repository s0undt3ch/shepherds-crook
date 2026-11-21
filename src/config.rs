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
}

#[cfg(test)]
mod home_tests {
    use std::path::Path;

    use super::*;

    fn cfg(home: Option<&str>, diary_root: Option<&str>) -> Config {
        Config {
            home: home.map(str::to_string),
            diary_root: diary_root.map(str::to_string),
            ..Config::default()
        }
    }

    fn with<R>(pairs: &[(&str, &str)], f: impl FnOnce(Env) -> R) -> R {
        let m: std::collections::HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        f(&move |k: &str| m.get(k).cloned())
    }

    #[test]
    fn home_config_reads_home_and_diary_root() {
        let t = tempfile::tempdir().unwrap();
        let f =
            paths::config_file(&|k: &str| (k == "HOME").then(|| t.path().display().to_string()));
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "home = \"/h\"\ndiary_root = \"/v\"\n").unwrap();
        let c = load(&|k: &str| (k == "HOME").then(|| t.path().display().to_string())).unwrap();
        assert_eq!(c.home.as_deref(), Some("/h"));
        assert_eq!(c.diary_root.as_deref(), Some("/v"));
    }

    #[test]
    fn home_config_default_follows_xdg_data_home() {
        let c = cfg(None, None);
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/d")], |e| c.project_home(e, "demo")).unwrap(),
            Path::new("/d/crook/demo")
        );
        assert_eq!(
            with(&[("HOME", "/h")], |e| c.project_home(e, "demo")).unwrap(),
            Path::new("/h/.local/share/crook/demo")
        );
        // An empty variable counts as unset.
        assert_eq!(
            with(&[("XDG_DATA_HOME", ""), ("HOME", "/h")], |e| c
                .project_home(e, "demo"))
            .unwrap(),
            Path::new("/h/.local/share/crook/demo")
        );
    }

    #[test]
    fn home_config_override_and_tilde() {
        let c = cfg(Some("~/projects-home"), None);
        assert_eq!(
            with(&[("HOME", "/h")], |e| c.project_home(e, "demo")).unwrap(),
            Path::new("/h/projects-home/demo")
        );
        let c = cfg(Some("/x/./y/../home/"), None);
        assert_eq!(
            with(&[], |e| c.project_home(e, "demo")).unwrap(),
            Path::new("/x/home/demo")
        );
        for bad in ["rel/dir", "", "~other/x"] {
            let c = cfg(Some(bad), None);
            assert!(
                with(&[("HOME", "/h")], |e| c.home_root(e)).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn home_config_inside_the_state_dir_is_refused() {
        for home in ["/s/crook", "/s/crook/demo", "/s/crook/a/../b"] {
            let c = cfg(Some(home), None);
            let e = with(&[("XDG_STATE_HOME", "/s")], |e| c.home_root(e)).unwrap_err();
            assert!(e.to_string().contains("state dir"), "{home}: {e}");
        }
        // The default home sits beside the state dir, not inside it.
        let c = cfg(None, None);
        assert!(with(&[("HOME", "/h")], |e| c.home_root(e)).is_ok());
        // Through a symlink too.
        let t = tempfile::tempdir().unwrap();
        let state = t.path().join("state");
        std::fs::create_dir_all(state.join("crook")).unwrap();
        std::os::unix::fs::symlink(state.join("crook"), t.path().join("link")).unwrap();
        let c = cfg(Some(&t.path().join("link/h").display().to_string()), None);
        let s = state.display().to_string();
        assert!(with(&[("XDG_STATE_HOME", &s)], |e| c.home_root(e)).is_err());
    }

    #[test]
    fn diary_location() {
        let c = cfg(None, None);
        assert_eq!(
            with(&[("XDG_DATA_HOME", "/d")], |e| c.diary(e, "demo")).unwrap(),
            Path::new("/d/crook/demo/diary")
        );
        let c = cfg(None, Some("~/vault/projects"));
        assert_eq!(
            with(&[("HOME", "/h")], |e| c.diary(e, "demo")).unwrap(),
            Path::new("/h/vault/projects/demo")
        );
    }

    #[test]
    fn diaries_glob_follows_the_config_and_never_fails() {
        let g = |c: &Config| with(&[("XDG_DATA_HOME", "/d")], |e| c.diaries_glob(e));
        assert_eq!(g(&cfg(None, None)).as_deref(), Some("/d/crook/*/diary"));
        assert_eq!(g(&cfg(Some("/h"), None)).as_deref(), Some("/h/*/diary"));
        assert_eq!(g(&cfg(None, Some("/v"))).as_deref(), Some("/v/*"));
        assert_eq!(g(&cfg(Some("rel"), None)), None);
    }
}
