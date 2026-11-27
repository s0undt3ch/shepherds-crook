//! `crook hook user-prompt-submit`: the results rescan and the reload nudge (ADR 0008, 0019, 0020;
//! Q26, Q27).

use std::path::PathBuf;

use super::{Input, Reply, Who, version};
use crate::Ctx;
use crate::cmd::Io;
use crate::files::{marks, result};
use crate::flock::{self, Flock};
use crate::hc::Hc;

#[cfg(test)]
mod tests;

/// The line the coordinator is told, as spike 14 worded it and the coordinator skill expects.
pub const UNDELIVERED: &str = "Undelivered child result: ";

/// A result the coordinator has not been told about: found by [`scan`], marked by [`commit`] once the
/// line that reports it has gone out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undelivered {
    pub folder: PathBuf,
    pub child: String,
    pub hash: String,
    pub path: PathBuf,
}

impl Undelivered {
    pub fn line(&self) -> String {
        format!("{UNDELIVERED}{}", self.path.display())
    }
}

/// Every `results/*.md` (dot files skipped) whose hash differs from its mark. Marks nothing: see
/// [`commit`]. An unreadable marks file counts as empty and is warned about once; [`commit`] rewrites
/// it. Also run by `session-start`, since the rescan happens "at start and on every turn" (ADR 0008).
pub fn scan(flock: &Flock, io: &mut Io) -> Vec<Undelivered> {
    let dir = flock.folder.join("results");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name.ends_with(".md") && e.path().is_file()
        })
        .map(|e| e.path())
        .collect();
    files.sort();
    let (marks, broken) = marks::load_or_empty(&flock.folder);
    if let Some(e) = broken {
        io.warn(format!(
            "crook hook: {e}; treating every result as undelivered"
        ));
    }
    let mut found = Vec::new();
    for path in files {
        let Some(child) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let hash = match result::read(&path) {
            Ok(info) => match info.sha256 {
                Some(h) => h,
                None => continue,
            },
            Err(e) => {
                io.warn(format!("crook hook: {e}"));
                continue;
            }
        };
        if marks.get(&child) != Some(&hash) {
            found.push(Undelivered {
                folder: flock.folder.clone(),
                child,
                hash,
                path,
            });
        }
    }
    found
}

/// Marks results delivered, under the flock's lock. A mark that cannot be written is warned about; the
/// worst case is the same line again next turn.
pub fn commit(items: &[&Undelivered], io: &mut Io) {
    for item in items {
        if let Err(e) = marks::mark(&item.folder, &item.child, &item.hash) {
            io.warn(format!(
                "crook hook: could not mark {} delivered: {e}",
                item.child
            ));
        }
    }
}

pub fn run(ctx: &Ctx, _hc: &dyn Hc, io: &mut Io, input: &Input, who: &Who) -> Reply {
    let mut reply = Reply::default();
    if let Some(session) = &input.session_id {
        match version::take_nudge(ctx, session) {
            Ok(true) => reply.lines.push(version::nudge_line(who)),
            Ok(false) => {}
            Err(e) => io.warn(format!("crook hook: {e}")),
        }
    }
    // A child gets no result lines: only the coordinator reads results.
    if who.child.is_none() {
        match flock::find(ctx.env, &who.flock) {
            Ok(flock) => reply.undelivered = scan(&flock, io),
            Err(e) => io.warn(format!("crook hook: {e}")),
        }
    }
    reply
}
