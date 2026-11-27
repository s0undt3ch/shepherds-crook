//! `results/.delivered.json`: a map from child name to the SHA-256 of the result last delivered
//! (ADR 0024). Written only by `crook hook`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::atomic::{with_flock_lock, write_json_atomic};
use crate::Error;

#[cfg(test)]
mod tests;

pub type Marks = BTreeMap<String, String>;

pub fn path(folder: &Path) -> PathBuf {
    folder.join("results/.delivered.json")
}

/// Reads the marks. A missing file is an empty map. Takes no lock: the atomic rename keeps a reader
/// safe.
pub fn load(folder: &Path) -> Result<Marks, Error> {
    let file = path(folder);
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| Error::Marks {
            path: file,
            message: e.to_string(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Marks::new()),
        Err(source) => Err(Error::File { path: file, source }),
    }
}

/// As [`load`], but a file that cannot be read as marks counts as empty: the error comes back beside
/// the map, for one warning. Nothing is known to be delivered then, so every result is reported once
/// more and marked again, which writes the file afresh.
pub fn load_or_empty(folder: &Path) -> (Marks, Option<Error>) {
    match load(folder) {
        Ok(marks) => (marks, None),
        Err(e) => (Marks::new(), Some(e)),
    }
}

/// Records that `child`'s result with this hash was delivered, under the flock's lock.
pub fn mark(folder: &Path, child: &str, sha256: &str) -> Result<(), Error> {
    with_flock_lock(folder, || {
        // A corrupt file is replaced, not an obstacle to delivering (the rescan would repeat for ever).
        let mut marks = load_or_empty(folder).0;
        marks.insert(child.to_string(), sha256.to_string());
        write_json_atomic(&path(folder), &marks)
    })
}
