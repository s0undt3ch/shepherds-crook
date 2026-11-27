//! Flock folders: create, find and list them, and resolve the notes folder (ADR 0004, 0012; Q2, Q18).

use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::files::atomic::LOCK_FILE;
use crate::files::roster;
use crate::{Env, Error, names, paths};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flock {
    pub slug: String,
    /// `yyyy-mm-dd`, as in the folder name.
    pub date: String,
    pub folder: PathBuf,
}

impl Flock {
    /// The folder's own name, `<yyyy-mm-dd>-<slug>`.
    pub fn dir_name(&self) -> String {
        names::folder_name(&self.date, &self.slug)
    }

    pub fn briefs_dir(&self) -> PathBuf {
        self.folder.join("briefs")
    }

    /// `relative` is a roster `brief` field, `briefs/<child>.md`.
    pub fn brief_path(&self, relative: &str) -> PathBuf {
        self.folder.join(relative)
    }

    /// `results/<child>.md` (ADR 0024).
    pub fn result_path(&self, child: &str) -> PathBuf {
        self.folder.join("results").join(format!("{child}.md"))
    }

    /// The folder for the coordinator's `README.md` and `notes/` (ADR 0012).
    pub fn notes_folder(&self, config: &Config) -> PathBuf {
        paths::notes_folder(&self.folder, &self.dir_name(), config.notes_root.as_deref())
    }
}

/// Whether `s` is shaped `yyyy-mm-dd`.
pub fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Splits `<yyyy-mm-dd>-<slug>`. Matching the date exactly keeps slug `b` apart from slug `a-b`.
fn split_dir_name(name: &str) -> Option<(&str, &str)> {
    let date = name.get(..10)?;
    let slug = name.get(11..)?;
    (is_date(date) && name.as_bytes()[10] == b'-' && names::validate_slug(slug).is_ok())
        .then_some((date, slug))
}

/// Every flock folder, closed ones included, in name order. A missing flocks root is no flocks.
pub fn list(env: Env) -> Result<Vec<Flock>, Error> {
    let root = paths::flocks_root(env)?;
    let entries = match std::fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(Error::File { path: root, source }),
    };
    let mut flocks = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| Error::File {
            path: root.clone(),
            source,
        })?;
        let name = entry.file_name();
        let Some((date, slug)) = name.to_str().and_then(split_dir_name) else {
            continue;
        };
        if entry.path().is_dir() {
            flocks.push(Flock {
                slug: slug.to_string(),
                date: date.to_string(),
                folder: entry.path(),
            });
        }
    }
    flocks.sort_by(|a, b| a.folder.cmp(&b.folder));
    Ok(flocks)
}

fn with_slug(env: Env, slug: &str) -> Result<Vec<Flock>, Error> {
    names::validate_slug(slug)?;
    Ok(list(env)?.into_iter().filter(|f| f.slug == slug).collect())
}

fn paths_of(flocks: &[Flock]) -> String {
    flocks
        .iter()
        .map(|f| f.folder.display().to_string())
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The one flock with this slug. None is an error, and so are two (one slug, two dates).
///
/// A folder whose `roster.json` is missing is reported as a broken flock, and so is one whose roster
/// names another flock; neither is accepted silently. Only `new` makes a folder without a roster, and
/// only for the moment before it writes one.
pub fn find(env: Env, slug: &str) -> Result<Flock, Error> {
    let mut found = with_slug(env, slug)?;
    if found.len() > 1 {
        return Err(Error::Flock(format!(
            "the slug {slug:?} is used by more than one flock folder: {}",
            paths_of(&found)
        )));
    }
    let Some(flock) = found.pop() else {
        return Err(Error::Flock(format!("no flock named {slug:?}")));
    };
    if !roster::path(&flock.folder).exists() {
        return Err(Error::Flock(format!(
            "{} is a broken flock: it has no roster.json",
            flock.folder.display()
        )));
    }
    let on_disk = roster::load(&flock.folder)?;
    if on_disk.flock != slug {
        return Err(Error::Flock(format!(
            "{} is a broken flock: its roster.json says flock {:?}, not {slug:?}",
            flock.folder.display(),
            on_disk.flock
        )));
    }
    Ok(flock)
}

/// Creates the flock folder with `briefs/`, `results/` and the lock file. With `notes_root` set it also
/// creates the notes folder, empty: crook never writes in the notes root (ADR 0004). A slug that any
/// folder already uses, closed flocks included, is refused (Q2). `date` is `yyyy-mm-dd`.
pub fn create(env: Env, config: &Config, date: &str, slug: &str) -> Result<Flock, Error> {
    names::validate_slug(slug)?;
    if !is_date(date) {
        return Err(Error::Flock(format!("{date:?} is not a yyyy-mm-dd date")));
    }
    let existing = with_slug(env, slug)?;
    if !existing.is_empty() {
        return Err(Error::Flock(format!(
            "the slug {slug:?} is already used by {}",
            paths_of(&existing)
        )));
    }
    let flock = Flock {
        slug: slug.to_string(),
        date: date.to_string(),
        folder: paths::flock_folder(env, &names::folder_name(date, slug))?,
    };
    if let Some(parent) = flock.folder.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    // `create_dir` fails when the folder exists, which closes the gap after the check above.
    std::fs::create_dir(&flock.folder).map_err(io(&flock.folder))?;
    // A half-made folder would burn the slug, so any failure from here on removes it again.
    if let Err(e) = populate(&flock, config) {
        let _ = std::fs::remove_dir_all(&flock.folder);
        return Err(e);
    }
    Ok(flock)
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Error + use<> {
    let path = path.to_path_buf();
    move |source| Error::File { path, source }
}

fn populate(flock: &Flock, config: &Config) -> Result<(), Error> {
    for sub in ["briefs", "results"] {
        std::fs::create_dir(flock.folder.join(sub)).map_err(io(&flock.folder.join(sub)))?;
    }
    let lock = flock.folder.join(LOCK_FILE);
    std::fs::File::create(&lock).map_err(io(&lock))?;
    if config.notes_root.is_some() {
        let notes = flock.notes_folder(config);
        std::fs::create_dir_all(&notes).map_err(io(&notes))?;
    }
    Ok(())
}
