//! Temp-and-rename writes and the flock's lock (ADR 0024).
//!
//! The rename stops torn reads, not lost updates: two writers can each read, change and write the
//! same file. A read-modify-write cycle holds [`with_flock_lock`] for its whole length.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::Error;

#[cfg(test)]
mod tests;

/// The lock file inside a flock folder.
pub const LOCK_FILE: &str = ".lock";

fn file_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    |source| Error::File {
        path: path.to_path_buf(),
        source,
    }
}

/// Writes `bytes` to a new temporary file next to `path` and flushes it to disk. The target is
/// untouched. The name carries the process id, the clock and a counter; a name that is taken anyway
/// (a stale file from a crashed run, say) is skipped for a fresh one.
fn write_temp(path: &Path, bytes: &[u8]) -> Result<PathBuf, Error> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let mut last = None;
    for _ in 0..8 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let tmp = path.with_file_name(format!(
            ".{name}.tmp-{}-{nanos:x}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp);
        let mut file = match file {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last = Some((tmp, e));
                continue;
            }
            Err(source) => return Err(file_err(&tmp)(source)),
        };
        if let Err(source) = file.write_all(bytes).and_then(|()| file.sync_all()) {
            let _ = std::fs::remove_file(&tmp);
            return Err(file_err(&tmp)(source));
        }
        return Ok(tmp);
    }
    let (tmp, source) = last.unwrap_or_else(|| {
        (
            path.to_path_buf(),
            std::io::Error::other("no temporary name"),
        )
    });
    Err(file_err(&tmp)(source))
}

/// Writes `value` as pretty JSON with a trailing newline: to a temporary file in the same folder,
/// flushed, then renamed over `path`.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::File {
        path: path.to_path_buf(),
        source: std::io::Error::other(e),
    })?;
    bytes.push(b'\n');
    let tmp = write_temp(path, &bytes)?;
    if let Err(source) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(file_err(path)(source));
    }
    // Flushes the rename itself. The write is already in place, so a platform that cannot sync a
    // folder is not an error.
    if let Some(dir) = path.parent()
        && let Ok(dir) = std::fs::File::open(dir)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Runs `f` holding an exclusive lock on `<folder>/.lock`, creating the file if it is missing. The
/// lock is released when `f` returns. It is not re-entrant: take it once per cycle.
pub fn with_flock_lock<R>(folder: &Path, f: impl FnOnce() -> Result<R, Error>) -> Result<R, Error> {
    let path = folder.join(LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(file_err(&path))?;
    file.lock().map_err(file_err(&path))?;
    let result = f();
    // Closing the file would release it too; unlocking first keeps that explicit.
    let _ = file.unlock();
    result
}
