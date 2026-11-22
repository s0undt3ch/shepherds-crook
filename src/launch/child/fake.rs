use std::cell::RefCell;
use std::collections::BTreeSet;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Add(PathBuf, PathBuf, String),
    Remove(PathBuf, PathBuf),
    Prune(PathBuf),
    Start(Mode, StartCmd),
    StopBg(String),
    Terminate(u32),
    ClosePane(String),
}

/// A launcher that records what it was asked and runs nothing.
#[derive(Default)]
pub struct Fake {
    pub calls: RefCell<Vec<Call>>,
    pub fail_add: bool,
    pub fail_start: bool,
    pub dirty: BTreeSet<PathBuf>,
    pub agents: Vec<Agent>,
    pub agents_fail: bool,
    /// What `worktree_list` shows; `None` lists every `<repo>/.worktrees/*/*` on disk.
    pub listed: Option<Vec<PathBuf>>,
    pub fail_stop_bg: bool,
    pub fail_close_pane: bool,
    /// What `close_pane` reports; `Default` makes it `Closed`.
    pub pane: PaneClose,
    /// What `branches` shows for each repo.
    pub branch_list: std::collections::BTreeMap<PathBuf, Vec<String>>,
    /// Runs inside `stop_bg` and `agents`, to change the disk mid-stop.
    pub on_stop: Option<Box<dyn Fn()>>,
    /// Sessions stay listed after `stop_bg` or `terminate`, as one that
    /// has not exited yet would.
    pub sticky: bool,
}

impl Fake {
    pub fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
    pub fn starts(&self) -> Vec<(Mode, StartCmd)> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Start(m, s) => Some((m, s)),
                _ => None,
            })
            .collect()
    }
    fn log(&self, c: Call) {
        self.calls.borrow_mut().push(c);
    }
}

impl Launcher for Fake {
    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<(), Error> {
        self.log(Call::Add(repo.into(), path.into(), branch.into()));
        if self.fail_add {
            return Err(Error::Refused("git failed".into()));
        }
        Ok(())
    }
    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), Error> {
        self.log(Call::Remove(repo.into(), path.into()));
        Ok(())
    }
    fn worktree_list(&self, repo: &Path) -> Result<Vec<PathBuf>, Error> {
        if let Some(l) = &self.listed {
            return Ok(l.clone());
        }
        let mut out = vec![repo.to_path_buf()];
        for slug in std::fs::read_dir(repo.join(".worktrees"))
            .into_iter()
            .flatten()
            .flatten()
        {
            for wt in std::fs::read_dir(slug.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                out.push(wt.path());
            }
        }
        Ok(out)
    }
    fn worktree_dirty(&self, path: &Path) -> Result<bool, Error> {
        Ok(self.dirty.contains(path))
    }
    fn worktree_prune(&self, repo: &Path) -> Result<(), Error> {
        self.log(Call::Prune(repo.into()));
        Ok(())
    }
    fn branches(&self, repo: &Path, prefix: &str) -> Result<Vec<String>, Error> {
        Ok(self
            .branch_list
            .get(repo)
            .into_iter()
            .flatten()
            .filter(|b| b.starts_with(prefix))
            .cloned()
            .collect())
    }
    fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error> {
        self.log(Call::Start(mode, cmd.clone()));
        if self.fail_start {
            return Err(Error::Refused("claude failed".into()));
        }
        Ok(())
    }
    fn agents(&self) -> Result<Vec<Agent>, Error> {
        if let Some(f) = &self.on_stop {
            f();
        }
        if self.agents_fail {
            return Err(Error::Agents("boom".into()));
        }
        let calls = self.calls();
        let stopped = |a: &Agent| {
            calls.iter().any(|c| match c {
                Call::StopBg(s) => *s == a.session_id,
                Call::Terminate(p) => a.pid == Some(*p),
                _ => false,
            })
        };
        Ok(self
            .agents
            .iter()
            .filter(|a| self.sticky || !stopped(a))
            .cloned()
            .collect())
    }
    fn stop_bg(&self, session_id: &str) -> Result<(), Error> {
        self.log(Call::StopBg(session_id.into()));
        if self.fail_stop_bg {
            return Err(Error::Refused("claude stop failed".into()));
        }
        Ok(())
    }
    fn terminate(&self, pid: u32) -> Result<(), Error> {
        self.log(Call::Terminate(pid));
        Ok(())
    }
    fn close_pane(&self, session_id: &str, _child: &str) -> Result<PaneClose, Error> {
        self.log(Call::ClosePane(session_id.into()));
        if self.fail_close_pane {
            return Err(Error::Refused("herdr failed".into()));
        }
        Ok(self.pane)
    }
    fn exit_wait(&self) -> Duration {
        Duration::ZERO
    }
}
