use super::*;
use crate::gate::Role;

fn os(s: &str) -> OsString {
    OsString::from(s)
}

fn opts(session: Session) -> LaunchOpts {
    LaunchOpts {
        name: "crook-demo".into(),
        session,
        permission_mode: "manual".into(),
        plugin_dir: "/x/crook-plugin/0.1.0-abc".into(),
        system_prompt: "PROMPT".into(),
        settings: "/s/crook/demo/settings-orchestrator.json".into(),
        add_dirs: vec!["/notes".into(), "/repo".into()],
    }
}

#[test]
fn argv_orchestrator() {
    assert_eq!(
        argv(&opts(Session::New("uuid-1".into()))),
        [
            "-n",
            "crook-demo",
            "--session-id",
            "uuid-1",
            "--permission-mode",
            "manual",
            "--plugin-dir",
            "/x/crook-plugin/0.1.0-abc",
            "--append-system-prompt",
            "PROMPT",
            "--settings",
            "/s/crook/demo/settings-orchestrator.json",
            "--add-dir",
            "/notes",
            "--add-dir",
            "/repo",
        ]
        .map(os)
    );
}

#[test]
fn argv_resume_swaps_only_the_session_flag() {
    let a = argv(&opts(Session::Resume("old".into())));
    let b = argv(&opts(Session::New("uuid-1".into())));
    assert_eq!(a[2], os("--resume"));
    assert_eq!(a[3], os("old"));
    assert_eq!(a[4..], b[4..]);
}

fn base() -> Vec<(OsString, OsString)> {
    [
        ("PATH", "/bin"),
        ("HOME", "/h"),
        ("CROOK_CHILD", "c9"),
        ("CROOK_PROJECT", "other"),
        ("CROOK_ROLE", "child"),
        ("CROOK_BIN", "/old/crook"),
        ("CROOK_ANYTHING", "1"),
        ("CLAUDE_CODE_MESSAGING_SOCKET", "/tmp/s"),
        ("CLAUDE_CODE_MESSAGING_TOKEN", "t"),
        ("CLAUDECODE", "1"),
        ("ANTHROPIC_MODEL", "haiku"),
    ]
    .map(|(k, v)| (os(k), os(v)))
    .to_vec()
}

fn id(role: Role, child: Option<&str>) -> Identity {
    Identity {
        slug: "demo".into(),
        role,
        child: child.map(str::to_string),
    }
}

fn get<'a>(v: &'a [(OsString, OsString)], k: &str) -> Option<&'a OsString> {
    v.iter().find(|(a, _)| a == k).map(|(_, b)| b)
}

#[test]
fn env_scrub() {
    let out = env(
        base().into_iter(),
        &id(Role::Orchestrator, None),
        Path::new("/new/crook"),
    );
    assert_eq!(get(&out, "CROOK_PROJECT"), Some(&os("demo")));
    assert_eq!(get(&out, "CROOK_ROLE"), Some(&os("orchestrator")));
    assert_eq!(get(&out, "CROOK_BIN"), Some(&os("/new/crook")));
    for gone in [
        "CROOK_CHILD",
        "CROOK_ANYTHING",
        "CLAUDE_CODE_MESSAGING_SOCKET",
        "CLAUDE_CODE_MESSAGING_TOKEN",
        "CLAUDECODE",
    ] {
        assert!(get(&out, gone).is_none(), "{gone}");
    }
    // Everything else passes through, and no key is set twice.
    assert_eq!(get(&out, "ANTHROPIC_MODEL"), Some(&os("haiku")));
    assert_eq!(get(&out, "PATH"), Some(&os("/bin")));
    let mut keys: Vec<_> = out.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n);
}

#[test]
fn env_child_carries_its_id() {
    let out = env(
        base().into_iter(),
        &id(Role::Child, Some("c1")),
        Path::new("/new/crook"),
    );
    assert_eq!(get(&out, "CROOK_ROLE"), Some(&os("child")));
    assert_eq!(get(&out, "CROOK_CHILD"), Some(&os("c1")));
}

mod reconcile {
    use super::super::*;
    use crate::store::{Mode, Store, Touch};

    fn store() -> (tempfile::TempDir, Store) {
        let tmp = tempfile::tempdir().unwrap();
        let x = tmp.path().display().to_string();
        let env = move |k: &str| (k == "XDG_STATE_HOME").then(|| x.clone());
        crate::project::init(&env, "p").unwrap();
        let s = Store::open(&env, "p", Mode::ReadWrite).unwrap();
        (tmp, s)
    }

    fn child(s: &mut Store, name: &str, worktree: &str, sid: &str, pid: Option<i64>) {
        s.write(Touch::CHILDREN, |tx| {
            tx.execute(
                "INSERT INTO children(child, repo, worktree, branch, brief, session_id,
                                      claude_pid, created_at)
                 VALUES (?1, '/r', ?2, 'b', 'brief', ?3, ?4, 0)",
                (name, worktree, sid, pid),
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn agent(sid: &str, cwd: &str) -> Agent {
        Agent {
            session_id: sid.into(),
            pid: Some(1),
            cwd: cwd.into(),
            kind: "interactive".into(),
            state: None,
        }
    }

    #[test]
    fn missing_worktree_is_reported() {
        let (tmp, mut s) = store();
        let here = tmp.path().display().to_string();
        child(&mut s, "ok", &here, "s1", None);
        child(&mut s, "gone", "/no/such/worktree", "s2", None);
        let lines = s
            .write(Touch::CHILDREN, |tx| {
                reconcile_with(tx, "p", &[], &|_| false)
            })
            .unwrap();
        let missing: Vec<_> = lines.iter().filter(|l| l.contains("missing")).collect();
        assert_eq!(missing.len(), 1, "{lines:?}");
        assert!(missing[0].contains("gone") && missing[0].contains("/no/such/worktree"));
    }

    #[test]
    fn archived_child_is_not_reported() {
        let (_t, mut s) = store();
        child(&mut s, "gone", "/no/such/worktree", "s2", None);
        s.write(Touch::CHILDREN, |tx| crate::crew::archive(tx, "gone"))
            .unwrap();
        let lines = s
            .write(Touch::CHILDREN, |tx| {
                reconcile_with(tx, "p", &[], &|_| false)
            })
            .unwrap();
        assert!(lines.is_empty(), "{lines:?}");
    }

    #[test]
    fn unrecorded_running_session_is_flagged() {
        let (tmp, mut s) = store();
        let wt = tmp.path().display().to_string();
        child(&mut s, "c1", &wt, "known", None);
        let agents = [
            agent("known", &wt),
            agent("stranger", &format!("{wt}/sub")),
            agent("elsewhere", "/somewhere/else"),
        ];
        let lines = s
            .write(Touch::CHILDREN, |tx| {
                reconcile_with(tx, "p", &agents, &|_| false)
            })
            .unwrap();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("stranger") && lines[0].contains("c1"));
    }

    /// The spawn worktree in both tables, gone: one line, not two.
    #[test]
    fn a_missing_worktree_in_both_tables_is_reported_once() {
        let (_t, mut s) = store();
        child(&mut s, "c1", "/no/such/wt", "s1", None);
        s.write(Touch::CHILDREN, |tx| {
            crate::crew::add_worktree(
                tx,
                &crate::crew::Worktree {
                    child: "c1".into(),
                    repo: "/r".into(),
                    path: "/no/such/wt".into(),
                    branch: "b".into(),
                },
            )
        })
        .unwrap();
        let lines = s
            .write(Touch::CHILDREN, |tx| {
                reconcile_with(tx, "p", &[], &|_| false)
            })
            .unwrap();
        let n = lines.iter().filter(|l| l.contains("/no/such/wt")).count();
        assert_eq!(n, 1, "{lines:?}");
    }

    /// Panes are looked up live through herdr, for interactive sessions only.
    #[test]
    fn a_running_child_in_no_herdr_pane_is_reported() {
        let (tmp, mut s) = store();
        let wt = tmp.path().display().to_string();
        child(&mut s, "c1", &wt, "s1", None);
        let agents = [agent("s1", &wt)];
        let asked = std::cell::Cell::new(0);
        let none = |_: u32| {
            asked.set(asked.get() + 1);
            None
        };
        let some = |_: u32| Some("p-1".to_string());
        let lines = |pane: Option<&dyn Fn(u32) -> Option<String>>| {
            let look = Look {
                agents: Some(&agents),
                live: &|_| false,
                pane_of_pid: pane,
            };
            s.read(|c| findings(c, "p", &look)).unwrap().lines
        };
        let l = lines(Some(&none));
        assert!(
            l.iter()
                .any(|x| x.contains("c1") && x.contains("no herdr pane")),
            "{l:?}"
        );
        assert_eq!(asked.get(), 1);
        assert!(lines(Some(&some)).is_empty());
        assert!(lines(None).is_empty(), "no herdr, nothing to say");
    }

    #[test]
    fn a_stale_pid_is_cleared_and_a_live_one_kept() {
        let (tmp, mut s) = store();
        let wt = tmp.path().display().to_string();
        child(&mut s, "dead", &wt, "s1", Some(111));
        child(&mut s, "alive", &wt, "s2", Some(222));
        s.write(Touch::CHILDREN, |tx| {
            reconcile_with(tx, "p", &[], &|p| p == 222)
        })
        .unwrap();
        let pid = |c: &str| s.read(|conn| crate::crew::claude_pid(conn, c)).unwrap();
        assert_eq!(pid("dead"), None);
        assert_eq!(pid("alive"), Some(222));
    }
}
