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
        [include_str!("../../../plugin/prompts/child.md")]
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
