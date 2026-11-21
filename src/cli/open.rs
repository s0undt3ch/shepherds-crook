//! `crook open <slug>`: starts the project's orchestrator.

use std::path::Path;

use crate::gate::{Identity, Role};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config, crew, paths, project};

const PROMPT: &str = include_str!("../../plugin/prompts/orchestrator.md");

/// What `open` does for `slug`: attach-by-message when its orchestrator runs,
/// else the `claude` start. Nothing is written to the orchestrator row here:
/// the SessionStart hook records it once claude is up, so a `claude` that fails
/// to start leaves no session id to resume.
pub fn plan(env: Env, slug: &str, host: &Host) -> Result<Outcome, Error> {
    paths::valid_slug(slug)?;
    launch::require_project(env, slug)?;
    let cfg = config::load(env)?;
    let home = cfg.project_home(env, slug)?;
    let diary = cfg.diary(env, slug)?;
    let mut store = Store::open(env, slug, Mode::Migrate)?;
    let (session_id, pid) = store.read(crew::orchestrator)?;
    if let Some(pid) = pid.filter(|p| (host.live)(*p)) {
        return Ok(Outcome::Say(format!(
            "crook: the orchestrator for {slug} is already running (pid {pid}, session {}); \
             not starting a second one",
            session_id.as_deref().unwrap_or("unknown")
        )));
    }

    let mut notes = Vec::new();
    if session_id.is_none() {
        notes.push(format!(
            "crook: Claude Code will ask you to trust {} the first time; accept it once.",
            home.display()
        ));
    }
    notes.extend(launch::preflight_notes(env, host)?);

    let agents = match (host.agents)() {
        Ok(a) => a,
        Err(e) => {
            notes.push(format!(
                "crook: warning: {e}; sessions running without a row were not checked"
            ));
            Vec::new()
        }
    };
    let lines = store.write(Touch::CHILDREN, |tx| {
        launch::reconcile_with(tx, &agents, host.live)
    })?;
    notes.extend(lines.into_iter().map(|l| format!("crook: {l}")));
    notes.push(
        "crook: warning: MCP tools able to read other projects (a loam plot reader, say) \
         are not closed"
            .into(),
    );

    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(&diary)?;
    let prep = launch::prepare(env, slug, Role::Orchestrator, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let outside = record
        .notes
        .iter()
        .filter(|n| !Path::new(n).starts_with(&home))
        .map(std::path::PathBuf::from);
    let add_dirs = launch::dedup_dirs(outside.chain(record.repos.iter().map(Into::into)));
    let session = match session_id {
        Some(id) => Session::Resume(id),
        None => Session::New((host.new_id)()),
    };
    let args = launch::argv(&LaunchOpts {
        name: format!("crook-{slug}"),
        session,
        permission_mode: prep.permission_mode.into(),
        plugin_dir: prep.plugin_dir,
        system_prompt: PROMPT.into(),
        settings: prep.settings,
        add_dirs,
    });
    let id = Identity {
        slug: slug.into(),
        role: Role::Orchestrator,
        child: None,
    };
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args,
        env: launch::env(host.base_env.iter().cloned(), &id, &host.crook_bin),
        cwd: home,
        notes,
    }))
}

#[cfg(test)]
mod tests {
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
            [include_str!("../../plugin/prompts/orchestrator.md")]
        );
        // The diary is inside the cwd, so it is not added; the other notes folder and the repo are.
        assert_eq!(arg(&p, "--add-dir"), ["/vault/notes", "/work/repo"]);
        let settings = arg(&p, "--settings");
        let want = fx
            .tmp
            .path()
            .join("state/crook/demo/settings-orchestrator.json");
        assert_eq!(settings, [want.display().to_string()]);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&want).unwrap()).unwrap();
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
}
