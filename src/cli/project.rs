//! `crook project <slug> …`: the user's edits to the project record.

use std::io::Read;
use std::path::Path;

use clap::Subcommand;
use rusqlite::Connection;

use crate::privacy::names_home;
use crate::project::{self, ItemKind};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error};

#[derive(Subcommand)]
pub enum ProjectCmd {
    /// Add a repo (absolute path)
    AddRepo { path: String },
    /// Add a notes folder (absolute path; also private)
    AddNotes { path: String },
    /// Add a private path or glob (relative to the current directory, `~` or `$HOME`)
    AddPrivate { value: String },
    /// Add an allow entry for a known false positive
    AddAllow { value: String },
    /// Add a public link
    AddLink { url: String },
    /// Add a ticket
    AddTicket { ticket: String },
    /// Remove one item
    Remove { kind: ItemKind, value: String },
    /// Set what the project is (text on stdin)
    SetWhat,
    /// Set why it exists (text on stdin)
    SetWhy,
    /// Set where it stands (text on stdin)
    SetStatus,
}

/// Repo, notes and private paths may be relative to where the user ran the
/// command. A private entry is stored absolute and cleaned, or as written from
/// the home directory, which the privacy check expands itself.
fn resolve(kind: ItemKind, value: &str, cwd: &Path) -> String {
    match kind {
        ItemKind::Repo | ItemKind::Notes if Path::new(value).is_relative() => {
            cwd.join(value).display().to_string()
        }
        // An empty value goes on as it is, for `project::add` to refuse.
        ItemKind::Private if !value.trim().is_empty() && !names_home(value) => {
            let joined = cwd.join(value).display().to_string();
            project::clean_path(&joined).unwrap_or(joined)
        }
        _ => value.to_string(),
    }
}

fn read_text(stdin: &mut dyn Read) -> Result<String, Error> {
    let mut s = String::new();
    stdin.read_to_string(&mut s)?;
    let s = s.trim().to_string();
    if s.is_empty() {
        return Err(Error::Refused("no text on stdin".into()));
    }
    Ok(s)
}

fn version(store: &Store) -> Result<i64, Error> {
    store.read(|c: &Connection| Ok(project::get(c)?.version))
}

/// Writes with the version the caller read, so a write made in between gives
/// `Error::Stale` instead of being overwritten.
pub fn set_status_at(store: &mut Store, text: &str, version: i64) -> Result<i64, Error> {
    store.write(Touch::PROJECT, |tx| project::set_status(tx, text, version))
}

pub fn set_what_at(store: &mut Store, what: &str, version: i64) -> Result<i64, Error> {
    store.write(Touch::PROJECT, |tx| {
        let why = project::get(tx)?.why;
        project::set_what_why(tx, what, &why, version)
    })
}

pub fn set_why_at(store: &mut Store, why: &str, version: i64) -> Result<i64, Error> {
    store.write(Touch::PROJECT, |tx| {
        let what = project::get(tx)?.what;
        project::set_what_why(tx, &what, why, version)
    })
}

pub fn run(
    env: Env,
    slug: &str,
    cmd: ProjectCmd,
    cwd: &Path,
    stdin: &mut dyn Read,
) -> Result<String, Error> {
    let mut store = Store::open(env, slug, Mode::ReadWrite)?;
    let add = |store: &mut Store, kind: ItemKind, value: &str| {
        let value = resolve(kind, value, cwd);
        store.write(Touch::PROJECT, |tx| project::add(tx, kind, &value))
    };
    let v = match cmd {
        ProjectCmd::AddRepo { path } => add(&mut store, ItemKind::Repo, &path)?,
        ProjectCmd::AddNotes { path } => add(&mut store, ItemKind::Notes, &path)?,
        ProjectCmd::AddPrivate { value } => add(&mut store, ItemKind::Private, &value)?,
        ProjectCmd::AddAllow { value } => add(&mut store, ItemKind::Allow, &value)?,
        ProjectCmd::AddLink { url } => add(&mut store, ItemKind::Link, &url)?,
        ProjectCmd::AddTicket { ticket } => add(&mut store, ItemKind::Ticket, &ticket)?,
        ProjectCmd::Remove { kind, value } => {
            let resolved = resolve(kind, &value, cwd);
            // The value as written first: an entry stored before values were
            // resolved (a relative `private` one) must stay removable.
            store.write(Touch::PROJECT, |tx| {
                match project::remove(tx, kind, &value) {
                    Err(Error::Refused(_)) if resolved != value => {
                        project::remove(tx, kind, &resolved)
                    }
                    r => r,
                }
            })?
        }
        ProjectCmd::SetWhat => {
            let text = read_text(stdin)?;
            let v = version(&store)?;
            set_what_at(&mut store, &text, v)?
        }
        ProjectCmd::SetWhy => {
            let text = read_text(stdin)?;
            let v = version(&store)?;
            set_why_at(&mut store, &text, v)?
        }
        ProjectCmd::SetStatus => {
            let text = read_text(stdin)?;
            let v = version(&store)?;
            set_status_at(&mut store, &text, v)?
        }
    };
    Ok(format!("{slug}: project version {v}"))
}

#[cfg(test)]
mod tests;
