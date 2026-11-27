//! A child's `results/<child>.md` (ADR 0024, 0008). crook reads one frontmatter field, `status`, and
//! hashes the whole file (Q27); it parses nothing else.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::Error;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Working,
    Done,
    Failed,
    /// The file exists but has no frontmatter `status` of a known value.
    Unreadable,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultInfo {
    pub status: Status,
    /// Lower-case hex SHA-256 of the whole file; `None` when it is missing.
    pub sha256: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The `status` of a result's text. Never `Missing`.
pub fn parse_status(text: &str) -> Status {
    let mut lines = text.strip_prefix('\u{feff}').unwrap_or(text).lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Status::Unreadable;
    }
    let mut value = None;
    for line in lines {
        let line = line.trim_end();
        if line == "---" {
            return match value {
                Some("working") => Status::Working,
                Some("done") => Status::Done,
                Some("failed") => Status::Failed,
                _ => Status::Unreadable,
            };
        }
        if value.is_none()
            && let Some(v) = line.strip_prefix("status:")
        {
            // A trailing YAML comment is not part of the value.
            let v = v.split(" #").next().unwrap_or(v).trim();
            let unquoted = [('"', '"'), ('\'', '\'')]
                .iter()
                .find_map(|(open, close)| v.strip_prefix(*open)?.strip_suffix(*close));
            value = Some(unquoted.unwrap_or(v));
        }
    }
    // No closing delimiter: not a readable frontmatter.
    Status::Unreadable
}

/// Reads a result file. A missing file is `Missing`, not an error.
pub fn read(path: &Path) -> Result<ResultInfo, Error> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ResultInfo {
                status: Status::Missing,
                sha256: None,
            });
        }
        Err(source) => {
            return Err(Error::File {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let status = std::str::from_utf8(&bytes).map_or(Status::Unreadable, parse_status);
    Ok(ResultInfo {
        status,
        sha256: Some(sha256_hex(&bytes)),
    })
}
