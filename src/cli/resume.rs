//! `crook resume <child>`: restarts a child's current session.

use std::path::{Path, PathBuf};

use crate::gate::{Identity, Role};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, crew, project};

use super::address;

const PROMPT: &str = include_str!("../../plugin/prompts/child.md");

/// Refuses a child whose session runs: a live recorded pid, or a session or
/// fork of it in `claude agents`. Without `claude agents`, a note says it
/// could not look.
pub(super) fn ensure_not_running(
    store: &Store,
    child: &str,
    row: &crew::Resumable,
    host: &Host,
    notes: &mut Vec<String>,
) -> Result<(), Error> {
    if let Some(pid) = row.claude_pid.filter(|p| (host.live)(*p)) {
        return Err(Error::Refused(format!(
            "child {child} is already running (claude pid {pid})"
        )));
    }
    match (host.agents)() {
        Ok(agents) => {
            let running = store.read(|c| {
                for a in &agents {
                    if a.session_id == row.session_id || crew::is_fork(c, child, &a.session_id)? {
                        return Ok(Some(a.session_id.clone()));
                    }
                }
                Ok(None)
            })?;
            if let Some(sid) = running {
                return Err(Error::Refused(format!(
                    "child {child} is already running (session {sid})"
                )));
            }
        }
        Err(e) => notes.push(format!(
            "crook: warning: {e}; a running session of {child} would not have been noticed"
        )),
    }
    Ok(())
}

/// The `claude` start for a child's current session, with the flags, environment
/// and cwd it was spawned with. Refuses a child that is running: forgetting the
/// recorded pid would lift the SessionStart guard that protects its row.
pub fn plan(env: Env, arg: &str, host: &Host) -> Result<Outcome, Error> {
    let (slug, child) = address::resolve(env, arg)?;
    launch::require_project(env, &slug)?;
    let mut notes = launch::preflight_notes(env, host)?;
    let mut store = Store::open(env, &slug, Mode::Migrate)?;
    let row = store.read(|c| crew::resumable(c, &child))?;
    if row.archived {
        return Err(Error::Refused(format!("child {child} is archived")));
    }
    if !Path::new(&row.worktree).is_dir() {
        return Err(Error::Refused(format!(
            "child {child}: worktree {} is missing on disk",
            row.worktree
        )));
    }
    ensure_not_running(&store, &child, &row, host, &mut notes)?;
    store.write(Touch::CHILDREN, |tx| crew::clear_claude_pid(tx, &child))?;

    let prep = launch::prepare(env, &slug, Role::Child, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let add_dirs = launch::dedup_dirs(record.notes.iter().map(PathBuf::from));
    let args = launch::argv(&LaunchOpts {
        name: format!("crook-{slug}-{child}"),
        session: Session::Resume(row.session_id),
        permission_mode: prep.permission_mode.into(),
        plugin_dir: prep.plugin_dir,
        system_prompt: PROMPT.into(),
        settings: prep.settings,
        add_dirs,
    });
    let id = Identity {
        slug,
        role: Role::Child,
        child: Some(child),
    };
    Ok(Outcome::Exec(Plan {
        program: launch::claude_program(env)?,
        args,
        env: launch::env(host.base_env.iter().cloned(), &id, &host.crook_bin),
        cwd: PathBuf::from(row.worktree),
        notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Agent;
    use crate::launch::fixture::Fx;
    use crate::store::Touch;

    fn args(o: Outcome) -> (Vec<String>, crate::launch::Plan) {
        match o {
            Outcome::Exec(p) => (
                p.args
                    .iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect(),
                p,
            ),
            Outcome::Say(s) => panic!("expected exec, got {s}"),
        }
    }

    fn after(a: &[String], flag: &str) -> Vec<String> {
        a.windows(2)
            .filter(|w| w[0] == flag)
            .map(|w| w[1].clone())
            .collect()
    }

    fn resume(
        fx: &Fx,
        arg: &str,
        live: &dyn Fn(u32) -> bool,
        agents: Vec<Agent>,
    ) -> Result<Outcome, Error> {
        let env = fx.env();
        let version = || Ok("2.1.329".to_string());
        let list = move || Ok(agents.clone());
        let new_id = || "unused".to_string();
        plan(&env, arg, &fx.host(live, &list, &version, &new_id))
    }

    fn fixture() -> (Fx, std::path::PathBuf) {
        let fx = Fx::new();
        fx.init("demo");
        let wt = fx.tmp.path().join("repo/.worktrees/demo/c1");
        std::fs::create_dir_all(&wt).unwrap();
        fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                crate::project::add(tx, crate::project::ItemKind::Repo, "/work/repo")?;
                crate::project::add(tx, crate::project::ItemKind::Notes, "/vault/notes")?;
                Ok(())
            })
            .unwrap();
        fx.add_child("demo", "c1", &wt, "child-sid", Some(555));
        (fx, wt)
    }

    #[test]
    fn resume_argv() {
        let (fx, wt) = fixture();
        let (a, p) = args(resume(&fx, "c1", &|_| false, vec![]).unwrap());
        assert_eq!(after(&a, "--resume"), ["child-sid"]);
        assert!(after(&a, "--session-id").is_empty());
        assert_eq!(after(&a, "-n"), ["crook-demo-c1"]);
        assert_eq!(after(&a, "--permission-mode"), ["manual"]);
        assert_eq!(
            after(&a, "--settings"),
            [fx.tmp
                .path()
                .join("state/crook/demo/settings-child.json")
                .display()
                .to_string()]
        );
        assert_eq!(
            after(&a, "--append-system-prompt"),
            [include_str!("../../plugin/prompts/child.md")]
        );
        // Notes folders only: a child works in its own worktree, not the record's repos.
        let diary = fx.home("demo").join("diary").display().to_string();
        assert_eq!(after(&a, "--add-dir"), [diary, "/vault/notes".to_string()]);
        assert_eq!(p.cwd, wt);
        assert_eq!(p.program, fx.claude());
        let env = |k: &str| {
            p.env
                .iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(env("CROOK_ROLE").as_deref(), Some("child"));
        assert_eq!(env("CROOK_CHILD").as_deref(), Some("c1"));
        assert_eq!(env("CROOK_PROJECT").as_deref(), Some("demo"));
        assert!(env("CLAUDECODE").is_none());
        // The recorded pid is forgotten so the restarted claude can record itself.
        let pid = fx
            .store("demo")
            .read(|c| crate::crew::claude_pid(c, "c1"))
            .unwrap();
        assert_eq!(pid, None);
        // Slug-qualified names resolve too.
        assert!(resume(&fx, "demo/c1", &|_| false, vec![]).is_ok());
    }

    #[test]
    fn resume_refuses_a_running_child_and_keeps_its_pid() {
        let (fx, _) = fixture();
        let e = resume(&fx, "c1", &|p| p == 555, vec![]).unwrap_err();
        assert!(e.to_string().contains("555"), "{e}");
        let pid = fx
            .store("demo")
            .read(|c| crate::crew::claude_pid(c, "c1"))
            .unwrap();
        assert_eq!(pid, Some(555));
        // A background session has no pid, but `claude agents` lists it.
        let bg = Agent {
            session_id: "child-sid".into(),
            pid: None,
            cwd: "/x".into(),
            kind: "background".into(),
            state: Some("working".into()),
        };
        let e = resume(&fx, "c1", &|_| false, vec![bg]).unwrap_err();
        assert!(e.to_string().contains("running"), "{e}");
    }

    #[test]
    fn resume_refuses_archived_missing_worktree_and_unknown_child() {
        let (fx, wt) = fixture();
        let e = resume(&fx, "nope", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("nope"), "{e}");
        std::fs::remove_dir_all(&wt).unwrap();
        let e = resume(&fx, "c1", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("worktree"), "{e}");
        std::fs::create_dir_all(&wt).unwrap();
        fx.store("demo")
            .write(Touch::CHILDREN, |tx| crate::crew::archive(tx, "c1"))
            .unwrap();
        let e = resume(&fx, "c1", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("archived"), "{e}");
    }
}
