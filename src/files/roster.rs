//! `roster.json` (ADR 0024, 0017, 0025; Q8, Q9, Q11). The roster changes only through the operations
//! here, each holding the flock's lock for its whole read-modify-write cycle.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic::{with_flock_lock, write_json_atomic};
use crate::{Error, names};

#[cfg(test)]
mod tests;

pub const FILE: &str = "roster.json";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Active,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinator {
    pub session_id: String,
    pub folder: PathBuf,
    pub state: State,
    /// False until the coordinator's first launch has succeeded (Q8).
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Child {
    pub name: String,
    pub role: String,
    /// The model the child was started on, never empty (Q9).
    pub model: String,
    pub session_id: String,
    pub folder: PathBuf,
    pub repo: Option<PathBuf>,
    pub branch: Option<String>,
    /// Relative to the flock folder, under `briefs/` (Q12).
    pub brief: String,
    pub state: State,
    /// False until a launch has succeeded or the terminal was seen live (D50). `spawn` writes the
    /// entry before `hc run`, so a crash between the two leaves a child that was never started and
    /// must be launched with `--session-id`, not `--resume`.
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roster {
    pub version: u32,
    pub flock: String,
    pub coordinator: Coordinator,
    pub children: Vec<Child>,
}

impl Roster {
    /// A new roster for a flock: no children, the coordinator active and not yet started.
    pub fn new(flock: &str, coordinator_session_id: &str, coordinator_folder: &Path) -> Roster {
        Roster {
            version: VERSION,
            flock: flock.to_string(),
            coordinator: Coordinator {
                session_id: coordinator_session_id.to_string(),
                folder: coordinator_folder.to_path_buf(),
                state: State::Active,
                started: false,
            },
            children: Vec::new(),
        }
    }
}

pub fn path(folder: &Path) -> PathBuf {
    folder.join(FILE)
}

fn err(folder: &Path, message: impl Into<String>) -> Error {
    Error::Roster {
        path: path(folder),
        message: message.into(),
    }
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

/// `briefs/<one segment>`, the segment neither `.` nor `..` (Q12). The schema says the same.
fn brief_ok(brief: &str) -> bool {
    brief
        .strip_prefix("briefs/")
        .is_some_and(|seg| !seg.is_empty() && !seg.contains('/') && seg != "." && seg != "..")
}

/// Everything `schema/roster.schema.json` demands beyond the shape serde already enforces. Runs on
/// load and before every write, so crook neither accepts nor produces what the schema refuses.
fn check(folder: &Path, roster: &Roster) -> Result<(), Error> {
    let bad = |what: String| Err(err(folder, what));
    if roster.version != VERSION {
        return bad(format!("unsupported roster version {}", roster.version));
    }
    names::validate_slug(&roster.flock)?;
    if blank(&roster.coordinator.session_id) {
        return bad("the coordinator has an empty session_id".to_string());
    }
    if blank(&roster.coordinator.folder.to_string_lossy()) {
        return bad("the coordinator has an empty folder".to_string());
    }
    for child in &roster.children {
        names::validate_child(&child.name)?;
        let name = &child.name;
        for (field, value) in [
            ("role", child.role.as_str()),
            ("model", child.model.as_str()),
            ("session_id", child.session_id.as_str()),
            ("folder", &child.folder.to_string_lossy()),
        ] {
            if blank(value) {
                return bad(format!("child {name:?} has an empty {field}"));
            }
        }
        if !brief_ok(&child.brief) {
            return bad(format!(
                "child {name:?}: brief {:?} must be briefs/<file>, with no further folders or `..`",
                child.brief
            ));
        }
    }
    Ok(())
}

/// Reads and checks `<folder>/roster.json`. Takes no lock: the atomic rename keeps a reader safe.
pub fn load(folder: &Path) -> Result<Roster, Error> {
    let file = path(folder);
    let text = std::fs::read_to_string(&file).map_err(|source| Error::File {
        path: file.clone(),
        source,
    })?;
    let mut value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| err(folder, e.to_string()))?;
    // JSON Schema counts `1.0` as the integer 1, so the loader does too.
    let version = value.get("version").and_then(|v| {
        v.as_u64().or_else(|| {
            v.as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                .map(|f| f as u64)
        })
    });
    match version {
        Some(v) if v == u64::from(VERSION) => {}
        Some(v) => {
            return Err(err(
                folder,
                format!("unsupported roster version {v}, this crook reads version {VERSION}"),
            ));
        }
        None => return Err(err(folder, "missing or malformed `version`")),
    }
    value["version"] = serde_json::json!(VERSION);
    let roster: Roster = serde_json::from_value(value).map_err(|e| err(folder, e.to_string()))?;
    check(folder, &roster)?;
    Ok(roster)
}

/// Writes a new roster. Refuses to replace one.
pub fn create(folder: &Path, roster: &Roster) -> Result<(), Error> {
    with_flock_lock(folder, || {
        if path(folder).exists() {
            return Err(err(folder, "the roster already exists"));
        }
        check(folder, roster)?;
        write_json_atomic(&path(folder), roster)
    })
}

fn modify<R>(folder: &Path, f: impl FnOnce(&mut Roster) -> Result<R, Error>) -> Result<R, Error> {
    with_flock_lock(folder, || {
        let mut roster = load(folder)?;
        let out = f(&mut roster)?;
        check(folder, &roster)?;
        write_json_atomic(&path(folder), &roster)?;
        Ok(out)
    })
}

fn child_mut<'a>(
    folder: &Path,
    roster: &'a mut Roster,
    name: &str,
) -> Result<&'a mut Child, Error> {
    roster
        .children
        .iter_mut()
        .find(|c| c.name == name)
        .ok_or_else(|| err(folder, format!("no child named {name:?}")))
}

/// Adds a child. A name already in the roster is an error.
pub fn add_child(folder: &Path, child: Child) -> Result<(), Error> {
    names::validate_child(&child.name)?;
    modify(folder, |roster| {
        if roster.children.iter().any(|c| c.name == child.name) {
            return Err(err(
                folder,
                format!("child {:?} already exists", child.name),
            ));
        }
        roster.children.push(child);
        Ok(())
    })
}

/// Removes a child, freeing its name (Q11).
pub fn remove_child(folder: &Path, name: &str) -> Result<(), Error> {
    modify(folder, |roster| {
        child_mut(folder, roster, name)?;
        roster.children.retain(|c| c.name != name);
        Ok(())
    })
}

pub fn set_child_state(folder: &Path, name: &str, state: State) -> Result<(), Error> {
    modify(folder, |roster| {
        child_mut(folder, roster, name)?.state = state;
        Ok(())
    })
}

pub fn set_child_model(folder: &Path, name: &str, model: &str) -> Result<(), Error> {
    modify(folder, |roster| {
        child_mut(folder, roster, name)?.model = model.to_string();
        Ok(())
    })
}

pub fn set_coordinator_state(folder: &Path, state: State) -> Result<(), Error> {
    modify(folder, |roster| {
        roster.coordinator.state = state;
        Ok(())
    })
}

pub fn set_coordinator_started(folder: &Path, started: bool) -> Result<(), Error> {
    modify(folder, |roster| {
        roster.coordinator.started = started;
        Ok(())
    })
}

/// Marks every child and the coordinator `done` in one cycle, which is what closing a flock is
/// (ADR 0018, 0020). One write, so a crash leaves the flock closed or not, never half.
pub fn mark_all_done(folder: &Path) -> Result<(), Error> {
    modify(folder, |roster| {
        roster.coordinator.state = State::Done;
        for child in &mut roster.children {
            child.state = State::Done;
        }
        Ok(())
    })
}

pub fn set_child_started(folder: &Path, name: &str, started: bool) -> Result<(), Error> {
    modify(folder, |roster| {
        child_mut(folder, roster, name)?.started = started;
        Ok(())
    })
}
