//! Children, their inbox, status and forks: DB functions only, no launching.
//!
//! Every function takes a `&Connection`, so a caller passes the `Transaction`
//! from `Store::write` for a write and `Store::read`'s connection for a read.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::card::{self, ChildLine, SelfPart};
use crate::gate::{Identity, Role};
use crate::{Error, paths, project};

/// Shown ahead of every question that reaches a model.
pub const QUESTION_LABEL: &str = "waiting on the user: don't answer or work around it";
pub const STOPPED: &str = "crook: this child was stopped";

const QUESTION_CAP: usize = 300;
const STALE_DONES: i64 = 3;
const STALE_SECS: i64 = 7 * 24 * 60 * 60;
const GOAL_CHARS: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Ok,
    Failed,
    Dropped,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Failed => "failed",
            Outcome::Dropped => "dropped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub child: String,
    pub repo: String,
    pub worktree: String,
    pub branch: String,
    pub brief: String,
    pub session_id: String,
}

fn no_such(child: &str) -> Error {
    Error::Refused(format!("no child {child:?}"))
}

/// Matches `^https://[^/]+/[^/]+/[^/]+/pull/[0-9]+$`. Whitespace and control
/// characters are refused too: the URL ends up in the orchestrator's context.
pub fn valid_pr_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let parts: Vec<&str> = rest.split('/').collect();
    matches!(parts.as_slice(), [host, owner, repo, "pull", n]
        if !host.is_empty()
            && !owner.is_empty()
            && !repo.is_empty()
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Inserts the child and its `working` status row.
pub fn add_child(c: &Connection, ch: &Child) -> Result<(), Error> {
    paths::valid_slug(&ch.child)
        .map_err(|_| Error::Refused(format!("invalid child name {:?}", ch.child)))?;
    let now = project::now();
    let n = c.execute(
        "INSERT OR IGNORE INTO children
           (child, repo, worktree, branch, brief, session_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        (
            &ch.child,
            &ch.repo,
            &ch.worktree,
            &ch.branch,
            &ch.brief,
            &ch.session_id,
            now,
        ),
    )?;
    if n == 0 {
        return Err(Error::Refused(format!("child {} already exists", ch.child)));
    }
    c.execute(
        "INSERT INTO status(child, state, seq, changed_at) VALUES (?1, 'working', 0, ?2)",
        (&ch.child, now),
    )?;
    Ok(())
}

/// Checked inside the same transaction as the write that follows: the child
/// must exist and not be archived, and with `need_current` the caller's
/// session must be the recorded one.
pub fn guard(
    c: &Connection,
    child: &str,
    session_id: &str,
    need_current: bool,
) -> Result<(), Error> {
    let (archived, current): (bool, String) = c
        .query_row(
            "SELECT archived, session_id FROM children WHERE child = ?1",
            [child],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| no_such(child))?;
    if archived {
        return Err(Error::Refused(STOPPED.into()));
    }
    if need_current && current != session_id {
        return Err(Error::Refused(format!(
            "this is a fork; the child is session {current}"
        )));
    }
    Ok(())
}

/// Adds a task to the child's inbox and returns its `seq`.
pub fn task(c: &Connection, child: &str, text: &str) -> Result<i64, Error> {
    if text.trim().is_empty() {
        return Err(Error::Refused("empty task".into()));
    }
    guard(c, child, "", false)?;
    c.execute(
        "INSERT INTO inbox(child, text) VALUES (?1, ?2)",
        (child, text),
    )?;
    Ok(c.last_insert_rowid())
}

struct StatusRow {
    state: String,
    current_task: Option<i64>,
}

fn status_row(c: &Connection, child: &str) -> Result<StatusRow, Error> {
    c.query_row(
        "SELECT state, current_task FROM status WHERE child = ?1",
        [child],
        |r| {
            Ok(StatusRow {
                state: r.get(0)?,
                current_task: r.get(1)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| no_such(child))
}

/// Takes the oldest untaken task. Refuses while parked, while a task is open
/// and on an empty inbox. A child coming back from `done` becomes `working`.
pub fn take_task(c: &Connection, child: &str) -> Result<(i64, String), Error> {
    let row = status_row(c, child)?;
    if row.state == "parked" {
        return Err(Error::Refused(
            "parked: the user has to answer first".into(),
        ));
    }
    if row.current_task.is_some() {
        return Err(Error::Refused("task open: call done first".into()));
    }
    let (seq, text): (i64, String) = c
        .query_row(
            "SELECT seq, text FROM inbox WHERE child = ?1 AND taken = 0 ORDER BY seq LIMIT 1",
            [child],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| Error::Refused("no tasks".into()))?;
    c.execute("UPDATE inbox SET taken = 1 WHERE seq = ?1", [seq])?;
    c.execute(
        "UPDATE status SET current_task = ?2, state = 'working', outcome = NULL, pr_url = NULL,
                seq = seq + CASE WHEN state = 'working' THEN 0 ELSE 1 END, changed_at = ?3
         WHERE child = ?1",
        (child, seq, project::now()),
    )?;
    Ok((seq, text))
}

pub fn need(c: &Connection, child: &str, q: &str) -> Result<(), Error> {
    if q.trim().is_empty() {
        return Err(Error::Refused("empty question".into()));
    }
    status_row(c, child)?;
    c.execute(
        "UPDATE status SET state = 'parked', question = ?2, seq = seq + 1, changed_at = ?3
         WHERE child = ?1",
        (child, q, project::now()),
    )?;
    Ok(())
}

/// Closes the current task. A parked child cannot call it: the user answers
/// first. Counts towards the card's staleness.
pub fn done(
    c: &Connection,
    child: &str,
    outcome: Outcome,
    pr_url: Option<&str>,
) -> Result<(), Error> {
    if let Some(u) = pr_url
        && !valid_pr_url(u)
    {
        return Err(Error::Refused(format!(
            "pr_url must look like https://host/owner/repo/pull/N, not {u:?}"
        )));
    }
    let row = status_row(c, child)?;
    if row.state == "parked" {
        return Err(Error::Refused(
            "parked: the user has to answer first".into(),
        ));
    }
    if row.state == "done" && row.current_task.is_none() {
        return Err(Error::Refused("no task open: already done".into()));
    }
    c.execute(
        "UPDATE status SET state = 'done', question = NULL, current_task = NULL,
                outcome = ?2, pr_url = ?3, seq = seq + 1, done_seq = seq + 1, changed_at = ?4
         WHERE child = ?1",
        (child, outcome.as_str(), pr_url, project::now()),
    )?;
    c.execute(
        "UPDATE project SET dones_since_card = dones_since_card + 1 WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// Lifts a park. True when the child was parked; the seq moves only then.
pub fn lift(c: &Connection, child: &str) -> Result<bool, Error> {
    let n = c.execute(
        "UPDATE status SET state = 'working', question = NULL, seq = seq + 1, changed_at = ?2
         WHERE child = ?1 AND state = 'parked'",
        (child, project::now()),
    )?;
    Ok(n == 1)
}

pub fn parked(c: &Connection, child: &str) -> Result<bool, Error> {
    Ok(status_row(c, child)?.state == "parked")
}

/// Like `parked`, but a child that does not exist is simply not parked.
pub fn parked_lenient(c: &Connection, child: &str) -> Result<bool, Error> {
    let state: Option<String> = c
        .query_row("SELECT state FROM status WHERE child = ?1", [child], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(state.as_deref() == Some("parked"))
}

/// A live parked child whose repo is `repo` (trailing slashes ignored).
pub fn parked_in_repo(c: &Connection, repo: &str) -> Result<Option<String>, Error> {
    Ok(c.query_row(
        "SELECT c.child FROM children c JOIN status s USING (child)
         WHERE c.archived = 0 AND s.state = 'parked'
           AND rtrim(c.repo, '/') = rtrim(?1, '/') ORDER BY c.child LIMIT 1",
        [repo],
        |r| r.get(0),
    )
    .optional()?)
}

/// Sets `archived`; every row stays, so counts do not move.
pub fn archive(c: &Connection, child: &str) -> Result<(), Error> {
    let n = c.execute("UPDATE children SET archived = 1 WHERE child = ?1", [child])?;
    if n == 0 {
        return Err(no_such(child));
    }
    Ok(())
}

/// Whether `session_id` is the child's recorded session, and that session.
pub fn is_current(c: &Connection, child: &str, session_id: &str) -> Result<(bool, String), Error> {
    let current: String = c
        .query_row(
            "SELECT session_id FROM children WHERE child = ?1",
            [child],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| no_such(child))?;
    Ok((current == session_id, current))
}

pub fn record_session(
    c: &Connection,
    child: &str,
    session_id: &str,
    pid: u32,
) -> Result<(), Error> {
    let n = c.execute(
        "UPDATE children SET session_id = ?2, claude_pid = ?3 WHERE child = ?1",
        (child, session_id, pid),
    )?;
    if n == 0 {
        return Err(no_such(child));
    }
    Ok(())
}

pub fn add_fork(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    status_row(c, child)?;
    c.execute(
        "INSERT OR IGNORE INTO forks(child, session_id) VALUES (?1, ?2)",
        (child, session_id),
    )?;
    Ok(())
}

pub fn is_fork(c: &Connection, child: &str, session_id: &str) -> Result<bool, Error> {
    Ok(c.query_row(
        "SELECT count(*) FROM forks WHERE child = ?1 AND session_id = ?2",
        (child, session_id),
        |r| r.get::<_, i64>(0),
    )? > 0)
}

pub fn claude_pid(c: &Connection, child: &str) -> Result<Option<u32>, Error> {
    let pid: Option<Option<i64>> = c
        .query_row(
            "SELECT claude_pid FROM children WHERE child = ?1",
            [child],
            |r| r.get(0),
        )
        .optional()?;
    Ok(pid.flatten().and_then(|p| u32::try_from(p).ok()))
}

pub fn record_orchestrator(c: &Connection, session_id: &str, pid: u32) -> Result<(), Error> {
    c.execute(
        "INSERT INTO orchestrator(id, session_id, pid) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET session_id = excluded.session_id, pid = excluded.pid",
        (session_id, pid),
    )?;
    Ok(())
}

/// Makes `session_id` the child, and drops it from `forks` so it is
/// park-denied like any child.
pub fn adopt(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    let n = c.execute(
        "UPDATE children SET session_id = ?2 WHERE child = ?1",
        (child, session_id),
    )?;
    if n == 0 {
        return Err(no_such(child));
    }
    c.execute(
        "DELETE FROM forks WHERE child = ?1 AND session_id = ?2",
        (child, session_id),
    )?;
    Ok(())
}

fn count(c: &Connection, sql: &str, child: &str) -> Result<i64, Error> {
    Ok(c.query_row(sql, [child], |r| r.get(0))?)
}

fn untaken(c: &Connection, child: &str) -> Result<i64, Error> {
    count(
        c,
        "SELECT count(*) FROM inbox WHERE child = ?1 AND taken = 0",
        child,
    )
}

fn task_text(c: &Connection, seq: Option<i64>) -> Result<Option<String>, Error> {
    let Some(seq) = seq else { return Ok(None) };
    Ok(
        c.query_row("SELECT text FROM inbox WHERE seq = ?1", [seq], |r| r.get(0))
            .optional()?,
    )
}

fn labelled(q: &str) -> String {
    format!("{QUESTION_LABEL}: {}", cap_question(q))
}

#[derive(Debug, Serialize)]
pub struct ChildView {
    pub child: String,
    pub repo: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub untaken: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StatusView {
    pub card: String,
    pub version: i64,
    pub children: Vec<ChildView>,
}

struct Live {
    child: String,
    repo: String,
    brief: String,
    state: String,
    question: Option<String>,
    current_task: Option<i64>,
    outcome: Option<String>,
    pr_url: Option<String>,
}

fn live_children(c: &Connection) -> Result<Vec<Live>, Error> {
    let mut st = c.prepare(
        "SELECT c.child, c.repo, c.brief, s.state, s.question, s.current_task, s.outcome, s.pr_url
         FROM children c JOIN status s USING (child)
         WHERE c.archived = 0 ORDER BY c.child",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Live {
            child: r.get(0)?,
            repo: r.get(1)?,
            brief: r.get(2)?,
            state: r.get(3)?,
            question: r.get(4)?,
            current_task: r.get(5)?,
            outcome: r.get(6)?,
            pr_url: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn goal(brief: &str) -> String {
    let line = brief.lines().map(str::trim).find(|l| !l.is_empty());
    line.unwrap_or_default().chars().take(GOAL_CHARS).collect()
}

/// The card and the children, as the caller may see them. A question shows
/// only on a turn the user started, and a child sees only its own.
pub fn status(c: &Connection, id: &Identity, user_turn: bool) -> Result<StatusView, Error> {
    let record = project::get(c)?;
    let live = live_children(c)?;
    let kids: Vec<ChildLine> = live
        .iter()
        .map(|k| ChildLine {
            id: k.child.clone(),
            repo: k.repo.clone(),
            goal: goal(&k.brief),
        })
        .collect();
    let mut children = Vec::new();
    let mut me_part = None;
    for k in &live {
        let own = id.child.as_deref() == Some(k.child.as_str());
        let see_all = id.role == Role::Orchestrator;
        let mut v = ChildView {
            child: k.child.clone(),
            repo: k.repo.clone(),
            state: k.state.clone(),
            question: None,
            task: None,
            untaken: None,
            outcome: None,
            pr_url: None,
        };
        if own || see_all {
            v.task = task_text(c, k.current_task)?;
            v.untaken = Some(untaken(c, &k.child)?);
            v.question = k.question.as_deref().filter(|_| user_turn).map(labelled);
        }
        if see_all {
            v.outcome = k.outcome.clone();
            v.pr_url = k.pr_url.clone();
        }
        if own {
            me_part = Some(SelfPart {
                brief: k.brief.clone(),
                current_task: v.task.clone(),
                question: k
                    .question
                    .as_deref()
                    .filter(|_| user_turn)
                    .map(cap_question),
            });
        }
        children.push(v);
    }
    if id.role == Role::Child && me_part.is_none() {
        return Err(Error::Refused(STOPPED.into()));
    }
    Ok(StatusView {
        card: card::render(&record, &kids, me_part.as_ref()),
        version: record.version,
        children,
    })
}

#[derive(Debug, Serialize)]
pub struct ChildSnap {
    pub untaken: i64,
    pub taken: i64,
    pub newest_seq: i64,
    pub task_open: bool,
    pub parked: bool,
    pub is_current: bool,
    pub current_id: String,
    pub card_version: i64,
}

#[derive(Debug, Serialize)]
pub struct DoneNote {
    pub child: String,
    pub done_seq: i64,
}

#[derive(Debug, Serialize)]
pub struct OrchSnap {
    pub done_unnotified: Vec<DoneNote>,
    pub waiting: i64,
    pub working: i64,
    pub done: i64,
    pub untaken: BTreeMap<String, i64>,
    pub card_stale: bool,
    pub card_version: i64,
    pub is_current: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Snap {
    Child(ChildSnap),
    Orch(OrchSnap),
}

pub fn card_stale(c: &Connection, now: i64) -> Result<bool, Error> {
    let r = project::get(c)?;
    Ok(r.dones_since_card >= STALE_DONES || now - r.card_updated_at >= STALE_SECS)
}

/// What a session's watcher needs. For the orchestrator, `ack` (child to the
/// `done_seq` its wake-up announced) is written to `notified` first, to exactly
/// those values, so a newer `done` stays unannounced. A child's snapshot is a
/// read and takes no `ack`.
pub fn snapshot(
    c: &Connection,
    id: &Identity,
    session_id: &str,
    now: i64,
    ack: &BTreeMap<String, i64>,
) -> Result<Snap, Error> {
    let version = project::get(c)?.version;
    match (id.role, id.child.as_deref()) {
        (Role::Child, Some(child)) => {
            if !ack.is_empty() {
                return Err(Error::Refused("ack is for the orchestrator".into()));
            }
            let row = status_row(c, child)?;
            let (is_current, current_id) = is_current(c, child, session_id)?;
            Ok(Snap::Child(ChildSnap {
                untaken: untaken(c, child)?,
                taken: count(
                    c,
                    "SELECT count(*) FROM inbox WHERE child = ?1 AND taken = 1",
                    child,
                )?,
                newest_seq: count(
                    c,
                    "SELECT COALESCE(max(seq), 0) FROM inbox WHERE child = ?1",
                    child,
                )?,
                task_open: row.current_task.is_some(),
                parked: row.state == "parked",
                is_current,
                current_id,
                card_version: version,
            }))
        }
        (Role::Orchestrator, _) => {
            for (child, seq) in ack {
                status_row(c, child)?;
                c.execute(
                    "INSERT INTO notified(child, seq) VALUES (?1, ?2)
                     ON CONFLICT(child) DO UPDATE SET seq = excluded.seq",
                    (child, seq),
                )?;
            }
            Ok(Snap::Orch(orch_snap(c, session_id, now, version)?))
        }
        _ => Err(Error::Refused(
            "snapshot needs a child or the orchestrator".into(),
        )),
    }
}

fn orch_snap(c: &Connection, session_id: &str, now: i64, version: i64) -> Result<OrchSnap, Error> {
    let mut st = c.prepare(
        "SELECT s.child, s.done_seq FROM status s
         JOIN children ch USING (child)
         LEFT JOIN notified n USING (child)
         WHERE ch.archived = 0 AND s.done_seq IS NOT NULL AND s.done_seq > COALESCE(n.seq, 0)
         ORDER BY s.child",
    )?;
    let done_unnotified = st
        .query_map([], |r| {
            Ok(DoneNote {
                child: r.get(0)?,
                done_seq: r.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let by_state = |state: &str| -> Result<i64, Error> {
        Ok(c.query_row(
            "SELECT count(*) FROM status s JOIN children ch USING (child)
             WHERE ch.archived = 0 AND s.state = ?1",
            [state],
            |r| r.get(0),
        )?)
    };
    let mut st = c.prepare(
        "SELECT i.child, count(*) FROM inbox i JOIN children ch USING (child)
         WHERE ch.archived = 0 AND i.taken = 0 GROUP BY i.child",
    )?;
    let untaken = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let recorded: Option<String> = c
        .query_row(
            "SELECT session_id FROM orchestrator WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(OrchSnap {
        done_unnotified,
        waiting: by_state("parked")?,
        working: by_state("working")?,
        done: by_state("done")?,
        untaken,
        card_stale: card_stale(c, now)?,
        card_version: version,
        // Nothing recorded yet: the SessionStart hook may not have run.
        is_current: recorded.is_none_or(|r| r == session_id),
    })
}

/// One line, at most `QUESTION_CAP` characters: control characters and the
/// Unicode line and paragraph separators become spaces.
fn cap_question(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}' | '\u{85}') {
                ' '
            } else {
                c
            }
        })
        .take(QUESTION_CAP)
        .collect()
}

/// One line per live child whose `seq` moved past `seen`, then marks them
/// seen. A question is child-authored, so it is marked untrusted and cut.
pub fn orch_context(c: &Connection) -> Result<Vec<String>, Error> {
    let mut st = c.prepare(
        "SELECT s.child, s.seq, s.state, s.question, s.outcome, s.pr_url
         FROM status s JOIN children ch USING (child)
         LEFT JOIN seen USING (child)
         WHERE ch.archived = 0 AND s.seq > COALESCE(seen.seq, 0) ORDER BY s.child",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut lines = Vec::new();
    for (child, seq, state, question, outcome, pr_url) in rows {
        let mut line = format!("{child} · {state}");
        match (state.as_str(), question, outcome) {
            ("parked", Some(q), _) => {
                let q = cap_question(&q);
                line.push_str(&format!(" · [untrusted, from child {child}] {q}"));
            }
            ("done", _, Some(o)) => {
                line.push_str(&format!(" · {o}"));
                if let Some(u) = pr_url {
                    line.push_str(&format!(" {u}"));
                }
            }
            _ => {}
        }
        lines.push(line);
        c.execute(
            "INSERT INTO seen(child, seq) VALUES (?1, ?2)
             ON CONFLICT(child) DO UPDATE SET seq = excluded.seq",
            (&child, seq),
        )?;
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Mode, Store, Touch};

    fn fresh() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let x = tmp.path().display().to_string();
        let env = move |k: &str| (k == "XDG_DATA_HOME").then(|| x.clone());
        project::init(&env, "p").unwrap();
        let mut s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        for id in ["c1", "c2"] {
            s.write(Touch::CHILDREN, |tx| add(tx, id)).unwrap();
        }
        (tmp, s)
    }

    fn add(c: &Connection, id: &str) -> Result<(), Error> {
        add_child(
            c,
            &Child {
                child: id.into(),
                repo: format!("/r/{id}"),
                worktree: format!("/r/{id}/.worktrees/{id}"),
                branch: format!("crook/{id}"),
                brief: "brief".into(),
                session_id: format!("s-{id}"),
            },
        )
    }

    fn w<T>(s: &mut Store, f: impl FnOnce(&Connection) -> Result<T, Error>) -> Result<T, Error> {
        s.write(Touch::STATUS, |tx| f(tx))
    }

    fn row(s: &Store, child: &str) -> (String, i64, Option<i64>, Option<i64>) {
        s.read(|c| {
            Ok(c.query_row(
                "SELECT state, seq, current_task, done_seq FROM status WHERE child = ?1",
                [child],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap()
    }

    fn finish(s: &mut Store, child: &str) {
        w(s, |c| task(c, child, "t")).unwrap();
        w(s, |c| take_task(c, child)).unwrap();
        w(s, |c| done(c, child, Outcome::Ok, None)).unwrap();
    }

    fn refused(r: Result<impl std::fmt::Debug, Error>) -> String {
        r.unwrap_err().to_string()
    }

    #[test]
    fn add_child_inserts_both_rows_and_refuses_duplicates() {
        let (_t, mut s) = fresh();
        assert_eq!(row(&s, "c1"), ("working".into(), 0, None, None));
        assert!(refused(w(&mut s, |c| add(c, "c1"))).contains("already exists"));
        assert!(refused(w(&mut s, |c| add(c, "Bad Name"))).contains("invalid child"));
    }

    #[test]
    fn take_task_order() {
        let (_t, mut s) = fresh();
        assert!(refused(w(&mut s, |c| take_task(c, "c1"))).contains("no tasks"));
        let a = w(&mut s, |c| task(c, "c1", "first")).unwrap();
        let b = w(&mut s, |c| task(c, "c1", "second")).unwrap();
        w(&mut s, |c| task(c, "c2", "other")).unwrap();
        assert!(b > a);
        assert_eq!(
            w(&mut s, |c| take_task(c, "c1")).unwrap(),
            (a, "first".into())
        );
        assert!(refused(w(&mut s, |c| take_task(c, "c1"))).contains("task open"));
        w(&mut s, |c| done(c, "c1", Outcome::Ok, None)).unwrap();
        assert_eq!(
            w(&mut s, |c| take_task(c, "c1")).unwrap(),
            (b, "second".into())
        );
        w(&mut s, |c| need(c, "c1", "q")).unwrap();
        w(&mut s, |c| task(c, "c1", "third")).unwrap();
        assert!(refused(w(&mut s, |c| take_task(c, "c1"))).contains("parked"));
        assert!(refused(w(&mut s, |c| take_task(c, "nobody"))).contains("nobody"));
        assert!(refused(w(&mut s, |c| task(c, "c1", "  "))).contains("empty"));
        assert!(refused(w(&mut s, |c| task(c, "nobody", "x"))).contains("nobody"));
    }

    #[test]
    fn done_clears_task_and_counts() {
        let (_t, mut s) = fresh();
        w(&mut s, |c| task(c, "c1", "t")).unwrap();
        w(&mut s, |c| take_task(c, "c1")).unwrap();
        assert!(row(&s, "c1").2.is_some());
        let url = "https://github.com/o/r/pull/7";
        w(&mut s, |c| done(c, "c1", Outcome::Ok, Some(url))).unwrap();
        assert_eq!(row(&s, "c1"), ("done".into(), 1, None, Some(1)));
        let (outcome, pr): (String, String) = s
            .read(|c| {
                Ok(c.query_row(
                    "SELECT outcome, pr_url FROM status WHERE child = 'c1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((outcome.as_str(), pr.as_str()), ("ok", url));
        let dones = |s: &Store| s.read(|c| Ok(project::get(c)?.dones_since_card)).unwrap();
        assert_eq!(dones(&s), 1);
        w(&mut s, |c| task(c, "c1", "t2")).unwrap();
        w(&mut s, |c| take_task(c, "c1")).unwrap();
        assert_eq!(row(&s, "c1").0, "working", "a later take-task works again");
        assert_eq!(row(&s, "c1").3, Some(1), "done_seq stays");
        assert!(
            refused(w(&mut s, |c| done(
                c,
                "c1",
                Outcome::Ok,
                Some("http://x/o/r/pull/1")
            )))
            .contains("pr_url")
        );
        assert_eq!(dones(&s), 1, "a refused done counts nothing");
        w(&mut s, |c| done(c, "c1", Outcome::Failed, None)).unwrap();
        assert_eq!(dones(&s), 2);
        let before = row(&s, "c1");
        assert!(refused(w(&mut s, |c| done(c, "c1", Outcome::Ok, None))).contains("no task open"));
        assert_eq!(
            (row(&s, "c1"), dones(&s)),
            (before, 2),
            "a repeated done changes nothing"
        );
        w(&mut s, |c| need(c, "c2", "q")).unwrap();
        assert!(refused(w(&mut s, |c| done(c, "c2", Outcome::Ok, None))).contains("parked"));
    }

    #[test]
    fn pr_url_pattern() {
        for ok in [
            "https://github.com/o/r/pull/1",
            "https://git.example.test/a/b/pull/123",
        ] {
            assert!(valid_pr_url(ok), "{ok}");
        }
        for bad in [
            "",
            "https://github.com/o/r/pull/",
            "https://github.com/o/r/pull/1/",
            "https://github.com/o/r/pull/1/files",
            "https://github.com/o/pull/1",
            "https://github.com//r/pull/1",
            "https://github.com/o/r/pull/1 x",
            "https://github.com/o/r/pull/1\n",
            "https://github.com/o/r/pulls/1",
            "ftp://github.com/o/r/pull/1",
            "https://github.com/o/r/pull/-1",
        ] {
            assert!(!valid_pr_url(bad), "{bad:?}");
        }
    }

    #[test]
    fn need_parks_lift_unparks() {
        let (_t, mut s) = fresh();
        w(&mut s, |c| need(c, "c1", "which?")).unwrap();
        assert!(s.read(|c| parked(c, "c1")).unwrap());
        assert_eq!(row(&s, "c1").1, 1);
        assert!(w(&mut s, |c| lift(c, "c1")).unwrap());
        assert_eq!(row(&s, "c1").1, 2);
        assert!(!w(&mut s, |c| lift(c, "c1")).unwrap());
        assert_eq!(row(&s, "c1").1, 2, "a no-op lift does not bump");
        assert!(!s.read(|c| parked(c, "c1")).unwrap());
        assert!(refused(w(&mut s, |c| need(c, "c1", " "))).contains("empty"));
    }

    #[test]
    fn adopt_removes_fork() {
        let (_t, mut s) = fresh();
        w(&mut s, |c| add_fork(c, "c1", "F")).unwrap();
        w(&mut s, |c| add_fork(c, "c1", "F")).unwrap();
        let forks = |s: &Store| -> i64 {
            s.read(|c| Ok(c.query_row("SELECT count(*) FROM forks", [], |r| r.get(0))?))
                .unwrap()
        };
        assert_eq!(forks(&s), 1);
        assert_eq!(
            s.read(|c| is_current(c, "c1", "F")).unwrap(),
            (false, "s-c1".into())
        );
        w(&mut s, |c| adopt(c, "c1", "F")).unwrap();
        assert_eq!(
            s.read(|c| is_current(c, "c1", "F")).unwrap(),
            (true, "F".into())
        );
        assert_eq!(forks(&s), 0);
        assert!(refused(w(&mut s, |c| adopt(c, "nobody", "F"))).contains("nobody"));
        w(&mut s, |c| record_session(c, "c1", "G", 42)).unwrap();
        let pid: i64 = s
            .read(|c| {
                Ok(c.query_row(
                    "SELECT claude_pid FROM children WHERE child='c1'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(pid, 42);
    }

    fn orch() -> Identity {
        Identity {
            slug: "p".into(),
            role: Role::Orchestrator,
            child: None,
        }
    }

    fn snap(s: &mut Store, ack: &[(&str, i64)]) -> OrchSnap {
        let ack: BTreeMap<String, i64> = ack.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        match s
            .write(Touch::empty(), |tx| {
                snapshot(tx, &orch(), "o1", project::now(), &ack)
            })
            .unwrap()
        {
            Snap::Orch(o) => o,
            Snap::Child(_) => unreachable!(),
        }
    }

    #[test]
    fn orch_snapshot() {
        let (_t, mut s) = fresh();
        w(&mut s, |c| done(c, "c1", Outcome::Ok, None)).unwrap();
        let o = snap(&mut s, &[]);
        assert_eq!(o.done_unnotified.len(), 1);
        assert_eq!((o.working, o.done, o.waiting), (1, 1, 0));
        assert_eq!(
            snap(&mut s, &[("c1", 0)]).done_unnotified.len(),
            1,
            "older ack"
        );
        assert!(snap(&mut s, &[("c1", 1)]).done_unnotified.is_empty());
        w(&mut s, |c| need(c, "c1", "q")).unwrap();
        w(&mut s, |c| lift(c, "c1")).unwrap();
        assert!(snap(&mut s, &[]).done_unnotified.is_empty());
        assert!(
            refused(s.write(Touch::empty(), |tx| {
                snapshot(
                    tx,
                    &orch(),
                    "o1",
                    0,
                    &BTreeMap::from([("nobody".to_string(), 1)]),
                )
            }))
            .contains("nobody")
        );
    }

    #[test]
    fn orchestrator_is_current_unless_another_session_is_recorded() {
        let (_t, mut s) = fresh();
        assert!(snap(&mut s, &[]).is_current, "nothing recorded yet");
        s.write(Touch::empty(), |tx| {
            tx.execute(
                "INSERT INTO orchestrator(id, session_id) VALUES (1, 'o1')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert!(snap(&mut s, &[]).is_current);
        s.write(Touch::empty(), |tx| {
            tx.execute("UPDATE orchestrator SET session_id = 'o2'", [])?;
            Ok(())
        })
        .unwrap();
        assert!(!snap(&mut s, &[]).is_current);
    }

    #[test]
    fn card_version() {
        let (_t, mut s) = fresh();
        let id = Identity {
            slug: "p".into(),
            role: Role::Child,
            child: Some("c1".into()),
        };
        let now = project::now();
        let stale = |s: &Store, at: i64| s.read(|c| card_stale(c, at)).unwrap();
        assert!(!stale(&s, now));
        assert!(stale(&s, now + STALE_SECS + 5));
        assert!(!stale(&s, now + STALE_SECS - 100));
        for _ in 0..2 {
            finish(&mut s, "c1");
        }
        assert!(!stale(&s, now));
        finish(&mut s, "c1");
        assert!(stale(&s, now), "three dones");
        match s
            .read(|c| snapshot(c, &id, "s-c1", now, &BTreeMap::new()))
            .unwrap()
        {
            Snap::Child(c) => assert_eq!((c.card_version, c.is_current), (1, true)),
            Snap::Orch(_) => unreachable!(),
        }
        assert!(
            refused(s.read(|c| snapshot(
                c,
                &id,
                "s-c1",
                now,
                &BTreeMap::from([("c1".to_string(), 1)])
            )))
            .contains("ack")
        );
    }

    #[test]
    fn orch_context() {
        let (_t, mut s) = fresh();
        let long = "x".repeat(500);
        w(&mut s, |c| need(c, "c1", &long)).unwrap();
        let lines = w(&mut s, super::orch_context).unwrap();
        assert_eq!(lines.len(), 1);
        let want = format!("[untrusted, from child c1] {}", "x".repeat(300));
        assert!(lines[0].ends_with(&want), "{}", lines[0]);
        assert!(w(&mut s, super::orch_context).unwrap().is_empty());
        w(&mut s, |c| need(c, "c2", "multi\nline")).unwrap();
        let lines = w(&mut s, super::orch_context).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("multi line") && !lines[0].contains('\n'),
            "{lines:?}"
        );
    }

    #[test]
    fn archived_kept_in_counts() {
        let (_t, mut s) = fresh();
        w(&mut s, |c| archive(c, "c1")).unwrap();
        let n: i64 = s
            .read(|c| Ok(c.query_row("SELECT count(*) FROM children", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(snap(&mut s, &[]).working, 1);
        assert!(refused(guard_err(&s, "c1")).contains("stopped"));
        assert!(refused(w(&mut s, |c| archive(c, "nobody"))).contains("nobody"));
        w(&mut s, |c| done(c, "c1", Outcome::Ok, None)).unwrap();
        assert!(
            snap(&mut s, &[]).done_unnotified.is_empty(),
            "stopped children are not announced"
        );
    }

    fn guard_err(s: &Store, child: &str) -> Result<(), Error> {
        s.read(|c| guard(c, child, "s", false))
    }

    #[test]
    fn question_is_one_line_and_capped() {
        let q = format!(
            "a\u{2028}b\u{2029}c\u{85}d\te\u{0}f\r\ng{}",
            "é".repeat(400)
        );
        let out = cap_question(&q);
        assert_eq!(out.chars().count(), QUESTION_CAP);
        assert!(out.starts_with("a b c d e f  g"), "{out:?}");
        assert!(
            !out.chars()
                .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        );
        let (_t, mut s) = fresh();
        let long = "q".repeat(500);
        w(&mut s, |c| need(c, "c1", &long)).unwrap();
        let id = orch();
        let v = s.read(|c| status(c, &id, true)).unwrap();
        let shown = v.children[0].question.clone().unwrap();
        assert_eq!(shown, format!("{QUESTION_LABEL}: {}", "q".repeat(300)));
        let stored: String = s
            .read(|c| {
                Ok(
                    c.query_row("SELECT question FROM status WHERE child = 'c1'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(stored.len(), 500, "the stored question stays whole");
    }
}
