//! The project record: what and why, where it stands, and its item lists.
//!
//! Every function takes the connection inside a `Store::write` transaction, so
//! the several statements of one change commit together.

use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::privacy::names_home;
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Repo,
    Notes,
    Private,
    Allow,
    Link,
    Ticket,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Repo => "repo",
            ItemKind::Notes => "notes",
            ItemKind::Private => "private",
            ItemKind::Allow => "allow",
            ItemKind::Link => "link",
            ItemKind::Ticket => "ticket",
        }
    }

    /// Repo and notes values are directories, stored absolute and cleaned.
    /// The kinds the orchestrator's tools may add. `private` and `allow` shape
    /// the privacy check, so only the user writes them.
    pub fn mod_addable(self) -> bool {
        matches!(
            self,
            ItemKind::Repo | ItemKind::Notes | ItemKind::Link | ItemKind::Ticket
        )
    }

    fn is_dir(self) -> bool {
        matches!(self, ItemKind::Repo | ItemKind::Notes)
    }
}

impl fmt::Display for ItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ItemKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "repo" => ItemKind::Repo,
            "notes" => ItemKind::Notes,
            "private" => ItemKind::Private,
            "allow" => ItemKind::Allow,
            "link" => ItemKind::Link,
            "ticket" => ItemKind::Ticket,
            other => {
                return Err(format!(
                    "unknown kind {other:?}: use repo, notes, private, allow, link or ticket"
                ));
            }
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub what: String,
    pub why: String,
    pub status_text: String,
    pub version: i64,
    pub card_updated_at: i64,
    pub dones_since_card: i64,
    pub repos: Vec<String>,
    pub notes: Vec<String>,
    pub private: Vec<String>,
    pub allow: Vec<String>,
    pub links: Vec<String>,
    pub tickets: Vec<String>,
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Creates the project's database and record. Refuses a slug that already has one.
pub fn init(env: Env, slug: &str) -> Result<(), Error> {
    let mut store = Store::open(env, slug, Mode::Migrate)?;
    store.write(Touch::PROJECT, |tx| {
        let n: i64 = tx.query_row("SELECT count(*) FROM project", [], |r| r.get(0))?;
        if n > 0 {
            return Err(Error::Refused(format!("project {slug} already exists")));
        }
        tx.execute(
            "INSERT INTO project(id, card_updated_at) VALUES (1, ?1)",
            [now()],
        )?;
        Ok(())
    })
}

pub fn get(c: &Connection) -> Result<Record, Error> {
    let mut r = c
        .query_row(
            "SELECT what, why, status_text, version, card_updated_at, dones_since_card
             FROM project WHERE id = 1",
            [],
            |row| {
                Ok(Record {
                    what: row.get(0)?,
                    why: row.get(1)?,
                    status_text: row.get(2)?,
                    version: row.get(3)?,
                    card_updated_at: row.get(4)?,
                    dones_since_card: row.get(5)?,
                    ..Record::default()
                })
            },
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                Error::Refused("no project record: run crook init <slug>".into())
            }
            e => e.into(),
        })?;
    let mut st = c.prepare("SELECT kind, value FROM project_item ORDER BY kind, pos")?;
    let rows = st.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (kind, value) = row?;
        match kind.as_str() {
            "repo" => r.repos.push(value),
            "notes" => r.notes.push(value),
            "private" => r.private.push(value),
            "allow" => r.allow.push(value),
            "link" => r.links.push(value),
            "ticket" => r.tickets.push(value),
            _ => {}
        }
    }
    Ok(r)
}

/// One `UPDATE … WHERE version = ?` and a rows-changed test, so two writers
/// cannot overwrite each other. A set also marks the card fresh.
fn set(
    c: &Connection,
    assignments: &str,
    params: &[&dyn rusqlite::ToSql],
    version: i64,
) -> Result<i64, Error> {
    let sql = format!(
        "UPDATE project SET {assignments}, version = version + 1, card_updated_at = ?, \
         dones_since_card = 0 WHERE id = 1 AND version = ?"
    );
    let stamp = now();
    let mut all = params.to_vec();
    all.push(&stamp);
    all.push(&version);
    if c.execute(&sql, all.as_slice())? == 1 {
        Ok(version + 1)
    } else {
        Err(Error::Stale)
    }
}

pub fn set_what_why(c: &Connection, what: &str, why: &str, version: i64) -> Result<i64, Error> {
    set(c, "what = ?, why = ?", &[&what, &why], version)
}

pub fn set_status(c: &Connection, text: &str, version: i64) -> Result<i64, Error> {
    set(c, "status_text = ?", &[&text], version)
}

/// Lexically cleans an absolute path: drops `.` and trailing `/`, folds `..`.
/// No filesystem access, so a missing path and a symlink stay as written.
pub fn clean_path(p: &str) -> Result<String, Error> {
    let path = Path::new(p);
    if !path.is_absolute() {
        return Err(Error::Refused(format!("path must be absolute: {p:?}")));
    }
    let mut out = PathBuf::from("/");
    for comp in path.components() {
        match comp {
            Component::Normal(n) => out.push(n),
            Component::ParentDir => {
                out.pop();
            }
            _ => {}
        }
    }
    out.to_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Refused("path is not valid UTF-8".into()))
}

fn normalise(kind: ItemKind, value: &str) -> Result<String, Error> {
    if value.trim().is_empty() {
        return Err(Error::Refused("empty value".into()));
    }
    if kind == ItemKind::Repo {
        // Children record the resolved repo, so the card must spell it the same way.
        // A repo gone from disk resolves by its parent, so it can still be removed.
        let clean = clean_path(value)?;
        let path = Path::new(&clean);
        let resolved = std::fs::canonicalize(path).ok().or_else(|| {
            let parent = std::fs::canonicalize(path.parent()?).ok()?;
            Some(parent.join(path.file_name()?))
        });
        Ok(resolved
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or(clean))
    } else if kind.is_dir() {
        clean_path(value)
    } else {
        Ok(value.to_string())
    }
}

fn bump(c: &Connection) -> Result<i64, Error> {
    c.query_row(
        "UPDATE project SET version = version + 1 WHERE id = 1 RETURNING version",
        [],
        |r| r.get(0),
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            Error::Refused("no project record: run crook init <slug>".into())
        }
        e => e.into(),
    })
}

fn insert(c: &Connection, kind: ItemKind, value: &str) -> Result<(), Error> {
    c.execute(
        "INSERT OR IGNORE INTO project_item(kind, value, pos)
         SELECT ?1, ?2, COALESCE(MAX(pos) + 1, 0) FROM project_item WHERE kind = ?1",
        (kind.as_str(), value),
    )?;
    Ok(())
}

/// Appends an item and returns the new version. Takes no version: it only
/// adds. A notes folder is also private, so a note can never be public by omission.
pub fn add(c: &Connection, kind: ItemKind, value: &str) -> Result<i64, Error> {
    let value_in = value;
    let value = normalise(kind, value)?;
    if kind == ItemKind::Repo {
        // A repo added before repos were stored resolved keeps its old spelling: no second row.
        let old: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_item WHERE kind = 'repo' AND value = ?1)",
            [clean_path(value_in)?],
            |r| r.get(0),
        )?;
        if old {
            return bump(c);
        }
    }
    // The privacy check refuses a relative private entry, so none is stored.
    if kind == ItemKind::Private && !value.starts_with('/') && !names_home(&value) {
        return Err(Error::Refused(format!(
            "private entry is not absolute: {value:?}"
        )));
    }
    insert(c, kind, &value)?;
    if kind == ItemKind::Notes {
        insert(c, ItemKind::Private, &value)?;
    }
    bump(c)
}

/// Removes one item of that kind and returns the new version. Removing a
/// notes folder also drops its `private` entry; a `private` entry that is a
/// notes folder cannot be removed on its own.
pub fn remove(c: &Connection, kind: ItemKind, value: &str) -> Result<i64, Error> {
    let value_in = value;
    let value = normalise(kind, value)?;
    if kind == ItemKind::Private {
        let is_notes: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_item WHERE kind = 'notes' AND value = ?1)",
            [&value],
            |r| r.get(0),
        )?;
        if is_notes {
            return Err(Error::Refused(format!(
                "{value:?} is a notes folder, so it stays private: remove the notes folder instead"
            )));
        }
    }
    let mut n = c.execute(
        "DELETE FROM project_item WHERE kind = ?1 AND value = ?2",
        (kind.as_str(), &value),
    )?;
    if n == 0 && kind == ItemKind::Repo {
        // A repo added before repos were stored resolved keeps its old spelling.
        n = c.execute(
            "DELETE FROM project_item WHERE kind = 'repo' AND value = ?1",
            [clean_path(value_in)?],
        )?;
    }
    if n == 0 {
        return Err(Error::Refused(format!("no {kind} item {value:?}")));
    }
    if kind == ItemKind::Notes {
        c.execute(
            "DELETE FROM project_item WHERE kind = 'private' AND value = ?1",
            [&value],
        )?;
    }
    bump(c)
}

#[cfg(test)]
mod tests;
