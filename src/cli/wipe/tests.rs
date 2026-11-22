use super::*;
use crate::agents::Agent;
use crate::launch::child::fake::{Call, Fake};
use crate::launch::child::{PaneClose, SpawnReq, WorktreeReq, add_worktree, spawn_child};
use crate::launch::fixture::Fx;
use crate::project::ItemKind;
use crate::store::Touch;

struct W {
    fx: Fx,
    repo: PathBuf,
    /// The worktree directories `world` made.
    made: Vec<PathBuf>,
}

/// Project `demo` with repo `repo`, children `c1` (two worktrees) and `c2`
/// (one), a diary and a few files in the home, all on disk.
fn world() -> W {
    let fx = Fx::new();
    fx.init("demo");
    let repo = std::fs::canonicalize(fx.tmp.path()).unwrap().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    fx.store("demo")
        .write(Touch::PROJECT, |tx| {
            project::add(tx, ItemKind::Repo, &repo.display().to_string())
        })
        .unwrap();
    let mut w = W {
        fx,
        repo,
        made: Vec::new(),
    };
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
    add_worktree(
        &mut w.fx.store("demo"),
        &WorktreeReq {
            child: "c1".into(),
            session_id: "sid-c1".into(),
            name: "a".into(),
            branch: None,
        },
        &run,
    )
    .unwrap();
    w.made = w.paths();
    for p in &w.made {
        std::fs::create_dir_all(p).unwrap();
    }
    let home = w.fx.home("demo");
    std::fs::write(home.join("diary/note.md"), "n").unwrap();
    std::fs::write(home.join("scratch.txt"), "s").unwrap();
    std::fs::create_dir_all(home.join("sub/deep")).unwrap();
    std::fs::write(home.join("sub/deep/x"), "x").unwrap();
    w
}

impl W {
    fn paths(&self) -> Vec<PathBuf> {
        self.fx
            .store("demo")
            .read(|c| crew::worktrees(c, None))
            .unwrap()
            .into_iter()
            .map(|r| PathBuf::from(r.path))
            .collect()
    }

    fn state(&self) -> PathBuf {
        self.fx.tmp.path().join("state/crook/demo")
    }

    fn wipe(&self, with_diary: bool, run: &Fake) -> Result<Vec<String>, Error> {
        super::run(&self.fx.env(), "demo", with_diary, run)
    }

    /// Nothing was touched: rows, worktrees, home and state all stay.
    fn assert_untouched(&self, before: &[PathBuf], run: &Fake) {
        assert_eq!(self.paths(), before);
        assert!(self.made.iter().all(|p| p.is_dir()));
        assert!(self.state().join("crook.db").is_file());
        let home = self.fx.home("demo");
        assert!(home.join("scratch.txt").is_file() && home.join("diary/note.md").is_file());
        assert!(
            !run.calls()
                .iter()
                .any(|c| matches!(c, Call::Remove(..) | Call::Prune(..))),
            "{:?}",
            run.calls()
        );
    }

    fn config(&self, toml: &str) {
        let f = self.fx.tmp.path().join("cfg/crook/config.toml");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, toml).unwrap();
    }
}

fn agent(sid: &str) -> Agent {
    Agent {
        session_id: sid.into(),
        pid: Some(1),
        cwd: "/x".into(),
        kind: "background".into(),
        state: None,
    }
}

#[test]
fn wipe_refuses_live() {
    for (name, sid) in [("orchestrator", "orch-sid"), ("child", "sid-c2")] {
        let w = world();
        w.fx.store("demo")
            .write(Touch::empty(), |tx| {
                crew::record_orchestrator(tx, "orch-sid", 7)
            })
            .unwrap();
        let before = w.paths();
        let run = Fake {
            agents: vec![agent("unrelated"), agent(sid)],
            ..Fake::default()
        };
        let e = w.wipe(false, &run).unwrap_err().to_string();
        assert!(e.contains(sid), "{name}: {e}");
        assert!(e.contains("live"), "{name}: {e}");
        w.assert_untouched(&before, &run);
    }
}

#[test]
fn wipe_names_every_live_session() {
    let w = world();
    w.fx.store("demo")
        .write(Touch::empty(), |tx| {
            crew::record_orchestrator(tx, "orch-sid", 7)
        })
        .unwrap();
    let run = Fake {
        agents: vec![agent("orch-sid"), agent("sid-c1"), agent("sid-c2")],
        ..Fake::default()
    };
    let e = w.wipe(false, &run).unwrap_err().to_string();
    for sid in ["orch-sid", "sid-c1", "sid-c2"] {
        assert!(e.contains(sid), "{e}");
    }
}

/// An archived child may still run (its stop could not confirm it gone),
/// and so may a fork or any id the child held.
#[test]
fn wipe_refuses_archived_and_forked_sessions() {
    for sid in ["sid-c2", "fork-c1", "old-c1"] {
        let w = world();
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                crew::archive(tx, "c2")?;
                crew::add_fork(tx, "c1", "fork-c1")?;
                tx.execute(
                    "INSERT INTO child_sessions(child, session_id) VALUES ('c1', 'old-c1')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let before = w.paths();
        let run = Fake {
            agents: vec![agent(sid), agent("another-project")],
            ..Fake::default()
        };
        let e = w.wipe(false, &run).unwrap_err().to_string();
        assert!(e.contains(sid) && e.contains("live"), "{sid}: {e}");
        let remedy = if sid == "sid-c2" {
            "crook stop demo/c2"
        } else {
            "stop that session itself"
        };
        assert!(e.contains(remedy), "{sid}: {e}");
        assert_eq!(e.matches(sid).count(), 1, "named once: {e}");
        w.assert_untouched(&before, &run);
    }
}

/// Any session whose cwd is in a recorded worktree or the home, whatever
/// its id: the user's own `claude`, say.
#[test]
fn wipe_refuses_a_session_inside_a_worktree_or_the_home() {
    for n in 0..3 {
        let w = world();
        let cwd = match n {
            0 => w.paths()[1].join("sub"),
            1 => w.fx.home("demo"),
            _ => w.fx.home("demo").join("diary"),
        };
        let before = w.paths();
        let run = Fake {
            agents: vec![
                agent("another-project"),
                Agent {
                    cwd: cwd.display().to_string(),
                    ..agent("users-own")
                },
            ],
            ..Fake::default()
        };
        let e = w.wipe(false, &run).unwrap_err().to_string();
        assert!(
            e.contains("users-own") && e.contains(&cwd.display().to_string()),
            "{e}"
        );
        w.assert_untouched(&before, &run);
    }
    // A session elsewhere does not block it.
    let w = world();
    let run = Fake {
        agents: vec![agent("another-project")],
        ..Fake::default()
    };
    w.wipe(false, &run).unwrap();
    assert!(!w.state().exists());
}

#[test]
fn wipe_refuses_when_agents_cannot_say() {
    let w = world();
    let before = w.paths();
    let run = Fake {
        agents_fail: true,
        ..Fake::default()
    };
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains("cannot confirm"), "{e}");
    w.assert_untouched(&before, &run);
}

#[test]
fn wipe_refuses_dirty() {
    let w = world();
    let all = w.paths();
    assert_eq!(all.len(), 3);
    let run = Fake {
        dirty: [all[0].clone(), all[2].clone()].into(),
        ..Fake::default()
    };
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains(&all[0].display().to_string()), "{e}");
    assert!(e.contains(&all[2].display().to_string()), "{e}");
    assert!(!e.contains(&all[1].display().to_string()), "{e}");
    w.assert_untouched(&all, &run);
}

#[test]
fn wipe_removes() {
    let w = world();
    let all = w.paths();
    let home = w.fx.home("demo");
    let run = Fake::default();
    let lines = w.wipe(false, &run).unwrap();
    let mut want: Vec<_> = all
        .iter()
        .map(|p| Call::Remove(w.repo.clone(), p.clone()))
        .collect();
    want.push(Call::Prune(w.repo.clone()));
    assert_eq!(
        run.calls(),
        want,
        "each by its recorded path, then one prune"
    );
    assert!(
        lines[0].contains("wiped demo") && lines[0].contains("3 worktrees"),
        "{lines:?}"
    );
    // The home keeps the diary and what crook did not make, named; the
    // state dir is gone.
    assert!(home.join("scratch.txt").is_file() && home.join("sub/deep/x").is_file());
    assert!(home.join("diary/note.md").is_file());
    assert!(
        lines[1].contains("not made by crook: scratch.txt, sub"),
        "{lines:?}"
    );
    assert!(!w.state().exists());
}

#[test]
fn wipe_drops_each_row_as_its_worktree_goes() {
    // A removal that fails leaves its row and everything after it intact,
    // and keeps the home and state.
    struct Fails(Fake, PathBuf);
    impl Launcher for Fails {
        fn worktree_add(&self, r: &Path, p: &Path, b: &str) -> Result<(), Error> {
            self.0.worktree_add(r, p, b)
        }
        fn worktree_remove(&self, r: &Path, p: &Path) -> Result<(), Error> {
            if p == self.1 {
                return Err(Error::Refused("git: contains modified files".into()));
            }
            self.0.worktree_remove(r, p)
        }
        fn worktree_list(&self, r: &Path) -> Result<Vec<PathBuf>, Error> {
            self.0.worktree_list(r)
        }
        fn worktree_dirty(&self, p: &Path) -> Result<bool, Error> {
            self.0.worktree_dirty(p)
        }
        fn worktree_prune(&self, r: &Path) -> Result<(), Error> {
            self.0.worktree_prune(r)
        }
        fn branches(&self, r: &Path, p: &str) -> Result<Vec<String>, Error> {
            self.0.branches(r, p)
        }
        fn start(
            &self,
            m: crate::launch::child::Mode,
            c: &crate::launch::child::StartCmd,
        ) -> Result<(), Error> {
            self.0.start(m, c)
        }
        fn agents(&self) -> Result<Vec<Agent>, Error> {
            self.0.agents()
        }
        fn stop_bg(&self, s: &str) -> Result<(), Error> {
            self.0.stop_bg(s)
        }
        fn terminate(&self, p: u32) -> Result<(), Error> {
            self.0.terminate(p)
        }
        fn close_pane(&self, s: &str, c: &str) -> Result<PaneClose, Error> {
            self.0.close_pane(s, c)
        }
    }
    let w = world();
    let all = w.paths();
    let run = Fails(Fake::default(), all[1].clone());
    let e = super::run(&w.fx.env(), "demo", false, &run)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(&all[1].display().to_string()) && e.contains("again"),
        "{e}"
    );
    assert_eq!(w.paths(), [all[1].clone()], "only the failed row stays");
    assert!(w.state().join("crook.db").is_file());
    assert!(w.fx.home("demo").join("scratch.txt").is_file());
    // The retry finishes the job.
    let again = Fake::default();
    w.wipe(false, &again).unwrap();
    assert!(!w.state().exists());
}

#[test]
fn wipe_skips_a_worktree_already_gone() {
    let w = world();
    let all = w.paths();
    std::fs::remove_dir_all(&all[1]).unwrap();
    let run = Fake::default();
    w.wipe(false, &run).unwrap();
    let removed: Vec<_> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Remove(_, p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(removed, [all[0].clone(), all[2].clone()]);
    assert!(!w.state().exists());
}

#[test]
fn wipe_reports_unrecorded() {
    let w = world();
    let stray = w.repo.join(".worktrees/demo/stray");
    let other = w.repo.join(".worktrees/other/x");
    let outside = w.repo.join("elsewhere");
    for p in [&stray, &other, &outside] {
        std::fs::create_dir_all(p).unwrap();
    }
    let mut listed = vec![
        w.repo.clone(),
        stray.clone(),
        other.clone(),
        outside.clone(),
    ];
    let recorded = w.paths();
    listed.extend(recorded.clone());
    let run = Fake {
        listed: Some(listed),
        ..Fake::default()
    };
    let lines = w.wipe(false, &run).unwrap();
    let text = lines.join("\n");
    assert!(text.contains(&stray.display().to_string()), "{text}");
    assert!(!text.contains(&other.display().to_string()), "{text}");
    assert!(!text.contains(&outside.display().to_string()), "{text}");
    for c in &recorded {
        assert!(
            !text.contains(&format!("{} (in", c.display())),
            "recorded ones are not reported: {text}"
        );
    }
    assert!(stray.is_dir(), "never removed");
    assert!(
        !run.calls()
            .iter()
            .any(|c| matches!(c, Call::Remove(_, p) if *p == stray)),
        "{:?}",
        run.calls()
    );
}

#[test]
fn wipe_only_removes_recorded() {
    let w0 = world();
    let repo = w0.repo.display().to_string();
    for bad in [
        "/elsewhere/x".to_string(),
        "/".to_string(),
        format!("{repo}/.worktrees/other/c1"),
        format!("{repo}/.worktrees/demo"),
        format!("{repo}/.worktrees/demo/../../x"),
        format!("{repo}/src"),
    ] {
        let w = world();
        w.fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                crew::add_worktree(
                    tx,
                    &crew::Worktree {
                        child: "c1".into(),
                        repo: w.repo.display().to_string(),
                        path: bad.clone(),
                        branch: "b".into(),
                    },
                )
            })
            .unwrap();
        let before = w.paths();
        let run = Fake::default();
        let e = w.wipe(false, &run).unwrap_err().to_string();
        assert!(e.contains(&bad), "{bad}: {e}");
        assert!(run.calls().is_empty(), "{bad}: {:?}", run.calls());
        w.assert_untouched(&before, &run);
    }
}

#[test]
fn wipe_refuses_a_worktree_git_does_not_list() {
    let w = world();
    let before = w.paths();
    let run = Fake {
        listed: Some(Vec::new()),
        ..Fake::default()
    };
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains("not removable"), "{e}");
    w.assert_untouched(&before, &run);
}

#[test]
fn wipe_refuses_a_symlinked_worktree() {
    let w = world();
    let all = w.paths();
    std::fs::remove_dir_all(&all[0]).unwrap();
    std::os::unix::fs::symlink(&all[2], &all[0]).unwrap();
    let run = Fake::default();
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains("symlink"), "{e}");
    w.assert_untouched(&all, &run);
}

#[test]
fn wipe_keeps_diary() {
    let w = world();
    let home = w.fx.home("demo");
    let lines = w.wipe(false, &Fake::default()).unwrap();
    assert!(home.join("diary/note.md").is_file());
    assert!(lines[0].contains("kept the diary"), "{lines:?}");

    let w = world();
    let home = w.fx.home("demo");
    w.wipe(true, &Fake::default()).unwrap();
    assert!(!home.join("diary").exists() && home.join("scratch.txt").is_file());

    let w = world();
    let home = w.fx.home("demo");
    std::fs::remove_file(home.join("scratch.txt")).unwrap();
    std::fs::remove_dir_all(home.join("sub")).unwrap();
    let lines = w.wipe(true, &Fake::default()).unwrap();
    assert!(
        !home.exists(),
        "--with-diary removes the diary and the home"
    );
    assert!(lines[0].contains("the home"), "{lines:?}");
    assert!(!w.state().exists());
}

#[test]
fn wipe_leaves_a_home_crook_did_not_make() {
    for marker in [None, Some("crook project home: other\n")] {
        let w = world();
        let home = w.fx.home("demo");
        match marker {
            None => std::fs::remove_file(home.join(paths::HOME_MARKER)).unwrap(),
            Some(m) => std::fs::write(home.join(paths::HOME_MARKER), m).unwrap(),
        }
        let lines = w.wipe(true, &Fake::default()).unwrap();
        assert!(home.join("diary/note.md").is_file(), "{marker:?}");
        assert!(home.join("scratch.txt").is_file(), "{marker:?}");
        assert!(
            lines.iter().any(|l| l.contains("crook did not make it")),
            "{lines:?}"
        );
        assert!(!lines[0].contains("the home"), "{lines:?}");
        assert!(!w.state().exists());
    }
}

#[test]
fn wipe_refuses_with_diary_under_diary_root() {
    let w = world();
    let vault = w.fx.tmp.path().join("vault");
    let diary = vault.join("demo");
    std::fs::create_dir_all(&diary).unwrap();
    std::fs::write(diary.join("n.md"), "n").unwrap();
    w.config(&format!("diary_root = \"{}\"\n", vault.display()));
    let before = w.paths();
    let run = Fake::default();
    let e = w.wipe(true, &run).unwrap_err().to_string();
    assert!(
        e.contains("diary_root") && e.contains(&diary.display().to_string()),
        "{e}"
    );
    w.assert_untouched(&before, &run);
    assert!(diary.join("n.md").is_file());
    // Without the flag the wipe goes ahead and leaves the vault alone.
    w.wipe(false, &Fake::default()).unwrap();
    assert!(diary.join("n.md").is_file());
    assert!(!w.state().exists());
}

#[test]
fn wipe_refuses_a_diary_that_overlaps_the_home() {
    let w = world();
    // `<diary_root>/demo` is the project home itself.
    let root = w.fx.tmp.path().join("data/crook");
    w.config(&format!("diary_root = \"{}\"\n", root.display()));
    let before = w.paths();
    let run = Fake::default();
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains("overlaps"), "{e}");
    w.assert_untouched(&before, &run);
}

#[test]
fn wipe_refuses_a_symlinked_home() {
    let w = world();
    let home = w.fx.home("demo");
    let real = w.fx.tmp.path().join("real-home");
    std::fs::rename(&home, &real).unwrap();
    std::os::unix::fs::symlink(&real, &home).unwrap();
    let run = Fake::default();
    let e = w.wipe(false, &run).unwrap_err().to_string();
    assert!(e.contains("symlink"), "{e}");
    assert!(real.join("scratch.txt").is_file());
    assert!(w.state().join("crook.db").is_file());
}

#[test]
fn wipe_unlinks_a_symlinked_diary_without_following_it() {
    let w = world();
    let home = w.fx.home("demo");
    let outside = w.fx.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep"), "k").unwrap();
    std::fs::remove_dir_all(home.join("diary")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join("diary")).unwrap();
    std::os::unix::fs::symlink(&outside, home.join("link")).unwrap();
    w.wipe(true, &Fake::default()).unwrap();
    assert!(outside.join("keep").is_file());
    assert!(home.join("diary").symlink_metadata().is_err());
    assert!(home.join("link").symlink_metadata().is_ok(), "not crook's");
}

#[test]
fn wipe_branches() {
    let w = world();
    let run = Fake {
        branch_list: [(
            w.repo.clone(),
            vec![
                "crook/demo/c1".to_string(),
                "crook/demo/c2".to_string(),
                "crook/other/c1".to_string(),
                "main".to_string(),
            ],
        )]
        .into(),
        ..Fake::default()
    };
    let lines = w.wipe(false, &run).unwrap();
    let text = lines.join("\n");
    assert!(
        text.contains("crook/demo/c1") && text.contains("crook/demo/c2"),
        "{text}"
    );
    assert!(!text.contains("other") && !text.contains("main"), "{text}");
    assert!(text.contains(&w.repo.display().to_string()), "{text}");
    // The launcher has no way to delete a branch, and the call log shows none.
    assert!(
        run.calls()
            .iter()
            .all(|c| matches!(c, Call::Remove(..) | Call::Prune(..))),
        "{:?}",
        run.calls()
    );
}

#[test]
fn wipe_prunes_every_record_repo() {
    let w = world();
    let second = w.fx.tmp.path().join("repo2");
    std::fs::create_dir_all(&second).unwrap();
    let second_text = second.display().to_string();
    w.fx.store("demo")
        .write(Touch::PROJECT, |tx| {
            project::add(tx, ItemKind::Repo, &second_text)
        })
        .unwrap();
    let run = Fake::default();
    w.wipe(false, &run).unwrap();
    let pruned: Vec<_> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Prune(r) => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(pruned.len(), 2, "{pruned:?}");
    assert!(pruned.contains(&w.repo) && pruned.iter().any(|p| p.ends_with("repo2")));
}

#[test]
fn wipe_unknown_project_is_refused() {
    let fx = Fx::new();
    let e = super::run(&fx.env(), "nope", false, &Fake::default()).unwrap_err();
    assert!(matches!(e, Error::Schema { .. }), "{e}");
    assert!(super::run(&fx.env(), "../x", false, &Fake::default()).is_err());
}
