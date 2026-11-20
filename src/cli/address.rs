//! `<child>` or `<slug>/<child>`: how `unpark`, `resume` and `stop` name a child.

use rusqlite::Connection;

use crate::store::{Mode, Store};
use crate::{Env, Error, paths};

/// Slugs of every project whose database holds `child`. A database that
/// cannot be opened or read is skipped: it cannot be the one meant.
fn projects_with(env: Env, child: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(paths::data_dir(env)) else {
        return Vec::new();
    };
    let mut slugs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().join("crook.db").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|slug| paths::valid_slug(slug).is_ok())
        .filter(|slug| {
            Store::open(env, slug, Mode::ReadOnly)
                .and_then(|s| s.read(|c| has_child(c, child)))
                .unwrap_or(false)
        })
        .collect();
    slugs.sort();
    slugs
}

fn has_child(c: &Connection, child: &str) -> Result<bool, Error> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM children WHERE child = ?1)",
        [child],
        |r| r.get(0),
    )?)
}

/// Returns `(slug, child)`. A bare child that more than one project holds is
/// refused, with the `<slug>/<child>` forms that match.
pub fn resolve(env: Env, arg: &str) -> Result<(String, String), Error> {
    if let Some((slug, child)) = arg.split_once('/') {
        paths::valid_slug(slug)?;
        paths::valid_slug(child).map_err(|_| Error::Refused(format!("invalid child {child:?}")))?;
        return Ok((slug.to_string(), child.to_string()));
    }
    paths::valid_slug(arg).map_err(|_| Error::Refused(format!("invalid child {arg:?}")))?;
    let mut found = projects_with(env, arg);
    match found.len() {
        0 => Err(Error::Refused(format!("no child {arg:?} in any project"))),
        1 => Ok((found.remove(0), arg.to_string())),
        _ => {
            let forms: Vec<String> = found.iter().map(|s| format!("{s}/{arg}")).collect();
            Err(Error::Refused(format!(
                "{arg:?} is in more than one project: use {}",
                forms.join(" or ")
            )))
        }
    }
}
