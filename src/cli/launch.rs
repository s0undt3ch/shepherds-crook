//! `crook launch <slug>/<child>`, hidden: what a herdr pane runs to start a
//! spawned child. The pane types only this command, so no environment value
//! or prompt text lands in the shell; the environment and argv come from the
//! child's rows.

use std::path::{Path, PathBuf};

use crate::gate::{Identity, Role};
use crate::launch::child::{self, Mode as Start};
use crate::launch::{self, Host, LaunchOpts, Outcome, Plan, Session};
use crate::store::{Mode, Store};
use crate::{Env, Error, crew, project};

use super::{address, resume};

/// The `claude` start of a child that has not run yet: the arguments a `--bg`
/// spawn gives it, without `--bg`, on its recorded session id and in its
/// worktree. A child that is running is refused; `crook resume` restarts one
/// that stopped.
pub fn plan(env: Env, arg: &str, host: &Host) -> Result<Outcome, Error> {
    let (slug, name) = address::resolve(env, arg)?;
    launch::require_project(env, &slug)?;
    let mut notes = launch::preflight_notes(env, host)?;
    let store = Store::open(env, &slug, Mode::Migrate)?;
    let row = store.read(|c| crew::resumable(c, &name))?;
    if row.archived {
        return Err(Error::Refused(format!("child {name} is archived")));
    }
    if !Path::new(&row.worktree).is_dir() {
        return Err(Error::Refused(format!(
            "child {name}: worktree {} is missing on disk",
            row.worktree
        )));
    }
    resume::ensure_not_running(&store, &name, &row, host, &mut notes)?;

    let prep = launch::prepare(env, &slug, Role::Child, &host.crook_bin)?;
    let record = store.read(project::get)?;
    let args = child::child_args(
        &LaunchOpts {
            name: format!("crook-{slug}-{name}"),
            session: Session::New(row.session_id),
            permission_mode: prep.permission_mode.into(),
            plugin_dir: prep.plugin_dir,
            system_prompt: child::PROMPT.into(),
            settings: prep.settings,
            add_dirs: launch::dedup_dirs(record.notes.iter().map(PathBuf::from)),
        },
        Start::Herdr,
        &child::launch_prompt(&slug, &name),
    );
    let id = Identity {
        slug: slug.clone(),
        role: Role::Child,
        child: Some(name),
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

    fn fixture() -> (Fx, PathBuf) {
        let fx = Fx::new();
        fx.init("demo");
        let wt = fx.tmp.path().join("repo/.worktrees/demo/c1");
        std::fs::create_dir_all(&wt).unwrap();
        fx.store("demo")
            .write(Touch::PROJECT, |tx| {
                project::add(tx, project::ItemKind::Notes, "/vault/notes")?;
                Ok(())
            })
            .unwrap();
        fx.add_child("demo", "c1", &wt, "child-sid", None);
        (fx, wt)
    }

    fn launch(
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

    fn exec(o: Outcome) -> Plan {
        match o {
            Outcome::Exec(p) => p,
            Outcome::Say(s) => panic!("expected exec, got {s}"),
        }
    }

    #[test]
    fn launch_env() {
        let (fx, wt) = fixture();
        let p = exec(launch(&fx, "demo/c1", &|_| false, vec![]).unwrap());
        let env = |k: &str| {
            p.env
                .iter()
                .find(|(a, _)| a == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(env("CROOK_PROJECT").as_deref(), Some("demo"));
        assert_eq!(env("CROOK_ROLE").as_deref(), Some("child"));
        assert_eq!(env("CROOK_CHILD").as_deref(), Some("c1"));
        assert_eq!(env("CROOK_BIN").as_deref(), Some("/abs/crook"));
        // The fixture's inherited environment carries a stray CROOK_CHILD,
        // CLAUDECODE and a messaging token; none survive.
        assert!(env("CLAUDECODE").is_none());
        assert!(env("CLAUDE_CODE_MESSAGING_SOCKET").is_none());
        assert_eq!(
            p.env.iter().filter(|(k, _)| k == "CROOK_CHILD").count(),
            1,
            "{:?}",
            p.env
        );
        assert_eq!(p.program, fx.claude());
        assert_eq!(p.cwd, wt);
    }

    #[test]
    fn launch_argv_is_the_spawn_argv_without_bg() {
        let (fx, _) = fixture();
        let p = exec(launch(&fx, "c1", &|_| false, vec![]).unwrap());
        let a: Vec<String> = p
            .args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let after = |flag: &str| -> Vec<&str> {
            a.windows(2)
                .filter(|w| w[0] == flag)
                .map(|w| w[1].as_str())
                .collect()
        };
        assert_eq!(after("--session-id"), ["child-sid"]);
        assert!(after("--resume").is_empty());
        assert_eq!(after("-n"), ["crook-demo-c1"]);
        assert!(!a.iter().any(|x| x == "--bg"));
        assert_eq!(
            after("--append-system-prompt"),
            [include_str!("../../plugin/prompts/child.md")]
        );
        let diary = fx.home("demo").join("diary").display().to_string();
        assert_eq!(after("--add-dir"), [diary.as_str(), "/vault/notes"]);
        assert_eq!(
            a[a.len() - 2..],
            [
                "--",
                "You are child c1 of crook-demo. Your project card is above."
            ]
        );
    }

    #[test]
    fn launch_refuses_a_running_child() {
        let (fx, _) = fixture();
        let bg = Agent {
            session_id: "child-sid".into(),
            pid: Some(77),
            cwd: "/x".into(),
            kind: "interactive".into(),
            state: Some("working".into()),
        };
        let e = launch(&fx, "c1", &|_| false, vec![bg]).unwrap_err();
        assert!(e.to_string().contains("already running"), "{e}");
        fx.store("demo")
            .write(Touch::CHILDREN, |tx| {
                tx.execute("UPDATE children SET claude_pid = 555", [])?;
                Ok(())
            })
            .unwrap();
        let e = launch(&fx, "c1", &|p| p == 555, vec![]).unwrap_err();
        assert!(e.to_string().contains("555"), "{e}");
    }

    #[test]
    fn launch_refuses_archived_missing_worktree_and_unknown_child() {
        let (fx, wt) = fixture();
        let e = launch(&fx, "demo/nope", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("nope"), "{e}");
        std::fs::remove_dir_all(&wt).unwrap();
        let e = launch(&fx, "c1", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("worktree"), "{e}");
        std::fs::create_dir_all(&wt).unwrap();
        fx.store("demo")
            .write(Touch::CHILDREN, |tx| crew::archive(tx, "c1"))
            .unwrap();
        let e = launch(&fx, "c1", &|_| false, vec![]).unwrap_err();
        assert!(e.to_string().contains("archived"), "{e}");
    }
}
