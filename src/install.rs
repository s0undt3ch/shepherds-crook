//! Where crook's own files are: the plugin and the mod next to the executable, their version, the
//! forward-only `current` link and `CROOK_BIN` (ADR 0019, spikes 18 and 23).

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::{Env, Error, paths};

#[cfg(test)]
mod tests;

/// A debug build (`cargo build`, `cargo run`) has no `plugin/` next to it and never moves the link.
pub const DEBUG_BUILD: bool = cfg!(debug_assertions);

/// The folder holding `crook`, `plugin/` and `mod/`: a mise install folder, or the repository for
/// a debug build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    root: PathBuf,
}

impl Install {
    pub(crate) fn at(root: PathBuf) -> Install {
        Install { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The only way other modules find the plugin folder.
    pub fn plugin_dir(&self) -> PathBuf {
        self.root.join("plugin")
    }

    /// The only way other modules find the mod folder.
    pub fn mod_dir(&self) -> PathBuf {
        self.root.join("mod")
    }

    pub fn version(&self) -> Result<String, Error> {
        plugin_version(&self.root)
    }
}

/// `exe` is already canonical. A folder is an install when `plugin/` is next to the executable.
pub fn locate(exe: &Path, debug: bool, manifest_dir: &Path) -> Result<Install, Error> {
    let folder = exe.parent().unwrap_or(exe);
    if folder.join("plugin").is_dir() {
        return Ok(Install::at(folder.to_path_buf()));
    }
    if debug {
        return Ok(Install::at(manifest_dir.to_path_buf()));
    }
    Err(Error::Install(format!(
        "the install is broken: there is no plugin/ folder next to the crook executable (looked in {})",
        folder.display()
    )))
}

/// Finds the install of the running executable, following links, so a mise shim or a linked
/// `crook` still lands in the real install folder.
pub fn find(
    current_exe: &dyn Fn() -> std::io::Result<PathBuf>,
    debug: bool,
    manifest_dir: &Path,
) -> Result<Install, Error> {
    let exe = current_exe().map_err(|source| Error::Io {
        program: "current_exe".to_string(),
        source,
    })?;
    let exe = exe.canonicalize().map_err(|source| Error::File {
        path: exe.clone(),
        source,
    })?;
    locate(&exe, debug, manifest_dir)
}

/// The install of the running executable, with this build's own flags.
pub fn find_own(ctx: &crate::Ctx) -> Result<Install, Error> {
    find(
        ctx.current_exe,
        DEBUG_BUILD,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

/// The `version` field of `<folder>/plugin/.claude-plugin/plugin.json`. The release stamps it; in
/// the repository it reads `0.0.0-dev` (Q17).
pub fn plugin_version(folder: &Path) -> Result<String, Error> {
    plugin_dir_version(&folder.join("plugin"))
}

/// As [`plugin_version`], for the plugin folder itself: `$CLAUDE_PLUGIN_ROOT`, or `current/plugin`.
pub fn plugin_dir_version(plugin_dir: &Path) -> Result<String, Error> {
    let path = plugin_dir.join(".claude-plugin/plugin.json");
    let bad = |message: &str| Error::Install(format!("{}: {message}", path.display()));
    let text = std::fs::read_to_string(&path).map_err(|source| Error::File {
        path: path.clone(),
        source,
    })?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| bad(&format!("not valid JSON: {e}")))?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad("no string field `version`"))
}

/// A parsed `MAJOR.MINOR.PATCH[-PRE][+BUILD]`. Build metadata is dropped: it takes no part in
/// precedence.
struct Semver<'a> {
    core: (u64, u64, u64),
    pre: Vec<&'a str>,
}

fn numeric(part: &str) -> bool {
    !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
}

fn semver(version: &str) -> Option<Semver<'_>> {
    let (rest, build) = match version.split_once('+') {
        Some((rest, build)) => (rest, Some(build)),
        None => (version, None),
    };
    let ident =
        |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if build.is_some_and(|b| !b.split('.').all(ident)) {
        return None;
    }
    let (core, pre) = match rest.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (rest, None),
    };
    let mut parts = core.split('.');
    let mut next = || {
        let part = parts.next()?;
        numeric(part).then(|| part.parse::<u64>().ok()).flatten()
    };
    let core_nums = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return None;
    }
    let pre: Vec<&str> = pre.map_or_else(Vec::new, |p| p.split('.').collect());
    let pre_ok = pre
        .iter()
        .all(|p| ident(p) && (!numeric(p) || p.len() == 1 || !p.starts_with('0')));
    pre_ok.then_some(Semver {
        core: core_nums,
        pre,
    })
}

fn compare_identifiers(a: &str, b: &str) -> Ordering {
    match (numeric(a), numeric(b)) {
        // Numbers too long for u64 still compare by length, then digit by digit.
        (true, true) => (a.len(), a).cmp(&(b.len(), b)),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

/// Semantic Versioning precedence: a release outranks its pre-releases, pre-release identifiers
/// compare dot by dot (numbers below words, numbers by value, a shorter list lower when equal so
/// far), build metadata is ignored. `None` when either side is not a semantic version.
pub fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let (a, b) = (semver(a)?, semver(b)?);
    Some(
        a.core
            .cmp(&b.core)
            .then_with(|| match (a.pre.is_empty(), b.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    let by_part = a
                        .pre
                        .iter()
                        .zip(&b.pre)
                        .map(|(x, y)| compare_identifiers(x, y))
                        .find(|o| o.is_ne());
                    by_part.unwrap_or_else(|| a.pre.len().cmp(&b.pre.len()))
                }
            }),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkOutcome {
    Created,
    Moved,
    Unchanged,
    DebugBuild,
    /// Something that is not a link sits at the path; the caller warns.
    LeftAlone(String),
}

/// Points `<data home>/crook/current` at `install`, but only forward (ADR 0019, Q16): when the link
/// is missing, dangling, or names a lower version. A target of unknown version stays.
pub fn point_current(env: Env, install: &Install, debug: bool) -> Result<LinkOutcome, Error> {
    if debug {
        return Ok(LinkOutcome::DebugBuild);
    }
    let link = paths::current_link(env)?;
    let file_err = |source| Error::File {
        path: link.clone(),
        source,
    };
    // Two crook processes of different versions can start together; the lock makes the read,
    // the version comparison and the rename one step, so the older cannot win after the newer.
    let parent = link.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(parent).map_err(file_err)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(parent.join("current.lock"))
        .map_err(file_err)?;
    lock.lock().map_err(file_err)?;
    let created = match std::fs::symlink_metadata(&link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => return Err(file_err(e)),
        Ok(meta) if !meta.file_type().is_symlink() => {
            return Ok(LinkOutcome::LeftAlone(format!(
                "{} is not a link, so crook leaves it alone",
                link.display()
            )));
        }
        Ok(_) => {
            let raw = std::fs::read_link(&link).map_err(file_err)?;
            let target = link.parent().unwrap_or(Path::new("/")).join(raw);
            if target.exists() {
                if target.canonicalize().ok().as_deref() == Some(install.root()) {
                    return Ok(LinkOutcome::Unchanged);
                }
                let older = match (plugin_version(&target), install.version()) {
                    (Ok(theirs), Ok(mine)) => {
                        compare_versions(&mine, &theirs) == Some(Ordering::Greater)
                    }
                    _ => false,
                };
                if !older {
                    return Ok(LinkOutcome::Unchanged);
                }
            }
            false
        }
    };
    swap(&link, install.root())?;
    Ok(if created {
        LinkOutcome::Created
    } else {
        LinkOutcome::Moved
    })
}

/// Makes a temporary link next to `link` and renames it over, so a reader never sees it missing.
fn swap(link: &Path, target: &Path) -> Result<(), Error> {
    let file_err = |source| Error::File {
        path: link.to_path_buf(),
        source,
    };
    let parent = link.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(parent).map_err(file_err)?;
    let tmp = parent.join(format!(".current.{}.tmp", std::process::id()));
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(file_err(e)),
    }
    std::os::unix::fs::symlink(target, &tmp).map_err(file_err)?;
    std::fs::rename(&tmp, link).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        file_err(e)
    })
}

/// The once-per-run link update. Never fails: the problems come back as `warning:` lines for
/// `main` to print.
pub fn refresh_with(
    env: Env,
    current_exe: &dyn Fn() -> std::io::Result<PathBuf>,
    debug: bool,
    manifest_dir: &Path,
) -> Vec<String> {
    if debug {
        return Vec::new();
    }
    let outcome = find(current_exe, debug, manifest_dir)
        .and_then(|install| point_current(env, &install, debug));
    match outcome {
        Ok(LinkOutcome::LeftAlone(why)) => vec![format!("warning: {why}")],
        Ok(_) => Vec::new(),
        Err(e) => vec![format!("warning: could not update the current link: {e}")],
    }
}

pub fn refresh(ctx: &crate::Ctx) -> Vec<String> {
    refresh_with(
        ctx.env,
        ctx.current_exe,
        DEBUG_BUILD,
        Path::new(env!("CARGO_MANIFEST_DIR")),
    )
}

/// The value to pass on as `CROOK_BIN` (ADR 0019, spike 23): the variable when set and not empty
/// (the shell form treats an empty one as unset); in a debug build, its own executable. A relative
/// `CROOK_BIN` is refused: it would mean a different file in every terminal's working folder.
pub fn crook_bin(
    env: Env,
    current_exe: &dyn Fn() -> std::io::Result<PathBuf>,
    debug: bool,
) -> Result<Option<String>, Error> {
    if let Some(set) = env("CROOK_BIN").filter(|v| !v.is_empty()) {
        if !Path::new(&set).is_absolute() {
            return Err(Error::Env(format!(
                "CROOK_BIN must be an absolute path, not {set:?}"
            )));
        }
        return Ok(Some(set));
    }
    if debug {
        return Ok(current_exe().ok().map(|p| p.display().to_string()));
    }
    Ok(None)
}
