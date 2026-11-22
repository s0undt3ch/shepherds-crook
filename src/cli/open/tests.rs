use std::ffi::OsString;

use super::*;
use crate::agents::Agent;
use crate::launch::{Plan, fixture::Fx};
use crate::store::Touch;

fn arg(p: &Plan, flag: &str) -> Vec<String> {
    let a: Vec<String> = p
        .args
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    a.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].clone())
        .collect()
}

fn envv(p: &Plan, k: &str) -> Option<String> {
    p.env
        .iter()
        .find(|(a, _)| a == &OsString::from(k))
        .map(|(_, v)| v.to_string_lossy().into_owned())
}

fn ok_version() -> Result<String, Error> {
    Ok("2.1.329".into())
}

fn no_agents() -> Result<Vec<Agent>, Error> {
    Ok(Vec::new())
}

fn exec(o: Outcome) -> Plan {
    match o {
        Outcome::Exec(p) => p,
        Outcome::Say(s) => panic!("expected exec, got {s}"),
    }
}

fn add_repo_and_notes(fx: &Fx) {
    let mut st = fx.store("demo");
    st.write(Touch::PROJECT, |tx| {
        crate::project::add(tx, crate::project::ItemKind::Repo, "/work/repo")?;
        crate::project::add(tx, crate::project::ItemKind::Notes, "/vault/notes")?;
        Ok(())
    })
    .unwrap();
}

fn set_orchestrator(fx: &Fx, sid: &str, pid: u32) {
    fx.store("demo")
        .write(Touch::empty(), |tx| {
            crate::crew::record_orchestrator(tx, sid, pid)
        })
        .unwrap();
}

fn open(fx: &Fx, live: &dyn Fn(u32) -> bool) -> Result<Outcome, Error> {
    let env = fx.env();
    let agents: &dyn Fn() -> Result<Vec<Agent>, Error> = &no_agents;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "new-uuid".to_string();
    plan(&env, "demo", &fx.host(live, agents, version, &new_id))
}

#[test]
fn open_builds_the_orchestrator_launch() {
    let fx = Fx::new();
    fx.init("demo");
    add_repo_and_notes(&fx);
    let p = exec(open(&fx, &|_| false).unwrap());
    let home = fx.home("demo");
    assert_eq!(p.program, fx.claude());
    assert_eq!(p.cwd, home);
    assert!(home.is_dir());
    assert_eq!(arg(&p, "-n"), ["crook-demo"]);
    assert_eq!(arg(&p, "--session-id"), ["new-uuid"]);
    assert!(arg(&p, "--resume").is_empty());
    assert_eq!(arg(&p, "--permission-mode"), ["manual"]);
    let plugin = crate::paths::plugin_dir(&fx.env());
    assert_eq!(arg(&p, "--plugin-dir"), [plugin.display().to_string()]);
    assert!(plugin.join("prompts/orchestrator.md").is_file());
    assert_eq!(
        arg(&p, "--append-system-prompt"),
        [include_str!("../../../plugin/prompts/orchestrator.md")]
    );
    // The diary is inside the cwd, so it is not added; the other notes folder and the repo are.
    assert_eq!(arg(&p, "--add-dir"), ["/vault/notes", "/work/repo"]);
    let settings = arg(&p, "--settings");
    let want = fx
        .tmp
        .path()
        .join("state/crook/demo/settings-orchestrator.json");
    assert_eq!(settings, [want.display().to_string()]);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&want).unwrap()).unwrap();
    let state = format!(
        "//{}/**",
        fx.tmp
            .path()
            .join("state/crook")
            .display()
            .to_string()
            .trim_start_matches('/')
    );
    assert_eq!(json["permissions"]["deny"][0], format!("Read({state})"));
    assert_eq!(
        json["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "/abs/crook"
    );
    // Environment: identity set, leaks gone.
    assert_eq!(envv(&p, "CROOK_PROJECT").as_deref(), Some("demo"));
    assert_eq!(envv(&p, "CROOK_ROLE").as_deref(), Some("orchestrator"));
    assert_eq!(envv(&p, "CROOK_BIN").as_deref(), Some("/abs/crook"));
    for gone in ["CROOK_CHILD", "CLAUDECODE", "CLAUDE_CODE_MESSAGING_SOCKET"] {
        assert!(envv(&p, gone).is_none(), "{gone}");
    }
    // Nothing is written to the orchestrator row before claude starts.
    let row = fx.store("demo").read(crate::crew::orchestrator).unwrap();
    assert_eq!(row, (None, None));
}

#[test]
fn open_warns_about_trust_on_the_first_open_only() {
    let fx = Fx::new();
    fx.init("demo");
    let first = format!(
        "crook: Claude Code will ask you to trust {} the first time; accept it once.",
        fx.home("demo").display()
    );
    let p = exec(open(&fx, &|_| false).unwrap());
    assert!(p.notes.contains(&first), "{:?}", p.notes);
    set_orchestrator(&fx, "old-sid", 99);
    let p = exec(open(&fx, &|_| false).unwrap());
    assert!(
        !p.notes.iter().any(|n| n.contains("trust")),
        "{:?}",
        p.notes
    );
}

#[test]
fn open_always_warns_about_unclosed_mcp_tools() {
    let fx = Fx::new();
    fx.init("demo");
    let p = exec(open(&fx, &|_| false).unwrap());
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("MCP tools able to read other projects")),
        "{:?}",
        p.notes
    );
}

#[test]
fn open_live_focuses_its_herdr_pane() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    fx.init("demo");
    set_orchestrator(&fx, "live-sid", 4242);
    let herdr = fx.tmp.path().join("bin/herdr");
    let log = fx.tmp.path().join("herdr.log");
    std::fs::write(
        &herdr,
        format!(
            "#!/bin/sh\necho \"$@\" >> {}\n\
             case \"$*\" in\n\
             'pane list') printf '%s' '{{\"result\":{{\"panes\":[{{\"pane_id\":\"w1:p4\"}}]}}}}';;\n\
             'pane process-info'*) printf '%s' '{{\"result\":{{\"process_info\":{{\"foreground_processes\":[{{\"pid\":4242}}]}}}}}}';;\n\
             *) printf '%s' '{{\"result\":{{\"type\":\"ok\"}}}}';;\n\
             esac\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&herdr, std::fs::Permissions::from_mode(0o755)).unwrap();
    let base = fx.env();
    let env = move |k: &str| match k {
        "HERDR_ENV" => Some("1".to_string()),
        _ => base(k),
    };
    let agents: &dyn Fn() -> Result<Vec<Agent>, Error> = &no_agents;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "new-uuid".to_string();
    let live: &dyn Fn(u32) -> bool = &|p| p == 4242;
    match plan(&env, "demo", &fx.host(live, agents, version, &new_id)).unwrap() {
        Outcome::Say(s) => assert!(s.contains("focused its pane w1:p4"), "{s}"),
        Outcome::Exec(_) => panic!("started a second orchestrator"),
    }
    let calls = std::fs::read_to_string(log).unwrap();
    assert!(calls.lines().any(|l| l == "agent focus w1:p4"), "{calls}");
}

/// A `herdr` in the fixture's `PATH` that logs its argv and lists workspace
/// `ws` (`None`: none) as `crook-demo`. Returns the log and an env for a
/// pane in workspace `here`.
fn fake_herdr(
    fx: &Fx,
    listed: Option<&str>,
    here: &'static str,
) -> (std::path::PathBuf, impl Fn(&str) -> Option<String> + use<>) {
    use std::os::unix::fs::PermissionsExt;
    let log = fx.tmp.path().join("herdr.log");
    let workspaces = listed.map_or(String::new(), |id| {
        format!("{{\"label\":\"crook-demo\",\"workspace_id\":\"{id}\"}}")
    });
    let bin = fx.tmp.path().join("bin/herdr");
    std::fs::write(
        &bin,
        format!(
            "#!/bin/sh\necho \"$@\" >> {}\n\
             case \"$*\" in\n\
             'workspace list') printf '%s' '{{\"result\":{{\"workspaces\":[{workspaces}]}}}}';;\n\
             'workspace create'*) printf '%s' '{{\"result\":{{\"workspace\":{{\"workspace_id\":\"w9\"}},\"tab\":{{\"tab_id\":\"w9:t1\"}},\"root_pane\":{{\"pane_id\":\"w9:p1\"}}}}}}';;\n\
             'tab create'*) printf '%s' '{{\"result\":{{\"tab\":{{\"tab_id\":\"w5:t3\"}},\"root_pane\":{{\"pane_id\":\"w5:p7\"}}}}}}';;\n\
             *) printf '%s' '{{\"result\":{{\"type\":\"ok\"}}}}';;\n\
             esac\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let base = fx.env();
    let env = move |k: &str| match k {
        "HERDR_ENV" => Some("1".to_string()),
        "HERDR_WORKSPACE_ID" => Some(here.to_string()),
        "ANTHROPIC_API_KEY" => Some("sk-secret".to_string()),
        _ => base(k),
    };
    (log, env)
}

fn herdr_calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn open_in(fx: &Fx, env: &dyn Fn(&str) -> Option<String>) -> Result<Outcome, Error> {
    let agents: &dyn Fn() -> Result<Vec<Agent>, Error> = &no_agents;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "new-uuid".to_string();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    plan(env, "demo", &fx.host(live, agents, version, &new_id))
}

#[test]
fn open_in_herdr_moves_to_project_workspace() {
    let fx = Fx::new();
    fx.init("demo");
    // No `crook-demo` workspace yet, and this pane is in another one.
    let (log, env) = fake_herdr(&fx, None, "w1");
    let home = fx.home("demo");
    match open_in(&fx, &env).unwrap() {
        Outcome::Say(s) => assert!(
            s.contains("crook-demo") && s.contains("w9:t1") && s.contains('\u{1F415}'),
            "{s}"
        ),
        Outcome::Exec(_) => panic!("exec'd claude in the invoking pane"),
    }
    let launch = "env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD";
    let calls = herdr_calls(&log);
    assert_eq!(
        calls,
        [
            // Once for "is this pane already there?", once to find or make it.
            "workspace list".to_string(),
            "workspace list".to_string(),
            format!(
                "workspace create --label crook-demo --cwd {} --no-focus",
                home.display()
            ),
            "tab rename w9:t1 \u{1F415}".to_string(),
            format!(
                "pane run w9:p1 {launch} XDG_STATE_HOME='{0}/state' XDG_DATA_HOME='{0}/data' \
                 '/abs/crook' open demo",
                quote_free(fx.tmp.path())
            ),
            "tab focus w9:t1".to_string(),
        ]
    );
    assert!(
        !calls.iter().any(|c| c.contains("sk-secret")),
        "no secrets are typed: {calls:?}"
    );
    // Nothing of the claude start was prepared here: the inner `open` does it.
    assert!(
        !fx.tmp
            .path()
            .join("state/crook/demo/settings-orchestrator.json")
            .exists()
    );
}

/// The fixture's temp dir as a bare shell word (a temp dir has no odd characters).
fn quote_free(p: &Path) -> String {
    let s = p.display().to_string();
    assert!(
        s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_.-".contains(c))
    );
    s
}

#[test]
fn open_in_herdr_uses_a_new_tab_in_an_existing_workspace_and_the_config_label() {
    let fx = Fx::new();
    fx.init("demo");
    let cfg = crate::paths::config_file(&fx.env());
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(&cfg, "[herdr]\norchestrator_label = \"boss\"\n").unwrap();
    let (log, env) = fake_herdr(&fx, Some("w5"), "w1");
    assert!(matches!(open_in(&fx, &env).unwrap(), Outcome::Say(_)));
    let calls = herdr_calls(&log);
    assert_eq!(calls[..2], ["workspace list", "workspace list"]);
    assert_eq!(
        calls[2],
        format!(
            "tab create --workspace w5 --cwd {} --label boss --no-focus",
            fx.home("demo").display()
        )
    );
    assert!(calls[3].starts_with("pane run w5:p7 env "), "{calls:?}");
    assert_eq!(calls[4], "tab focus w5:t3");
}

#[test]
fn open_in_project_workspace_runs_in_place() {
    let fx = Fx::new();
    fx.init("demo");
    let (log, env) = fake_herdr(&fx, Some("w5"), "w5");
    let here = exec(open_in(&fx, &env).unwrap());
    let plain = exec(open(&fx, &|_| false).unwrap());
    assert_eq!(
        (&here.program, &here.args, &here.env, &here.cwd),
        (&plain.program, &plain.args, &plain.env, &plain.cwd)
    );
    // Only the workspace was looked up.
    assert_eq!(herdr_calls(&log), ["workspace list"]);
}

#[test]
fn open_in_herdr_with_a_failing_herdr_runs_in_place_with_a_warning() {
    let fx = Fx::new();
    fx.init("demo");
    let (_log, env) = fake_herdr(&fx, Some("w5"), "w5");
    std::fs::write(fx.tmp.path().join("bin/herdr"), "#!/bin/sh\nexit 1\n").unwrap();
    let p = exec(open_in(&fx, &env).unwrap());
    assert!(
        p.notes.iter().any(|n| n.contains("herdr workspace")),
        "{:?}",
        p.notes
    );
}

#[test]
fn open_live_attaches() {
    let fx = Fx::new();
    fx.init("demo");
    set_orchestrator(&fx, "live-sid", 4242);
    match open(&fx, &|p| p == 4242).unwrap() {
        Outcome::Say(s) => {
            assert!(s.contains("4242") && s.contains("live-sid"), "{s}");
            assert!(s.contains("already running"), "{s}");
        }
        Outcome::Exec(_) => panic!("started a second orchestrator"),
    }
    // Nothing was launched or written.
    assert!(
        !fx.tmp
            .path()
            .join("state/crook/demo/settings-orchestrator.json")
            .exists()
    );
    assert!(!fx.tmp.path().join("data/crook-plugin").exists());
}

#[test]
fn open_resumes_an_orchestrator_whose_process_is_gone() {
    let fx = Fx::new();
    fx.init("demo");
    set_orchestrator(&fx, "old-sid", 4242);
    let p = exec(open(&fx, &|_| false).unwrap());
    assert_eq!(arg(&p, "--resume"), ["old-sid"]);
    assert!(arg(&p, "--session-id").is_empty());
}

#[test]
fn open_refuses_an_unknown_project_and_creates_nothing() {
    let fx = Fx::new();
    let e = open(&fx, &|_| false).unwrap_err();
    assert!(e.to_string().contains("crook init demo"), "{e}");
    assert!(!fx.tmp.path().join("state").exists());
    assert!(!fx.tmp.path().join("data").exists());
}

#[test]
fn open_refuses_an_old_claude_and_a_missing_one() {
    let fx = Fx::new();
    fx.init("demo");
    let env = fx.env();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    let agents: &dyn Fn() -> Result<Vec<Agent>, Error> = &no_agents;
    let old = || Ok("2.1.317".to_string());
    let new_id = || "x".to_string();
    let e = plan(&env, "demo", &fx.host(live, agents, &old, &new_id)).unwrap_err();
    assert!(e.to_string().contains("2.1.318"), "{e}");
    let missing = || Err(Error::Agents("claude not found on PATH".into()));
    let e = plan(&env, "demo", &fx.host(live, agents, &missing, &new_id)).unwrap_err();
    assert!(e.to_string().contains("claude"), "{e}");
}

#[test]
fn open_reconciles_and_survives_a_failing_agents_list() {
    let fx = Fx::new();
    fx.init("demo");
    fx.add_child(
        "demo",
        "c1",
        std::path::Path::new("/no/such/worktree"),
        "s1",
        Some(7),
    );
    let env = fx.env();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "x".to_string();
    let down = || Err(Error::Agents("timed out after 5 s".into()));
    let p = exec(plan(&env, "demo", &fx.host(live, &down, version, &new_id)).unwrap());
    let all = p.notes.join("\n");
    assert!(
        all.contains("c1") && all.contains("/no/such/worktree"),
        "{all}"
    );
    assert!(all.contains("timed out"), "{all}");
    // The stale pid was forgotten.
    let pid = fx
        .store("demo")
        .read(|c| crate::crew::claude_pid(c, "c1"))
        .unwrap();
    assert_eq!(pid, None);
}

fn agent(kind: &str, sid: &str, cwd: &str) -> Agent {
    Agent {
        session_id: sid.into(),
        pid: Some(9),
        cwd: cwd.into(),
        kind: kind.into(),
        state: None,
    }
}

fn open_with_agents(fx: &Fx, agents: Vec<Agent>) -> Result<Outcome, Error> {
    let env = fx.env();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "new-uuid".to_string();
    let list = move || Ok(agents.clone());
    plan(&env, "demo", &fx.host(live, &list, version, &new_id))
}

#[test]
fn open_does_not_start_a_second_orchestrator_before_the_row_exists() {
    let fx = Fx::new();
    fx.init("demo");
    // A first orchestrator is up (maybe at the trust dialog): no row yet.
    let home = fx.home("demo").display().to_string();
    match open_with_agents(&fx, vec![agent("interactive", "first-sid", &home)]).unwrap() {
        Outcome::Say(s) => {
            assert!(
                s.contains("first-sid") && s.contains("already running"),
                "{s}"
            );
        }
        Outcome::Exec(_) => panic!("started a second orchestrator"),
    }
    assert!(
        !fx.tmp
            .path()
            .join("state/crook/demo/settings-orchestrator.json")
            .exists()
    );
    // Other sessions do not block it: another folder, or a background session at the home.
    for a in [
        agent("interactive", "s", "/somewhere/else"),
        agent("background", "s", &home),
    ] {
        assert!(matches!(
            open_with_agents(&fx, vec![a]).unwrap(),
            Outcome::Exec(_)
        ));
    }
}

#[test]
fn open_refusal_migrates_nothing() {
    let fx = Fx::new();
    let dir = fx.tmp.path().join("state/crook/demo");
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("crook.db");
    drop(rusqlite::Connection::open(&db).unwrap());
    let env = fx.env();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    let agents: &dyn Fn() -> Result<Vec<Agent>, Error> = &no_agents;
    let old = || Ok("2.1.317".to_string());
    let new_id = || "x".to_string();
    let e = plan(&env, "demo", &fx.host(live, agents, &old, &new_id)).unwrap_err();
    assert!(e.to_string().contains("2.1.318"), "{e}");
    let c = rusqlite::Connection::open(&db).unwrap();
    let v: i32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    let tables: i32 = c
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert_eq!((v, tables), (0, 0));
}

fn open_notes(fx: &Fx, agents: Vec<Agent>) -> String {
    match open_with_agents(fx, agents).unwrap() {
        Outcome::Exec(p) => p.notes.join("\n"),
        Outcome::Say(s) => panic!("expected exec, got {s}"),
    }
}

/// A child whose claude never started holds a slot silently unless open
/// says so, with the remedy; one with a running session is not named.
#[test]
fn open_reports_a_child_with_no_running_session() {
    let fx = Fx::new();
    fx.init("demo");
    let wt = fx.tmp.path().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    fx.add_child("demo", "c1", &wt, "s1", None);
    fx.add_child("demo", "c2", &wt, "s2", None);
    let running = Agent {
        pid: None,
        ..agent("background", "s2", "/x")
    };
    let all = open_notes(&fx, vec![running]);
    assert!(
        all.contains("child c1: no session is running")
            && all.contains("crook resume demo/c1")
            && all.contains("crook stop demo/c1"),
        "{all}"
    );
    assert!(!all.contains("child c2"), "{all}");
    // When `claude agents` cannot say, nobody is called missing.
    let env = fx.env();
    let live: &dyn Fn(u32) -> bool = &|_| false;
    let version: &dyn Fn() -> Result<String, Error> = &ok_version;
    let new_id = || "x".to_string();
    let down = || Err(Error::Agents("timed out after 5 s".into()));
    let p = exec(plan(&env, "demo", &fx.host(live, &down, version, &new_id)).unwrap());
    assert!(!p.notes.join("\n").contains("no session is running"));
}

/// `worktrees` rows are checked too: a path gone from disk (a failed
/// `git worktree add`), and a worktree an archived child kept. Nothing is
/// deleted.
#[test]
fn open_reports_worktree_rows() {
    let fx = Fx::new();
    fx.init("demo");
    let base = fx.tmp.path();
    let (wt, kept) = (base.join("wt"), base.join("kept"));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&kept).unwrap();
    fx.add_child("demo", "c1", &wt, "s1", None);
    fx.add_child("demo", "old", &kept, "s9", None);
    let gone = base.join("wt-failed").display().to_string();
    fx.store("demo")
        .write(Touch::CHILDREN, |tx| {
            for (child, path) in [("c1", gone.clone()), ("old", kept.display().to_string())] {
                crew::add_worktree(
                    tx,
                    &crew::Worktree {
                        child: child.into(),
                        repo: "/r".into(),
                        path,
                        branch: "b".into(),
                    },
                )?;
            }
            crew::archive(tx, "old")
        })
        .unwrap();
    let all = open_notes(&fx, vec![agent("background", "s1", "/x")]);
    assert!(
        all.contains(&gone) && all.contains("crook stop demo/c1"),
        "{all}"
    );
    assert!(
        all.contains(&kept.display().to_string()) && all.contains("crook stop demo/old again"),
        "{all}"
    );
    let rows = fx.store("demo").read(|c| crew::worktrees(c, None)).unwrap();
    assert_eq!(rows.len(), 2, "open deletes nothing");
}

#[test]
fn open_of_a_clean_project_writes_nothing_and_leaves_gen_alone() {
    let fx = Fx::new();
    fx.init("demo");
    let gen_file = fx.tmp.path().join("state/crook/demo/gen");
    let before = std::fs::read_to_string(&gen_file).unwrap();
    exec(open(&fx, &|_| false).unwrap());
    assert_eq!(std::fs::read_to_string(&gen_file).unwrap(), before);
    // A stale pid is a change: it is cleared and `gen` moves.
    fx.add_child("demo", "c1", fx.tmp.path(), "s1", Some(7));
    let before = std::fs::read_to_string(&gen_file).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    exec(open(&fx, &|_| false).unwrap());
    assert_ne!(std::fs::read_to_string(&gen_file).unwrap(), before);
    assert_eq!(
        fx.store("demo")
            .read(|c| crate::crew::claude_pid(c, "c1"))
            .unwrap(),
        None
    );
}
