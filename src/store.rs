use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bitflags::bitflags;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::{Env, Error, paths};

pub const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = include_str!("store/schema.sql");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Never creates or migrates; any schema version opens (for `crook list`).
    ReadOnly,
    /// Needs a database already at `SCHEMA_VERSION`.
    ReadWrite,
    /// Creates the directory and database, and migrates. Only `crook open` and `crook resume`.
    Migrate,
}

bitflags! {
    /// What a write changed, so watchers know to re-read. Any bit bumps `gen`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Touch: u8 {
        const INBOX = 1;
        const STATUS = 2;
        const CHILDREN = 4;
        const PROJECT = 8;
    }
}

pub struct Store {
    conn: Connection,
    gen_path: PathBuf,
    slug: String,
}

impl Store {
    pub fn open(env: Env, slug: &str, mode: Mode) -> Result<Store, Error> {
        Store::open_with_busy(env, slug, mode, Duration::from_secs(5))
    }

    /// `open` with its own busy timeout, for callers on a tighter budget.
    pub fn open_with_busy(
        env: Env,
        slug: &str,
        mode: Mode,
        busy: Duration,
    ) -> Result<Store, Error> {
        let dir = paths::project_dir(env, slug)?;
        let db = dir.join("crook.db");
        let schema_err = || Error::Schema {
            slug: slug.to_string(),
        };
        let flags = match mode {
            Mode::ReadOnly => OpenFlags::SQLITE_OPEN_READ_ONLY,
            Mode::ReadWrite => OpenFlags::SQLITE_OPEN_READ_WRITE,
            Mode::Migrate => {
                std::fs::create_dir_all(&dir)?;
                OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
            }
        } | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if mode != Mode::Migrate && !db.is_file() {
            return Err(schema_err());
        }
        let mut conn = Connection::open_with_flags(&db, flags)?;
        conn.busy_timeout(busy)?;
        match mode {
            Mode::ReadOnly => {}
            Mode::ReadWrite => {
                if user_version(&conn)? != SCHEMA_VERSION {
                    return Err(schema_err());
                }
            }
            Mode::Migrate => {
                // WAL persists in the file; the pragma returns a row.
                conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| {
                    r.get::<_, String>(0)
                })?;
                migrate(&mut conn).map_err(|e| match e {
                    Error::Schema { .. } => schema_err(),
                    e => e,
                })?;
            }
        }
        Ok(Store {
            conn,
            gen_path: dir.join("gen"),
            slug: slug.to_string(),
        })
    }

    /// Runs `f` in an IMMEDIATE transaction, so concurrent writers queue on
    /// `busy_timeout` instead of failing on a lock upgrade mid-transaction.
    /// A non-empty `touches` bumps `gen` after the commit.
    pub fn write<T>(
        &mut self,
        touches: Touch,
        f: impl FnOnce(&Transaction) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let out = f(&tx)?;
        tx.commit()?;
        if !touches.is_empty() {
            self.bump_gen();
        }
        Ok(out)
    }

    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
        f(&self.conn)
    }

    pub fn gen_path(&self) -> &Path {
        &self.gen_path
    }

    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Best effort: the commit is already durable, so a failure here must not
    /// report the write as failed. The watcher's 60 s safety net covers a miss.
    fn bump_gen(&self) {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let tmp = self.gen_path.with_extension(format!(
            "tmp{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let done = std::fs::write(&tmp, nanos.to_string())
            .and_then(|()| std::fs::rename(&tmp, &self.gen_path));
        if done.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn user_version(conn: &Connection) -> Result<i32, Error> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// Re-checks the version inside the IMMEDIATE lock, so two concurrent
/// `crook open`s cannot both create the tables.
fn migrate(conn: &mut Connection) -> Result<(), Error> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    match user_version(&tx)? {
        0 => {
            tx.execute_batch(SCHEMA)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
            Ok(())
        }
        SCHEMA_VERSION => Ok(()),
        _ => Err(Error::Schema {
            slug: String::new(),
        }),
    }
}

#[cfg(test)]
mod tests;
