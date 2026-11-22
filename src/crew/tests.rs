use super::*;
use crate::store::{Mode, Store, Touch};

fn fresh() -> (tempfile::TempDir, Store) {
    let tmp = tempfile::tempdir().unwrap();
    let x = tmp.path().display().to_string();
    let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
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

fn wt(child: &str, path: &str) -> Worktree {
    Worktree {
        child: child.into(),
        repo: format!("/r/{child}"),
        path: path.into(),
        branch: format!("crook/{child}"),
    }
}

#[test]
fn worktree_rows_add_list_and_remove() {
    let (_t, mut s) = fresh();
    for (c, p) in [("c1", "/a"), ("c2", "/b"), ("c1", "/c")] {
        w(&mut s, |tx| add_worktree(tx, &wt(c, p))).unwrap();
    }
    let paths = |s: &Store, who| -> Vec<String> {
        s.read(|c| worktrees(c, who))
            .unwrap()
            .into_iter()
            .map(|w| w.path)
            .collect()
    };
    assert_eq!(paths(&s, Some("c1")), ["/a", "/c"]);
    assert_eq!(paths(&s, None), ["/a", "/b", "/c"]);
    assert!(refused(w(&mut s, |tx| add_worktree(tx, &wt("c2", "/a")))).contains("already"));
    w(&mut s, |tx| remove_worktree_row(tx, "/a")).unwrap();
    assert_eq!(paths(&s, None), ["/b", "/c"]);
}

#[test]
fn card_lists_worktrees() {
    let (_t, mut s) = fresh();
    for p in [
        "/r/c1/.worktrees/p/c1",
        "/r/c1/.worktrees/p/c1.a",
        "/r/c1/.worktrees/p/c1.b",
    ] {
        w(&mut s, |tx| add_worktree(tx, &wt("c1", p))).unwrap();
    }
    let id = Identity {
        slug: "p".into(),
        role: Role::Orchestrator,
        child: None,
    };
    let v = s.read(|c| status(c, &id, false)).unwrap();
    let lines: Vec<&str> = v.card.lines().filter(|l| l.contains("c1 · ")).collect();
    assert_eq!(lines.len(), 1, "{}", v.card);
    for p in ["c1,", "c1.a,", "c1.b"] {
        assert!(
            lines[0].contains(&format!("/r/c1/.worktrees/p/{p}")),
            "{}",
            lines[0]
        );
    }
    assert!(v.card.lines().count() <= card::MAX_LINES);
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
fn a_held_session_id_is_never_a_fork() {
    let (_t, mut s) = fresh();
    assert!(s.read(|c| held(c, "c1", "s-c1")).unwrap());
    w(&mut s, |c| record_session(c, "c1", "s-new", 42)).unwrap();
    for id in ["s-c1", "s-new"] {
        assert!(s.read(|c| held(c, "c1", id)).unwrap(), "{id}");
        assert!(refused(w(&mut s, |c| add_fork(c, "c1", id))).contains(id));
        assert!(refused(w(&mut s, |c| adopt_fork(c, "c1", id))).contains(id));
    }
    // The user's `/crook-adopt` may take a held id back.
    w(&mut s, |c| adopt(c, "c1", "s-c1")).unwrap();
    assert!(s.read(|c| is_current(c, "c1", "s-c1")).unwrap().0);
    assert!(!s.read(|c| held(c, "c1", "other")).unwrap());
    assert!(!s.read(|c| held(c, "c2", "s-c1")).unwrap());
    w(&mut s, |c| adopt(c, "c1", "B")).unwrap();
    assert!(s.read(|c| held(c, "c1", "B")).unwrap());
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
