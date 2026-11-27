//! Session version records, the self-reload and the reload nudge (ADR 0019, spike 20; Q17, Q26).
//!
//! SessionStart records the plugin version the session loaded. On Stop the plugin's version is read
//! again; when it moved, the session sends itself `/reload-plugins`, and on a successful send
//! records the new version and sets `reloaded`. The next prompt turns that flag into a one-line
//! nudge, because a reload leaves the old skill text in the conversation.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Who;
use crate::cmd::Io;
use crate::files::atomic::write_json_atomic;
use crate::hc::{Hc, SendOutcome};
use crate::{Ctx, Error, install, paths};

#[cfg(test)]
mod tests;

/// Records older than this are pruned (Q17, Q41).
pub const MAX_RECORD_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub const RELOAD_COMMAND: &str = "/reload-plugins";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub version: String,
    /// A reload happened that the model has not been told about yet.
    #[serde(default)]
    pub reloaded: bool,
}

/// The plugin version on disk now: through `$CLAUDE_PLUGIN_ROOT` (spike 20), else through the
/// `current` link (Q17).
pub fn current(ctx: &Ctx) -> Result<String, Error> {
    let dir: PathBuf = match (ctx.env)("CLAUDE_PLUGIN_ROOT").filter(|v| !v.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => paths::current_link(ctx.env)?.join("plugin"),
    };
    install::plugin_dir_version(&dir)
}

/// The record of a session. A missing or unreadable one is `None`: it is written again.
pub fn load(ctx: &Ctx, session: &str) -> Result<Option<Record>, Error> {
    let path = paths::session_record(ctx.env, session)?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(serde_json::from_str(&text).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::File { path, source }),
    }
}

pub fn save(ctx: &Ctx, session: &str, record: &Record) -> Result<(), Error> {
    let path = paths::session_record(ctx.env, session)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| Error::File {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    write_json_atomic(&path, record)
}

/// SessionStart: the session has loaded the plugin as it is now. With `overwrite` false (after
/// `clear` or `compact`) an existing record is kept and one is made only when there is none (D70).
pub fn record_start(ctx: &Ctx, session: &str, overwrite: bool) -> Result<(), Error> {
    if !overwrite && load(ctx, session)?.is_some() {
        return Ok(());
    }
    save(
        ctx,
        session,
        &Record {
            version: current(ctx)?,
            reloaded: false,
        },
    )
}

/// Stop: reloads this session's plugins when their version moved since the record. A failed send
/// keeps the old record, so the next Stop tries again.
pub fn self_reload(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, session: &str, who: &Who) {
    if let Err(e) = try_self_reload(ctx, hc, io, session, who) {
        io.warn(format!("crook hook: version check failed: {e}"));
    }
}

fn try_self_reload(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    session: &str,
    who: &Who,
) -> Result<(), Error> {
    let now = current(ctx)?;
    let Some(record) = load(ctx, session)? else {
        // Never recorded (the session started before this crook): what it runs now is the baseline.
        return save(
            ctx,
            session,
            &Record {
                version: now,
                reloaded: false,
            },
        );
    };
    if record.version == now {
        return Ok(());
    }
    // The new record goes down before the send (D71). A hook cut off by its time budget while `hc send`
    // waits would otherwise leave the old record, and every later Stop would send the reload again.
    // The price is that a send which never completes is not retried. A send that fails outright puts the
    // old record back, so the next Stop tries again.
    save(
        ctx,
        session,
        &Record {
            version: now,
            reloaded: true,
        },
    )?;
    let sent = hc.send(&who.address, RELOAD_COMMAND);
    let outcome = match sent {
        Ok(SendOutcome::Sent) => return Ok(()),
        Ok(other) => format!("{other:?}"),
        Err(e) => e.to_string(),
    };
    save(ctx, session, &record)?;
    io.warn(format!(
        "crook hook: could not reload the plugins ({outcome}); will try again next turn"
    ));
    Ok(())
}

/// UserPromptSubmit: whether a reload is waiting to be mentioned. Clears the flag.
pub fn take_nudge(ctx: &Ctx, session: &str) -> Result<bool, Error> {
    match load(ctx, session)? {
        Some(record) if record.reloaded => {
            save(
                ctx,
                session,
                &Record {
                    reloaded: false,
                    ..record
                },
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// The line that tells the model to load its skill again.
pub fn nudge_line(who: &Who) -> String {
    let skill = if who.child.is_some() {
        "crook-child"
    } else {
        "crook-coordinator"
    };
    format!(
        "crook: the plugin was reloaded to a newer version. Invoke your `{skill}` skill again before you carry on: the text of the old one is still in this conversation."
    )
}

/// Deletes the records not touched for [`MAX_RECORD_AGE`]; returns how many. For `setup` and
/// `doctor --fix` (Q41). A missing folder is nothing to prune.
pub fn prune(ctx: &Ctx) -> Result<usize, Error> {
    let dir = paths::sessions_dir(ctx.env)?;
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(source) => return Err(Error::File { path: dir, source }),
    };
    let now = ctx.clock.now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|m| m.modified().ok())
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > MAX_RECORD_AGE);
        if old && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}
