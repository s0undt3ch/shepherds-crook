use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::store::{Mode, Store, Touch};

/// A temp machine: XDG dirs, a fake `claude` on `PATH`, one initialised project.
pub struct Fx {
    pub tmp: tempfile::TempDir,
    vars: HashMap<String, String>,
}

impl Fx {
    pub fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().display().to_string();
        let bin = tmp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = bin.join("claude");
        std::fs::write(&claude, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        let vars = HashMap::from([
            ("HOME".to_string(), format!("{d}/h")),
            ("XDG_DATA_HOME".to_string(), format!("{d}/data")),
            ("XDG_STATE_HOME".to_string(), format!("{d}/state")),
            ("XDG_CONFIG_HOME".to_string(), format!("{d}/cfg")),
            ("CLAUDE_CONFIG_DIR".to_string(), format!("{d}/claude")),
            ("PATH".to_string(), bin.display().to_string()),
        ]);
        Fx { tmp, vars }
    }

    pub fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        let v = self.vars.clone();
        move |k: &str| v.get(k).cloned()
    }

    pub fn claude(&self) -> PathBuf {
        self.tmp.path().join("bin/claude")
    }

    pub fn init(&self, slug: &str) {
        crate::cli::init::run(&self.env(), slug).unwrap();
    }

    pub fn home(&self, slug: &str) -> PathBuf {
        self.tmp.path().join("data/crook").join(slug)
    }

    pub fn store(&self, slug: &str) -> Store {
        Store::open(&self.env(), slug, Mode::ReadWrite).unwrap()
    }

    pub fn add_child(&self, slug: &str, child: &str, worktree: &Path, sid: &str, pid: Option<u32>) {
        self.store(slug)
            .write(Touch::CHILDREN, |tx| {
                crew::add_child(
                    tx,
                    &crew::Child {
                        child: child.into(),
                        repo: "/r".into(),
                        worktree: worktree.display().to_string(),
                        branch: "b".into(),
                        brief: "x".into(),
                        session_id: sid.into(),
                    },
                )?;
                if let Some(p) = pid {
                    tx.execute(
                        "UPDATE children SET claude_pid = ?2 WHERE child = ?1",
                        (child, p),
                    )?;
                }
                Ok(())
            })
            .unwrap();
    }

    pub fn host<'a>(
        &self,
        live: &'a dyn Fn(u32) -> bool,
        agents: &'a dyn Fn() -> Result<Vec<Agent>, Error>,
        version: &'a dyn Fn() -> Result<String, Error>,
        new_id: &'a dyn Fn() -> String,
    ) -> Host<'a> {
        Host {
            base_env: vec![
                ("PATH".into(), self.vars["PATH"].clone().into()),
                ("CROOK_CHILD".into(), "leak".into()),
                ("CLAUDECODE".into(), "1".into()),
                ("CLAUDE_CODE_MESSAGING_SOCKET".into(), "/x".into()),
            ],
            crook_bin: "/abs/crook".into(),
            live,
            new_id,
            version,
            agents,
            pane_of_pid: None,
            managed_dirs: Vec::new(),
        }
    }
}
