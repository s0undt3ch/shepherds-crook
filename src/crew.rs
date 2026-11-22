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

/// A row of `worktrees`: one git worktree crook made for a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub child: String,
    pub repo: String,
    pub path: String,
    pub branch: String,
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
    hold(c, &ch.child, &ch.session_id)
}

/// Records a worktree for a child. A path already recorded is refused.
pub fn add_worktree(c: &Connection, w: &Worktree) -> Result<(), Error> {
    let n = c.execute(
        "INSERT OR IGNORE INTO worktrees(child, repo, path, branch, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        (&w.child, &w.repo, &w.path, &w.branch, project::now()),
    )?;
    if n == 0 {
        return Err(Error::Refused(format!(
            "worktree {} is already recorded",
            w.path
        )));
    }
    Ok(())
}

/// The recorded worktrees, oldest first: one child's, or everyone's.
pub fn worktrees(c: &Connection, child: Option<&str>) -> Result<Vec<Worktree>, Error> {
    let mut st = c.prepare(
        "SELECT child, repo, path, branch FROM worktrees
         WHERE ?1 IS NULL OR child = ?1 ORDER BY rowid",
    )?;
    let rows = st.query_map([child], |r| {
        Ok(Worktree {
            child: r.get(0)?,
            repo: r.get(1)?,
            path: r.get(2)?,
            branch: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Forgets a worktree row, once the worktree itself is gone.
pub fn remove_worktree_row(c: &Connection, path: &str) -> Result<(), Error> {
    c.execute("DELETE FROM worktrees WHERE path = ?1", [path])?;
    Ok(())
}

/// The canonical repo a child works in.
pub fn child_repo(c: &Connection, child: &str) -> Result<String, Error> {
    c.query_row("SELECT repo FROM children WHERE child = ?1", [child], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or_else(|| no_such(child))
}

/// How many children are not archived.
pub fn live_count(c: &Connection) -> Result<i64, Error> {
    Ok(c.query_row(
        "SELECT count(*) FROM children WHERE archived = 0",
        [],
        |r| r.get(0),
    )?)
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
    hold(c, child, session_id)
}

pub fn add_fork(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    status_row(c, child)?;
    refuse_held(c, child, session_id)?;
    c.execute(
        "INSERT OR IGNORE INTO forks(child, session_id) VALUES (?1, ?2)",
        (child, session_id),
    )?;
    Ok(())
}

/// Whether the child's row has ever held `session_id`.
pub fn held(c: &Connection, child: &str, session_id: &str) -> Result<bool, Error> {
    Ok(c.query_row(
        "SELECT count(*) FROM child_sessions WHERE child = ?1 AND session_id = ?2",
        (child, session_id),
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// Records that the child's row holds `session_id`, for good.
fn hold(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    c.execute(
        "INSERT OR IGNORE INTO child_sessions(child, session_id) VALUES (?1, ?2)",
        (child, session_id),
    )?;
    Ok(())
}

/// A held id is the child's, so the hook can never make it a fork or adopt it as one.
fn refuse_held(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    if held(c, child, session_id)? {
        return Err(Error::Refused(format!(
            "session {session_id} has been {child} itself, so it is not a fork"
        )));
    }
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

/// Forgets the child's claude pid, so a restarted claude records itself: the
/// SessionStart guard refuses a new session while the recorded pid is a live claude.
pub fn clear_claude_pid(c: &Connection, child: &str) -> Result<(), Error> {
    let n = c.execute(
        "UPDATE children SET claude_pid = NULL WHERE child = ?1",
        [child],
    )?;
    if n == 0 {
        return Err(no_such(child));
    }
    Ok(())
}

/// What `crook resume` needs of a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resumable {
    pub worktree: String,
    pub session_id: String,
    pub claude_pid: Option<u32>,
    pub archived: bool,
}

pub fn resumable(c: &Connection, child: &str) -> Result<Resumable, Error> {
    c.query_row(
        "SELECT worktree, session_id, claude_pid, archived FROM children WHERE child = ?1",
        [child],
        |r| {
            Ok(Resumable {
                worktree: r.get(0)?,
                session_id: r.get(1)?,
                claude_pid: r
                    .get::<_, Option<i64>>(2)?
                    .and_then(|p| u32::try_from(p).ok()),
                archived: r.get::<_, i64>(3)? != 0,
            })
        },
    )
    .optional()?
    .ok_or_else(|| no_such(child))
}

/// The orchestrator row: its session id and pid, when a session has recorded them.
pub fn orchestrator(c: &Connection) -> Result<(Option<String>, Option<u32>), Error> {
    let row: Option<(Option<String>, Option<i64>)> = c
        .query_row(
            "SELECT session_id, pid FROM orchestrator WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.map_or((None, None), |(s, p)| {
        (s, p.and_then(|p| u32::try_from(p).ok()))
    }))
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
/// park-denied like any child. Any id, held or not: the user's `/crook-adopt`.
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
    hold(c, child, session_id)
}

/// `adopt` for the hook, which takes a fork as the child: a held id is
/// refused, since a forged SessionStart could otherwise lift a park.
pub fn adopt_fork(c: &Connection, child: &str, session_id: &str) -> Result<(), Error> {
    refuse_held(c, child, session_id)?;
    adopt(c, child, session_id)
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
    let all_worktrees = worktrees(c, None)?;
    let kids: Vec<ChildLine> = live
        .iter()
        .map(|k| ChildLine {
            id: k.child.clone(),
            repo: k.repo.clone(),
            goal: goal(&k.brief),
            worktrees: all_worktrees
                .iter()
                .filter(|w| w.child == k.child)
                .map(|w| w.path.clone())
                .collect(),
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
mod tests;
