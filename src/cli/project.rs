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
mod tests {
    use std::io::Cursor;

    use super::*;

    fn env_for(dir: &Path) -> impl Fn(&str) -> Option<String> + use<> {
        let x = dir.to_str().unwrap().to_string();
        move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone())
    }

    #[test]
    fn cli_project_version_check() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_for(tmp.path());
        project::init(&env, "p").unwrap();
        let mut cli = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        let mut tool = Store::open(&env, "p", Mode::ReadWrite).unwrap();

        // The CLI reads the version, then a tool write lands before its own.
        let seen = version(&cli).unwrap();
        tool.write(Touch::PROJECT, |tx| {
            project::set_status(tx, "from the tool", seen)
        })
        .unwrap();
        let e = set_status_at(&mut cli, "from the cli", seen).unwrap_err();
        assert!(matches!(e, Error::Stale), "{e}");
        cli.read(|c| {
            assert_eq!(project::get(c)?.status_text, "from the tool");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn set_commands_read_stdin_and_keep_the_other_field() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_for(tmp.path());
        project::init(&env, "p").unwrap();
        let cwd = Path::new("/");
        for (cmd, text) in [
            (ProjectCmd::SetWhat, "the what\n"),
            (ProjectCmd::SetWhy, "the why\n"),
            (ProjectCmd::SetStatus, "the status\n"),
        ] {
            run(&env, "p", cmd, cwd, &mut Cursor::new(text)).unwrap();
        }
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        s.read(|c| {
            let r = project::get(c)?;
            assert_eq!(
                (r.what.as_str(), r.why.as_str(), r.status_text.as_str()),
                ("the what", "the why", "the status")
            );
            Ok(())
        })
        .unwrap();
        let e = run(
            &env,
            "p",
            ProjectCmd::SetStatus,
            cwd,
            &mut Cursor::new("  \n"),
        )
        .unwrap_err();
        assert!(e.to_string().contains("no text"), "{e}");
    }

    #[test]
    fn relative_repo_paths_resolve_against_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_for(tmp.path());
        project::init(&env, "p").unwrap();
        run(
            &env,
            "p",
            ProjectCmd::AddRepo {
                path: "../other/./repo".into(),
            },
            Path::new("/work/here"),
            &mut Cursor::new(""),
        )
        .unwrap();
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        s.read(|c| {
            assert_eq!(project::get(c)?.repos, ["/work/other/repo"]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn relative_private_entries_resolve_against_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_for(tmp.path());
        project::init(&env, "p").unwrap();
        let cwd = Path::new("/work/here");
        for value in [
            "../notes/./secret/**",
            "plain",
            "/abs/../abs/x/",
            "~/vault",
            "$HOME/v",
            "${HOME}/w",
        ] {
            let cmd = ProjectCmd::AddPrivate {
                value: value.into(),
            };
            run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap();
        }
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        let private = s.read(|c| Ok(project::get(c)?.private)).unwrap();
        assert_eq!(
            private,
            [
                "/work/notes/secret/**",
                "/work/here/plain",
                "/abs/x",
                "~/vault",
                "$HOME/v",
                "${HOME}/w",
            ]
        );
        drop(s);
        let cmd = ProjectCmd::AddPrivate { value: " ".into() };
        let e = run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap_err();
        assert!(e.to_string().contains("empty value"), "{e}");
        // `remove` resolves the same way.
        let cmd = ProjectCmd::Remove {
            kind: ItemKind::Private,
            value: "plain".into(),
        };
        run(&env, "p", cmd, cwd, &mut Cursor::new("")).unwrap();
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        let private = s.read(|c| Ok(project::get(c)?.private)).unwrap();
        assert!(!private.iter().any(|p| p.ends_with("plain")), "{private:?}");
    }

    #[test]
    fn legacy_relative_private_entry_can_be_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let env = env_for(tmp.path());
        project::init(&env, "p").unwrap();
        let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        s.write(Touch::PROJECT, |tx| {
            tx.execute(
                "INSERT INTO project_item(kind, value, pos) VALUES ('private', 'rel/x', 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        drop(s);
        let cmd = ProjectCmd::Remove {
            kind: ItemKind::Private,
            value: "rel/x".into(),
        };
        run(&env, "p", cmd, Path::new("/work"), &mut Cursor::new("")).unwrap();
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        assert!(s.read(|c| Ok(project::get(c)?.private)).unwrap().is_empty());
    }
}
