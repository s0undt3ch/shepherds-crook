use std::path::Path;

use crate::project::{self, ItemKind, Record};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config, paths, privacy};

/// Makes `home` crook's, or refuses before any change: a missing or empty
/// directory gets the marker, a home crook made for `slug` is kept, and
/// anything else is the user's, which `wipe` would otherwise empty.
fn adopt_home(home: &Path, slug: &str) -> Result<(), Error> {
    let mut entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return paths::make_home(home, slug),
        Err(e) => return Err(e.into()),
    };
    if entries.next().is_none() {
        return paths::mark_home(home, slug);
    }
    if paths::is_crook_home(home, slug) {
        return Ok(());
    }
    Err(Error::Refused(format!(
        "the project home {} already exists and crook did not make it (no {} naming {slug}); \
         nothing was created: pick another name, or set `home` in config.toml elsewhere",
        home.display(),
        paths::HOME_MARKER
    )))
}

/// Creates the project record, `<home>/<slug>/` and the diary, and records the
/// diary as a notes folder. Makes no `repos/` folder.
pub fn run(env: Env, slug: &str) -> Result<String, Error> {
    paths::valid_slug(slug)?;
    let cfg = config::load(env)?;
    let home = cfg.project_home(env, slug)?;
    let diary = cfg.diary(env, slug)?;
    // The home is the orchestrator's cwd: a private location there would make
    // every ordinary call in it ask. The diary may be private: it is a notes folder.
    let set = privacy::Set::load(&Record::default(), &cfg, env, slug)?;
    if set.holds(&home) {
        return Err(Error::Refused(format!(
            "the project home {} is inside a private location: set `home` in config.toml elsewhere",
            home.display()
        )));
    }
    if paths::project_dir(env, slug)?.join("crook.db").is_file() {
        return Err(Error::Refused(format!("project {slug} already exists")));
    }
    adopt_home(&home, slug)?;
    std::fs::create_dir_all(&diary)?;
    project::init(env, slug)?;
    let diary_text = diary
        .to_str()
        .ok_or_else(|| Error::Refused("the diary path is not valid UTF-8".into()))?;
    let mut store = Store::open(env, slug, Mode::ReadWrite)?;
    store.write(Touch::PROJECT, |tx| {
        project::add(tx, ItemKind::Notes, diary_text).map(|_| ())
    })?;
    Ok(format!(
        "created project {slug}\nhome  {}\ndiary {}",
        home.display(),
        diary.display()
    ))
}

#[cfg(test)]
mod tests;
