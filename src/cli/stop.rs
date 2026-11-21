//! `crook stop <child>`: archives a child, stops its session and removes its worktrees.

use std::path::Path;

use crate::launch::child::{Launcher, archive_and_stop, verify_worktree};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew};

use super::address;

/// Stops `arg` (`<child>` or `<slug>/<child>`). It refuses, changing nothing,
/// when a recorded worktree fails `verify_worktree` or has uncommitted changes.
/// Otherwise it archives the child and stops its session, and removes each
/// worktree by its recorded path and forgets its row, but only once the session
/// is confirmed gone: when it may still run, the worktrees stay and `stop` says
/// why, to be run again.
pub fn run(env: Env, arg: &str, run: &dyn Launcher) -> Result<String, Error> {
    let (slug, child) = address::resolve(env, arg)?;
    let mut store = Store::open(env, &slug, Mode::ReadWrite)?;
    let (repo, rows) = store.read(|c| {
        Ok((
            crew::child_repo(c, &child)?,
            crew::worktrees(c, Some(&child))?,
        ))
    })?;

    // Every refusal comes before the first change.
    let mut checked = Vec::new();
    for r in &rows {
        let real = verify_worktree(run, &repo, &slug, &child, Path::new(&r.path))?;
        checked.push((r.path.as_str(), real));
    }
    let mut dirty = Vec::new();
    for (path, real) in &checked {
        if let Some(real) = real
            && run.worktree_dirty(real)?
        {
            dirty.push(*path);
        }
    }
    if !dirty.is_empty() {
        return Err(Error::Refused(format!(
            "child {child} has uncommitted changes in: {}; nothing was changed",
            dirty.join(", ")
        )));
    }

    let stopped = archive_and_stop(&mut store, &child, run)?;
    if !stopped.session_gone() {
        return Ok(format!(
            "archived {child}, but its session may still run ({}); its {} worktrees were kept: \
             stop the session, then run crook stop {child} again",
            stopped.describe(),
            rows.len()
        ));
    }
    let mut removed = 0;
    let mut failed = Vec::new();
    for (path, real) in &checked {
        // A worktree already gone from disk has nothing to remove.
        let res = match real {
            Some(real) => run.worktree_remove(Path::new(&repo), real),
            None => Ok(()),
        };
        match res {
            Ok(()) => {
                store.write(Touch::CHILDREN, |tx| crew::remove_worktree_row(tx, path))?;
                removed += 1;
            }
            Err(e) => failed.push(format!("{path}: {e}")),
        }
    }
    if !failed.is_empty() {
        return Err(Error::Refused(format!(
            "child {child} is archived ({}), but not every worktree was removed: {}",
            stopped.describe(),
            failed.join("; ")
        )));
    }
    Ok(format!(
        "stopped {child}: {}; removed {removed} worktrees",
        stopped.describe()
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::agents::Agent;
    use crate::launch::child::fake::{Call, Fake};
    use crate::launch::child::{SpawnReq, WorktreeReq, add_worktree, spawn_child};
    use crate::launch::fixture::Fx;
    use crate::project::{self, ItemKind};

    struct W {
        fx: Fx,
        repo: PathBuf,
    }

    /// Project `demo` with children `c1` (three worktrees) and `c2` (one), each
    /// worktree a real directory, as `git` would have made it.
    fn w() -> W {
        let fx = Fx::new();
        fx.init("demo");
        let repo = std::fs::canonicalize(fx.tmp.path()).unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, ItemKind::Repo, &repo.display().to_string())
            })
            .unwrap();
        let w = W { fx, repo };
        let run = Fake::default();
        let env = w.fx.env();
        for child in ["c1", "c2"] {
            let mut s = w.fx.store("demo");
            spawn_child(
                &mut s,
                &SpawnReq {
                    env: &env,
                    slug: "demo".into(),
                    child: child.into(),
                    repo: w.repo.display().to_string(),
                    brief: "b".into(),
                    branch: None,
                    session_id: format!("sid-{child}"),
                    base_env: Vec::new(),
                    crook_bin: "/abs/crook".into(),
                },
                &run,
            )
            .unwrap();
        }
        let mut s = w.fx.store("demo");
        for name in ["a", "b"] {
            add_worktree(
                &mut s,
                &WorktreeReq {
                    child: "c1".into(),
                    session_id: "sid-c1".into(),
                    name: name.into(),
                    branch: None,
                },
                &run,
            )
            .unwrap();
        }
        for row in s.read(|c| crew::worktrees(c, None)).unwrap() {
            std::fs::create_dir_all(&row.path).unwrap();
        }
        w
    }

    impl W {
        fn paths(&self, child: &str) -> Vec<PathBuf> {
            self.fx
                .store("demo")
                .read(|c| crew::worktrees(c, Some(child)))
                .unwrap()
                .into_iter()
                .map(|r| PathBuf::from(r.path))
                .collect()
        }

        fn archived(&self, child: &str) -> bool {
            self.fx
                .store("demo")
                .read(|c| crew::resumable(c, child))
                .unwrap()
                .archived
        }

        fn stop(&self, arg: &str, run: &Fake) -> Result<String, Error> {
            run_stop(self, arg, run)
        }
    }

    fn run_stop(w: &W, arg: &str, fake: &Fake) -> Result<String, Error> {
        super::run(&w.fx.env(), arg, fake)
    }

    fn bg_agent(sid: &str) -> Agent {
        Agent {
            session_id: sid.into(),
            pid: None,
            cwd: "/x".into(),
            kind: "background".into(),
            state: None,
        }
    }

    #[test]
    fn cli_stop_refuses_dirty() {
        let w = w();
        let all = w.paths("c1");
        assert_eq!(all.len(), 3);
        let run = Fake {
            dirty: [all[0].clone(), all[2].clone()].into(),
            agents: vec![bg_agent("sid-c1")],
            ..Fake::default()
        };
        let e = w.stop("c1", &run).unwrap_err().to_string();
        assert!(e.contains(&all[0].display().to_string()), "{e}");
        assert!(e.contains(&all[2].display().to_string()), "{e}");
        assert!(!e.contains(&all[1].display().to_string()), "{e}");
        // Nothing changed.
        assert!(!w.archived("c1"));
        assert_eq!(w.paths("c1"), all);
        assert!(run.calls().is_empty(), "{:?}", run.calls());
        assert!(all.iter().all(|p| p.is_dir()));
    }

    #[test]
    fn stop_removes_all_child_worktrees() {
        let w = w();
        let c1 = w.paths("c1");
        let c2 = w.paths("c2");
        let run = Fake {
            agents: vec![bg_agent("sid-c1")],
            ..Fake::default()
        };
        let msg = w.stop("demo/c1", &run).unwrap();
        assert!(msg.contains("c1") && msg.contains("3 worktrees"), "{msg}");
        let repo = w.repo.clone();
        let mut want = vec![Call::StopBg("sid-c1".into())];
        want.extend(c1.iter().map(|p| Call::Remove(repo.clone(), p.clone())));
        assert_eq!(run.calls(), want);
        assert!(w.archived("c1"));
        assert!(w.paths("c1").is_empty(), "rows deleted");
        assert_eq!(w.paths("c2"), c2, "another child's worktrees stay");
        assert!(!w.archived("c2"));
    }

    #[test]
    fn stop_takes_a_bare_child_name() {
        let w = w();
        let run = Fake::default();
        w.stop("c2", &run).unwrap();
        assert!(w.archived("c2"));
        assert!(w.paths("c2").is_empty());
    }

    #[test]
    fn stop_refuses_a_recorded_path_outside_the_project_worktrees() {
        for bad in ["/elsewhere/x".to_string(), "/".to_string()] {
            bad_path(&bad);
        }
        let w0 = w();
        let repo = w0.repo.display().to_string();
        for bad in [
            format!("{repo}/.worktrees/other/c1"),
            format!("{repo}/.worktrees/demo"),
            format!("{repo}/.worktrees/demo/../../x"),
            format!("{repo}/src"),
        ] {
            bad_path(&bad);
        }
    }

    fn bad_path(bad: &str) {
        let w = w();
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                crew::add_worktree(
                    tx,
                    &crew::Worktree {
                        child: "c1".into(),
                        repo: w.repo.display().to_string(),
                        path: bad.into(),
                        branch: "b".into(),
                    },
                )
            })
            .unwrap();
        let before = w.paths("c1");
        let run = Fake {
            agents: vec![bg_agent("sid-c1")],
            ..Fake::default()
        };
        let e = w.stop("c1", &run).unwrap_err().to_string();
        assert!(e.contains(bad), "{bad}: {e}");
        assert!(
            run.calls().is_empty(),
            "{bad}: never passed on: {:?}",
            run.calls()
        );
        assert!(!w.archived("c1"), "{bad}");
        assert_eq!(w.paths("c1"), before, "{bad}");
    }

    #[test]
    fn stop_drops_the_row_of_a_worktree_already_gone() {
        let w = w();
        let all = w.paths("c1");
        std::fs::remove_dir_all(&all[1]).unwrap();
        let run = Fake::default();
        w.stop("c1", &run).unwrap();
        let removed: Vec<_> = run
            .calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Remove(_, p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(removed, [all[0].clone(), all[2].clone()]);
        assert!(w.paths("c1").is_empty());
    }

    /// Runs `stop c1` after `tamper`, and checks it was refused whole: named,
    /// no worktree removed, no session stopped, child not archived, rows kept.
    fn refused_after(listed: Option<Vec<PathBuf>>, tamper: impl FnOnce(&W)) {
        let w = w();
        tamper(&w);
        let before = w.paths("c1");
        let run = Fake {
            agents: vec![bg_agent("sid-c1")],
            listed,
            ..Fake::default()
        };
        let e = w.stop("c1", &run).unwrap_err().to_string();
        assert!(e.contains("not removable"), "{e}");
        assert!(run.calls().is_empty(), "never passed on: {:?}", run.calls());
        assert!(!w.archived("c1"));
        assert_eq!(w.paths("c1"), before);
    }

    fn link(from: &Path, to: &Path) {
        std::os::unix::fs::symlink(to, from).unwrap();
    }

    #[test]
    fn stop_refuses_a_symlinked_leaf() {
        // c1's path points at c2's real worktree: status there is clean, and
        // removing it would delete another child's work.
        refused_after(None, |w| {
            let c1 = w.repo.join(".worktrees/demo/c1");
            std::fs::remove_dir_all(&c1).unwrap();
            link(&c1, &w.repo.join(".worktrees/demo/c2"));
        });
    }

    #[test]
    fn stop_refuses_a_symlinked_slug_directory() {
        refused_after(None, |w| {
            let wt = w.repo.join(".worktrees");
            std::fs::rename(wt.join("demo"), w.repo.join("elsewhere")).unwrap();
            link(&wt.join("demo"), &w.repo.join("elsewhere"));
        });
    }

    #[test]
    fn stop_refuses_a_symlinked_worktrees_directory() {
        refused_after(None, |w| {
            std::fs::rename(w.repo.join(".worktrees"), w.repo.join("wt-real")).unwrap();
            link(&w.repo.join(".worktrees"), &w.repo.join("wt-real"));
        });
    }

    #[test]
    fn stop_refuses_another_projects_directory() {
        refused_after(None, |w| {
            let p = w.repo.join(".worktrees/demo2/c1");
            std::fs::create_dir_all(&p).unwrap();
            add_row(w, "c1", &p);
        });
    }

    #[test]
    fn stop_refuses_another_children_worktree() {
        refused_after(None, |w| {
            // The row for c1 names c2's directory.
            let c2 = w.paths("c2").remove(0);
            w.fx.store("demo")
                .write(Touch::CHILDREN, |tx| {
                    crew::remove_worktree_row(tx, &c2.display().to_string())?;
                    Ok(())
                })
                .unwrap();
            add_row(w, "c1", &c2);
        });
    }

    #[test]
    fn stop_refuses_a_worktree_git_does_not_list() {
        refused_after(Some(Vec::new()), |_| {});
    }

    fn add_row(w: &W, child: &str, path: &Path) {
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                crew::add_worktree(
                    tx,
                    &crew::Worktree {
                        child: child.into(),
                        repo: w.repo.display().to_string(),
                        path: path.display().to_string(),
                        branch: "b".into(),
                    },
                )
            })
            .unwrap();
    }

    #[test]
    fn stop_keeps_the_worktrees_while_the_session_may_still_run() {
        let interactive = |pid| Agent {
            session_id: "sid-c1".into(),
            pid,
            cwd: "/x".into(),
            kind: "interactive".into(),
            state: None,
        };
        let cases = [
            (
                "agents fails",
                Fake {
                    agents_fail: true,
                    ..Fake::default()
                },
            ),
            (
                "pid mismatch",
                Fake {
                    agents: vec![interactive(Some(9))],
                    ..Fake::default()
                },
            ),
            (
                "pid missing",
                Fake {
                    agents: vec![interactive(None)],
                    ..Fake::default()
                },
            ),
            (
                "unknown kind",
                Fake {
                    agents: vec![Agent {
                        kind: "daemon".into(),
                        ..interactive(Some(1))
                    }],
                    ..Fake::default()
                },
            ),
        ];
        for (name, run) in cases {
            let w = w();
            w.fx.store("demo")
                .write(Touch::CHILDREN, |tx| {
                    tx.execute("UPDATE children SET claude_pid = 1 WHERE child = 'c1'", [])?;
                    Ok(())
                })
                .unwrap();
            let before = w.paths("c1");
            let msg = w.stop("c1", &run).unwrap();
            assert!(
                msg.contains("may still run") && msg.contains("again"),
                "{name}: {msg}"
            );
            assert!(w.archived("c1"), "{name}");
            assert_eq!(w.paths("c1"), before, "{name}: rows kept");
            assert!(
                !run.calls().iter().any(|c| matches!(c, Call::Remove(..))),
                "{name}: {:?}",
                run.calls()
            );
            assert!(before.iter().all(|p| p.is_dir()), "{name}");
        }
    }

    #[test]
    fn stop_unknown_child_is_refused() {
        let w = w();
        assert!(w.stop("demo/nobody", &Fake::default()).is_err());
        assert!(w.stop("nobody", &Fake::default()).is_err());
    }
}
