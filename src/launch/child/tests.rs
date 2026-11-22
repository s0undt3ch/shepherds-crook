use std::collections::HashMap;

use super::fake::{Call, Fake};
use super::*;
use crate::launch::fixture::Fx;
use crate::project::{self, ItemKind};
use crate::store::Touch;
use crate::{crew, paths};

/// A machine with project `demo` and one real directory as its repo.
struct World {
    fx: Fx,
    repo: PathBuf,
    extra: HashMap<String, String>,
}

fn world() -> World {
    let fx = Fx::new();
    fx.init("demo");
    let repo = fx.tmp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    fx.store("demo")
        .write(Touch::PROJECT, |tx| {
            project::add(tx, ItemKind::Repo, &repo.display().to_string())?;
            project::add(tx, ItemKind::Notes, "/vault/notes")
        })
        .unwrap();
    World {
        fx,
        repo,
        extra: HashMap::new(),
    }
}

impl World {
    fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        let base = self.fx.env();
        let extra = self.extra.clone();
        move |k: &str| extra.get(k).cloned().or_else(|| base(k))
    }

    fn canon(&self) -> PathBuf {
        std::fs::canonicalize(&self.repo).unwrap()
    }

    fn spawn(&self, slug: &str, child: &str, brief: &str, run: &Fake) -> Result<String, Error> {
        self.spawn_in(
            slug,
            child,
            &self.repo.display().to_string(),
            brief,
            None,
            run,
        )
    }

    fn spawn_in(
        &self,
        slug: &str,
        child: &str,
        repo: &str,
        brief: &str,
        branch: Option<&str>,
        run: &Fake,
    ) -> Result<String, Error> {
        let env = self.env();
        let mut store = self.fx.store(slug);
        let req = SpawnReq {
            env: &env,
            slug: slug.into(),
            child: child.into(),
            repo: repo.into(),
            brief: brief.into(),
            branch: branch.map(str::to_string),
            session_id: format!("sid-{slug}-{child}"),
            base_env: vec![
                ("PATH".into(), "/bin".into()),
                ("CROOK_PROJECT".into(), "leak".into()),
                ("CLAUDECODE".into(), "1".into()),
            ],
            crook_bin: "/abs/crook".into(),
        };
        spawn_child(&mut store, &req, run)
    }

    fn rows(&self, slug: &str) -> (Vec<String>, Vec<crew::Worktree>) {
        let s = self.fx.store(slug);
        let kids = s
            .read(|c| {
                let mut st = c.prepare("SELECT child FROM children ORDER BY child")?;
                Ok(st
                    .query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap();
        (kids, s.read(|c| crew::worktrees(c, None)).unwrap())
    }
}

fn flag(args: &[OsString], name: &str) -> Vec<String> {
    let a: Vec<String> = args
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    a.windows(2)
        .filter(|w| w[0] == name)
        .map(|w| w[1].clone())
        .collect()
}

fn refusal(r: Result<impl std::fmt::Debug, Error>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn spawn_checks() {
    let w = world();
    let run = Fake::default();
    // A repo not in the record.
    let other = w.fx.tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let e = refusal(w.spawn_in("demo", "c1", &other.display().to_string(), "b", None, &run));
    assert!(e.contains("not a repo of project demo"), "{e}");
    // A brief over 4000 characters, counted in characters not bytes.
    let e = refusal(w.spawn("demo", "c1", &"é".repeat(MAX_BRIEF_CHARS + 1), &run));
    assert!(e.contains("4000"), "{e}");
    assert!(
        w.spawn("demo", "c1", &"é".repeat(MAX_BRIEF_CHARS), &run)
            .is_ok()
    );
    // A slug-unsafe child id.
    for bad in ["C1", "a/b", "../x", "a.b", "a b", ""] {
        let e = refusal(w.spawn("demo", bad, "b", &run));
        assert!(e.contains("invalid child name"), "{bad:?}: {e}");
    }
    // Nothing else was recorded or run: only the one good spawn.
    assert_eq!(w.rows("demo").0, ["c1"]);
    assert_eq!(run.starts().len(), 1);
}

#[test]
fn spawn_refuses_a_ninth_live_child() {
    let w = world();
    let run = Fake::default();
    for i in 0..MAX_LIVE_CHILDREN {
        w.spawn("demo", &format!("c{i}"), "b", &run).unwrap();
    }
    let e = refusal(w.spawn("demo", "c9", "b", &run));
    assert!(e.contains("8 live children"), "{e}");
    assert_eq!(run.starts().len(), 8);
    // An archived child no longer counts.
    w.fx.store("demo")
        .write(Touch::CHILDREN, |tx| crew::archive(tx, "c0"))
        .unwrap();
    assert!(w.spawn("demo", "c9", "b", &run).is_ok());
}

#[test]
fn spawn_rows_first() {
    for (fail_add, fail_start) in [(true, false), (false, true)] {
        let w = world();
        let run = Fake {
            fail_add,
            fail_start,
            ..Fake::default()
        };
        assert!(w.spawn("demo", "c1", "b", &run).is_err());
        let (kids, wts) = w.rows("demo");
        assert_eq!(kids, ["c1"], "the rows stay for open to reconcile");
        assert_eq!(wts.len(), 1);
        let held =
            w.fx.store("demo")
                .read(|c| crew::held(c, "c1", "sid-demo-c1"))
                .unwrap();
        assert!(
            held,
            "the first session id is held, so the hook park-denies it"
        );
        // Rows were written before the failing call ran.
        assert_eq!(run.calls().len(), if fail_add { 1 } else { 2 });
    }
}

#[test]
fn spawn_worktree() {
    let w = world();
    let run = Fake::default();
    let sid = w.spawn("demo", "c1", "the brief", &run).unwrap();
    assert_eq!(sid, "sid-demo-c1");
    let path = w.canon().join(".worktrees/demo/c1");
    assert_eq!(
        run.calls()[0],
        Call::Add(w.canon(), path.clone(), "crook/demo/c1".into())
    );
    let s = w.fx.store("demo");
    let r = s.read(|c| crew::resumable(c, "c1")).unwrap();
    assert_eq!(r.worktree, path.display().to_string());
    let (_, wts) = w.rows("demo");
    assert_eq!(
        wts,
        [crew::Worktree {
            child: "c1".into(),
            repo: w.canon().display().to_string(),
            path: path.display().to_string(),
            branch: "crook/demo/c1".into(),
        }]
    );
    assert_eq!(run.starts()[0].1.cwd, path);
    // No link is made under the home.
    let mut home: Vec<_> = std::fs::read_dir(w.fx.home("demo"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    home.sort();
    assert_eq!(home, [crate::paths::HOME_MARKER, "diary"]);
}

#[test]
fn spawn_takes_a_given_branch() {
    let w = world();
    let run = Fake::default();
    w.spawn_in(
        "demo",
        "c1",
        &w.repo.display().to_string(),
        "b",
        Some("feat/x"),
        &run,
    )
    .unwrap();
    assert!(matches!(&run.calls()[0], Call::Add(_, _, b) if b == "feat/x"));
    let (_, wts) = w.rows("demo");
    assert_eq!(wts[0].branch, "feat/x");
}

#[test]
fn a_branch_that_would_be_a_git_option_is_refused() {
    let w = world();
    let run = Fake::default();
    for bad in ["--detach", "-x", "a b", "a\nb", "", "x\ty"] {
        let e = refusal(w.spawn_in(
            "demo",
            "c1",
            &w.repo.display().to_string(),
            "b",
            Some(bad),
            &run,
        ));
        assert!(e.contains("branch"), "{bad:?}: {e}");
    }
    assert!(run.calls().is_empty());
    assert!(w.rows("demo").0.is_empty());
}

#[test]
fn spawn_stores_the_repo_canonicalised() {
    // A repo reached through a symlink (as /tmp is /private/tmp on macOS)
    // is stored as the real path, which the hook's parked-in-repo check compares.
    let w = world();
    let link = w.fx.tmp.path().join("link");
    std::os::unix::fs::symlink(&w.repo, &link).unwrap();
    w.fx.store("demo")
        .write(Touch::PROJECT, |tx| {
            project::add(tx, ItemKind::Repo, &link.display().to_string())
        })
        .unwrap();
    let run = Fake::default();
    w.spawn_in("demo", "c1", &link.display().to_string(), "b", None, &run)
        .unwrap();
    let canon = w.canon().display().to_string();
    assert_ne!(canon, link.display().to_string());
    let stored =
        w.fx.store("demo")
            .read(|c| crew::child_repo(c, "c1"))
            .unwrap();
    assert_eq!(stored, canon);
    let (_, wts) = w.rows("demo");
    assert_eq!(wts[0].repo, canon);
    assert!(wts[0].path.starts_with(&canon));
    // The hook finds the parked child by the canonical spelling.
    w.fx.store("demo")
        .write(Touch::STATUS, |tx| {
            tx.execute("UPDATE status SET state = 'parked' WHERE child = 'c1'", [])?;
            Ok(())
        })
        .unwrap();
    let found =
        w.fx.store("demo")
            .read(|c| crew::parked_in_repo(c, &canon))
            .unwrap();
    assert_eq!(found.as_deref(), Some("c1"));
}

#[test]
fn spawn_two_projects() {
    let w = world();
    w.fx.init("other");
    w.fx.store("other")
        .write(Touch::PROJECT, |tx| {
            project::add(tx, ItemKind::Repo, &w.repo.display().to_string())
        })
        .unwrap();
    let run = Fake::default();
    w.spawn("demo", "c1", "b", &run).unwrap();
    w.spawn("other", "c1", "b", &run).unwrap();
    let adds: Vec<_> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Add(_, p, b) => Some((p, b)),
            _ => None,
        })
        .collect();
    assert_eq!(adds.len(), 2);
    assert_ne!(adds[0].0, adds[1].0);
    assert_ne!(adds[0].1, adds[1].1);
    assert!(adds[0].0.ends_with(".worktrees/demo/c1"));
    assert!(adds[1].0.ends_with(".worktrees/other/c1"));
    assert_eq!(adds[1].1, "crook/other/c1");
}

fn start_of(herdr: bool) -> (World, Mode, StartCmd) {
    let mut w = world();
    if herdr {
        w.extra.insert("HERDR_ENV".into(), "1".into());
    }
    let run = Fake::default();
    w.spawn("demo", "c1", "b", &run).unwrap();
    let (mode, cmd) = run.starts().remove(0);
    (w, mode, cmd)
}

#[test]
fn spawn_argv_child() {
    let (w, mode, cmd) = start_of(false);
    assert_eq!(mode, Mode::Bg);
    let a = &cmd.args;
    assert_eq!(flag(a, "-n"), ["crook-demo-c1"]);
    assert_eq!(flag(a, "--session-id"), ["sid-demo-c1"]);
    assert_eq!(flag(a, "--permission-mode"), ["manual"]);
    assert_eq!(
        flag(a, "--append-system-prompt"),
        [include_str!("../../../plugin/prompts/child.md")]
    );
    // Every notes folder: the diary `init` added, then the one the test added.
    let diary = w.fx.home("demo").join("diary").display().to_string();
    assert_eq!(flag(a, "--add-dir"), [diary.as_str(), "/vault/notes"]);
    assert!(flag(a, "--settings")[0].ends_with("settings-child.json"));
    assert!(a.iter().any(|x| x == "--bg"));
    let prompt = "You are child c1 of crook-demo. Your project card is above.";
    assert_eq!(a.last().unwrap(), prompt);
    // A variadic --add-dir would swallow a bare prompt after its last value.
    let strs: Vec<String> = a.iter().map(|x| x.to_string_lossy().into_owned()).collect();
    for (i, s) in strs.iter().enumerate() {
        if s == "--add-dir" {
            let after = strs.get(i + 2).map(String::as_str);
            assert!(after.is_none_or(|n| n.starts_with('-')), "{strs:?}");
        }
    }
    let env = |k: &str| {
        cmd.env
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.to_string_lossy().into_owned())
    };
    assert_eq!(env("CROOK_ROLE").as_deref(), Some("child"));
    assert_eq!(env("CROOK_CHILD").as_deref(), Some("c1"));
    assert_eq!(env("CROOK_PROJECT").as_deref(), Some("demo"));
    assert_eq!(env("CROOK_BIN").as_deref(), Some("/abs/crook"));
    assert!(env("CLAUDECODE").is_none());
}

#[test]
fn spawn_argv_in_herdr_has_no_bg() {
    let (_w, mode, cmd) = start_of(true);
    assert_eq!(mode, Mode::Herdr);
    assert!(!cmd.args.iter().any(|x| x == "--bg"));
    assert_eq!(flag(&cmd.args, "-n"), ["crook-demo-c1"]);
}

#[test]
fn spawn_ensures_plugin() {
    let w = world();
    let dir = paths::plugin_dir(&w.env());
    assert!(!dir.exists());
    let run = Fake::default();
    w.spawn("demo", "c1", "b", &run).unwrap();
    assert!(dir.join("prompts/child.md").is_file());
    assert_eq!(
        flag(&run.starts()[0].1.args, "--plugin-dir"),
        [dir.display().to_string()]
    );
}

#[test]
fn spawn_checks_the_mode_before_writing_rows() {
    struct NoHerdr(Fake);
    impl Launcher for NoHerdr {
        fn check(&self, mode: Mode) -> Result<(), Error> {
            match mode {
                Mode::Herdr => Err(Error::Refused("herdr panes arrive with Task 20".into())),
                Mode::Bg => Ok(()),
            }
        }
        fn worktree_add(&self, r: &Path, p: &Path, b: &str) -> Result<(), Error> {
            self.0.worktree_add(r, p, b)
        }
        fn worktree_remove(&self, r: &Path, p: &Path) -> Result<(), Error> {
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
        fn start(&self, m: Mode, c: &StartCmd) -> Result<(), Error> {
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
        fn close_pane(&self, s: &str, c: &str) -> Result<bool, Error> {
            self.0.close_pane(s, c)
        }
    }
    let mut w = world();
    w.extra.insert("HERDR_ENV".into(), "1".into());
    let run = NoHerdr(Fake::default());
    let env = w.env();
    let mut store = w.fx.store("demo");
    let req = SpawnReq {
        env: &env,
        slug: "demo".into(),
        child: "c1".into(),
        repo: w.repo.display().to_string(),
        brief: "b".into(),
        branch: None,
        session_id: "s".into(),
        base_env: Vec::new(),
        crook_bin: "/abs/crook".into(),
    };
    let e = refusal(spawn_child(&mut store, &req, &run));
    assert!(e.contains("Task 20"), "{e}");
    assert!(w.rows("demo").0.is_empty());
    assert!(run.0.calls().is_empty());
}

// The `worktree` verb.

fn spawned() -> (World, Fake) {
    let w = world();
    let run = Fake::default();
    w.spawn("demo", "c1", "b", &run).unwrap();
    (w, run)
}

fn wt_req(name: &str, branch: Option<&str>) -> WorktreeReq {
    WorktreeReq {
        child: "c1".into(),
        session_id: "sid-demo-c1".into(),
        name: name.into(),
        branch: branch.map(str::to_string),
    }
}

#[test]
fn worktree_creates_and_records() {
    let (w, _) = spawned();
    let run = Fake::default();
    let mut s = w.fx.store("demo");
    let p = add_worktree(&mut s, &wt_req("fix", None), &run).unwrap();
    let want = w.canon().join(".worktrees/demo/c1.fix");
    assert_eq!(p, want);
    assert_eq!(
        run.calls(),
        [Call::Add(
            w.canon(),
            want.clone(),
            "crook/demo/c1.fix".into()
        )]
    );
    let (_, wts) = w.rows("demo");
    assert_eq!(wts.len(), 2);
    assert_eq!(wts[1].path, want.display().to_string());
    assert_eq!(wts[1].child, "c1");
    // A given branch.
    let p2 = add_worktree(&mut s, &wt_req("docs", Some("feat/docs")), &run).unwrap();
    assert!(p2.ends_with("c1.docs"));
    assert!(matches!(&run.calls()[1], Call::Add(_, _, b) if b == "feat/docs"));
    // The same name twice is refused, without a raw constraint error.
    let e = refusal(add_worktree(&mut s, &wt_req("fix", None), &run));
    assert!(e.contains("already") && !e.contains("UNIQUE"), "{e}");
    assert_eq!(run.calls().len(), 2);
}

#[test]
fn worktree_row_is_written_before_the_launcher_runs() {
    let (w, _) = spawned();
    let run = Fake {
        fail_add: true,
        ..Fake::default()
    };
    let mut s = w.fx.store("demo");
    assert!(add_worktree(&mut s, &wt_req("fix", None), &run).is_err());
    assert_eq!(
        w.rows("demo").1.len(),
        2,
        "the row stays for open to reconcile"
    );
}

#[test]
fn worktree_refuses_a_session_that_is_not_the_child() {
    let (w, _) = spawned();
    let run = Fake::default();
    let mut s = w.fx.store("demo");
    let mut req = wt_req("fix", None);
    req.session_id = "a-fork".into();
    let e = refusal(add_worktree(&mut s, &req, &run));
    assert!(e.contains("fork"), "{e}");
    assert!(run.calls().is_empty());
    // An unknown child.
    let mut req = wt_req("fix", None);
    req.child = "nobody".into();
    assert!(add_worktree(&mut s, &req, &run).is_err());
}

#[test]
fn worktree_rejects_bad_name() {
    let (w, _) = spawned();
    let run = Fake::default();
    let mut s = w.fx.store("demo");
    for bad in ["Fix", "a/b", "../x", "a.b", "a b", ""] {
        let e = refusal(add_worktree(&mut s, &wt_req(bad, None), &run));
        assert!(e.contains("invalid worktree name"), "{bad:?}: {e}");
    }
    for bad in ["--detach", "a b"] {
        assert!(add_worktree(&mut s, &wt_req("ok", Some(bad)), &run).is_err());
    }
    assert!(run.calls().is_empty(), "nothing is run");
    assert_eq!(w.rows("demo").1.len(), 1, "nothing is written");
}

#[test]
fn parse_worktree_list_takes_the_worktree_lines() {
    let out = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\n\
               worktree /r/.worktrees/demo/c1\nHEAD def\nbranch refs/heads/crook/demo/c1\n\n\
               worktree /r/detached dir\nHEAD 123\ndetached\n";
    assert_eq!(
        parse_worktree_list(out),
        [
            PathBuf::from("/r"),
            PathBuf::from("/r/.worktrees/demo/c1"),
            PathBuf::from("/r/detached dir")
        ]
    );
    assert!(parse_worktree_list("").is_empty());
}

// stop

fn agent(sid: &str, pid: Option<u32>, kind: &str) -> Agent {
    Agent {
        session_id: sid.into(),
        pid,
        cwd: "/x".into(),
        kind: kind.into(),
        state: None,
    }
}

fn set_pid(w: &World, pid: u32) {
    w.fx.store("demo")
        .write(Touch::CHILDREN, |tx| {
            tx.execute(
                "UPDATE children SET claude_pid = ?1 WHERE child = 'c1'",
                [pid],
            )?;
            Ok(())
        })
        .unwrap();
}

fn archived(w: &World) -> bool {
    w.fx.store("demo")
        .read(|c| crew::resumable(c, "c1"))
        .unwrap()
        .archived
}

#[test]
fn stop_kinds() {
    // A --bg child: claude stop <session id>.
    let (w, _) = spawned();
    let run = Fake {
        agents: vec![agent("sid-demo-c1", None, "background")],
        ..Fake::default()
    };
    let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
    assert_eq!(r, Stopped::Background);
    assert_eq!(run.calls(), [Call::StopBg("sid-demo-c1".into())]);
    assert!(archived(&w));

    // A herdr child whose pid claude agents confirms: its pane, then SIGTERM.
    let (w, _) = spawned();
    set_pid(&w, 4242);
    let run = Fake {
        agents: vec![agent("sid-demo-c1", Some(4242), "interactive")],
        ..Fake::default()
    };
    let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
    assert_eq!(r, Stopped::Pane);
    assert_eq!(
        run.calls(),
        [Call::ClosePane("sid-demo-c1".into()), Call::Terminate(4242)]
    );

    // A pane crook did not open (one `crook resume` ran in): signalled, left open.
    let (w, _) = spawned();
    set_pid(&w, 4242);
    let run = Fake {
        agents: vec![agent("sid-demo-c1", Some(4242), "interactive")],
        foreign_pane: true,
        ..Fake::default()
    };
    let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
    assert_eq!(r, Stopped::Signalled);
    assert!(r.session_gone());
    assert_eq!(
        run.calls(),
        [Call::ClosePane("sid-demo-c1".into()), Call::Terminate(4242)]
    );

    // A pane that will not close does not skip the signal; the error is reported.
    let (w, _) = spawned();
    set_pid(&w, 4242);
    let run = Fake {
        agents: vec![agent("sid-demo-c1", Some(4242), "interactive")],
        fail_close_pane: true,
        ..Fake::default()
    };
    let e = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap_err();
    assert!(e.to_string().contains("herdr failed"), "{e}");
    assert_eq!(
        run.calls(),
        [Call::ClosePane("sid-demo-c1".into()), Call::Terminate(4242)]
    );

    // A stale pid: a different pid, or none, runs the session: only archived.
    for agents in [
        vec![agent("sid-demo-c1", Some(9999), "interactive")],
        vec![agent("sid-demo-c1", None, "interactive")],
    ] {
        let (w, _) = spawned();
        set_pid(&w, 4242);
        let run = Fake {
            agents,
            ..Fake::default()
        };
        let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
        assert!(matches!(r, Stopped::ArchivedOnly(_)), "{r:?}");
        assert!(
            run.calls().is_empty(),
            "no signal, no stop: {:?}",
            run.calls()
        );
        assert!(archived(&w));
    }

    // No row for the session: gone, so its worktrees may go.
    for agents in [
        vec![agent("someone-else", Some(4242), "interactive")],
        Vec::new(),
    ] {
        let (w, _) = spawned();
        let run = Fake {
            agents,
            ..Fake::default()
        };
        let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
        assert_eq!(r, Stopped::NoSession);
        assert!(r.session_gone());
    }

    // claude stop failing: archived, and the error says the worktrees were kept.
    let (w, _) = spawned();
    let run = Fake {
        agents: vec![agent("sid-demo-c1", None, "background")],
        fail_stop_bg: true,
        ..Fake::default()
    };
    let e = refusal(archive_and_stop(&mut w.fx.store("demo"), "c1", &run));
    assert!(
        e.contains("archived") && e.contains("worktrees were kept"),
        "{e}"
    );
    assert!(archived(&w));

    // claude agents failing: only archived, never a blind signal.
    let (w, _) = spawned();
    set_pid(&w, 4242);
    let run = Fake {
        agents_fail: true,
        ..Fake::default()
    };
    let r = archive_and_stop(&mut w.fx.store("demo"), "c1", &run).unwrap();
    assert!(matches!(r, Stopped::ArchivedOnly(m) if m.contains("boom")));
    assert!(run.calls().is_empty());
    assert!(archived(&w));
}
