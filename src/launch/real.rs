//! `git` and `claude` for real: the `Launcher` that `crook` itself uses.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::agents::{self, Agent};
use crate::{Env, Error, launch};

use super::child::{
    CLAUDE_START_TIMEOUT, GIT_TIMEOUT, Launcher, Mode, StartCmd, WORKTREE_ADD_TIMEOUT,
    parse_worktree_list,
};

/// `git` and `claude` on the injected `PATH`.
pub struct RealLauncher<'a> {
    env: Env<'a>,
}

impl<'a> RealLauncher<'a> {
    pub fn new(env: Env<'a>) -> Self {
        RealLauncher { env }
    }

    fn git(&self) -> Result<PathBuf, Error> {
        find_exe(self.env, "git").ok_or_else(|| Error::Refused("git not found on PATH".into()))
    }

    fn git_out(&self, args: &[&std::ffi::OsStr]) -> Result<String, Error> {
        self.git_within(args, GIT_TIMEOUT)
    }

    fn git_within(&self, args: &[&std::ffi::OsStr], timeout: Duration) -> Result<String, Error> {
        let mut cmd = Command::new(self.git()?);
        cmd.args(args);
        run_captured(cmd, timeout, "git")
    }
}

/// `claude stop <id>` without the caller's `CLAUDECODE`, `CROOK_*` or
/// `CLAUDE_CODE_MESSAGING_*`, as a start has none of them.
fn stop_command(claude: &Path, session_id: &str) -> Command {
    let mut c = Command::new(claude);
    c.args(["stop", session_id]);
    for (k, _) in std::env::vars_os().filter(|(k, _)| launch::scrubbed(k)) {
        c.env_remove(k);
    }
    c.env_remove("CLAUDECODE");
    c
}

fn find_exe(env: Env, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    env("PATH")?
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| PathBuf::from(d).join(name))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Runs `cmd` with no stdin and returns its stdout, or an error carrying its
/// stderr. Waits for the process, not for the pipes to close: a `claude --bg`
/// leaves a background session that may keep them open.
fn run_captured(mut cmd: Command, timeout: Duration, what: &str) -> Result<String, Error> {
    let fail = |msg: String| Error::Io(std::io::Error::other(format!("{what}: {msg}")));
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(e.to_string()))?;
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>)>();
    for (is_err, pipe) in [
        (
            false,
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
        (
            true,
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
    ] {
        if let Some(mut pipe) = pipe {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 || tx.send((is_err, buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            });
        }
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fail(format!("timed out after {} s", timeout.as_secs())));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => return Err(fail(e.to_string())),
        }
    };
    // What the process wrote before it exited; a pipe held open by a
    // descendant ends the wait at the deadline.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let end = Instant::now() + Duration::from_millis(300);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok((true, b)) => err.extend(b),
            Ok((false, b)) => out.extend(b),
            Err(_) => break,
        }
    }
    if !status.success() {
        let err = String::from_utf8_lossy(&err);
        let msg = err.trim();
        return Err(fail(if msg.is_empty() {
            format!("exited with {status}")
        } else {
            msg.chars().take(500).collect()
        }));
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

impl Launcher for RealLauncher<'_> {
    fn check(&self, mode: Mode) -> Result<(), Error> {
        match mode {
            Mode::Bg => Ok(()),
            Mode::Herdr => Err(Error::Refused("herdr panes arrive with Task 20".into())),
        }
    }

    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str) -> Result<(), Error> {
        use std::ffi::OsStr;
        self.git_within(
            &[
                "-C".as_ref(),
                repo.as_os_str(),
                "worktree".as_ref(),
                "add".as_ref(),
                path.as_os_str(),
                "-b".as_ref(),
                OsStr::new(branch),
            ],
            WORKTREE_ADD_TIMEOUT,
        )
        .map(drop)
    }

    fn worktree_remove(&self, repo: &Path, path: &Path) -> Result<(), Error> {
        self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "remove".as_ref(),
            path.as_os_str(),
        ])
        .map(drop)
    }

    fn worktree_list(&self, repo: &Path) -> Result<Vec<PathBuf>, Error> {
        let out = self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "list".as_ref(),
            "--porcelain".as_ref(),
        ])?;
        Ok(parse_worktree_list(&out))
    }

    fn worktree_dirty(&self, path: &Path) -> Result<bool, Error> {
        let out = self.git_out(&[
            "-C".as_ref(),
            path.as_os_str(),
            "status".as_ref(),
            "--porcelain".as_ref(),
        ])?;
        Ok(!out.trim().is_empty())
    }

    fn worktree_prune(&self, repo: &Path) -> Result<(), Error> {
        self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "worktree".as_ref(),
            "prune".as_ref(),
        ])
        .map(drop)
    }

    fn branches(&self, repo: &Path, prefix: &str) -> Result<Vec<String>, Error> {
        let out = self.git_out(&[
            "-C".as_ref(),
            repo.as_os_str(),
            "for-each-ref".as_ref(),
            "--format=%(refname:short)".as_ref(),
            format!("refs/heads/{prefix}").as_ref(),
        ])?;
        Ok(out.lines().map(str::to_string).collect())
    }

    fn start(&self, mode: Mode, cmd: &StartCmd) -> Result<(), Error> {
        self.check(mode)?;
        let mut c = Command::new(&cmd.program);
        c.args(&cmd.args)
            .env_clear()
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .current_dir(&cmd.cwd);
        run_captured(c, CLAUDE_START_TIMEOUT, "claude").map(drop)
    }

    fn agents(&self) -> Result<Vec<Agent>, Error> {
        agents::list(self.env)
    }

    fn stop_bg(&self, session_id: &str) -> Result<(), Error> {
        let claude = launch::claude_program(self.env)?;
        run_captured(
            stop_command(&claude, session_id),
            GIT_TIMEOUT,
            "claude stop",
        )
        .map(drop)
    }

    fn terminate(&self, pid: u32) -> Result<(), Error> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let raw = i32::try_from(pid)
            .ok()
            .filter(|p| *p > 0)
            .ok_or_else(|| Error::Refused(format!("not a pid: {pid}")))?;
        kill(Pid::from_raw(raw), Signal::SIGTERM)
            .map_err(|e| Error::Io(std::io::Error::other(format!("kill {pid}: {e}"))))
    }

    /// herdr panes arrive with Task 20; a `--bg` or unpaned child has none.
    fn close_pane(&self, _session_id: &str) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::super::*;

    fn path_env() -> impl Fn(&str) -> Option<String> {
        |k: &str| (k == "PATH").then(|| std::env::var("PATH").unwrap_or_default())
    }

    fn git(dir: &Path, args: &[&str]) {
        let s = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.test"])
            .args(args)
            .output()
            .unwrap();
        assert!(s.status.success(), "git {args:?}: {s:?}");
    }

    #[test]
    fn git_worktrees_add_list_dirty_remove() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        let env = path_env();
        let l = RealLauncher::new(&env);
        let wt = repo.join(".worktrees/demo/c1");
        l.worktree_add(&repo, &wt, "crook/demo/c1").unwrap();
        assert!(wt.join(".git").exists());
        assert_eq!(l.worktree_list(&repo).unwrap(), [repo.clone(), wt.clone()]);
        assert!(!l.worktree_dirty(&wt).unwrap());
        std::fs::write(wt.join("new.txt"), "x").unwrap();
        assert!(l.worktree_dirty(&wt).unwrap());
        // A branch that already exists is git's refusal, with its message.
        let e = l
            .worktree_add(&repo, &repo.join(".worktrees/demo/c2"), "crook/demo/c1")
            .unwrap_err()
            .to_string();
        assert!(e.contains("git") && e.contains("already"), "{e}");
        std::fs::remove_file(wt.join("new.txt")).unwrap();
        l.worktree_remove(&repo, &wt).unwrap();
        assert!(!wt.exists());
        assert_eq!(l.worktree_list(&repo).unwrap(), [repo]);
    }

    #[test]
    fn git_prune_and_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(tmp.path()).unwrap().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
        for b in [
            "crook/demo/c1",
            "crook/demo/c2",
            "crook/demo2/c1",
            "crook-x",
        ] {
            git(&repo, &["branch", b]);
        }
        let env = path_env();
        let l = RealLauncher::new(&env);
        assert_eq!(
            l.branches(&repo, "crook/demo/").unwrap(),
            ["crook/demo/c1", "crook/demo/c2"]
        );
        assert!(l.branches(&repo, "crook/none/").unwrap().is_empty());
        // A worktree whose directory was deleted by hand is dropped by prune.
        let wt = repo.join(".worktrees/demo/c3");
        l.worktree_add(&repo, &wt, "crook/demo/c3").unwrap();
        std::fs::remove_dir_all(&wt).unwrap();
        assert_eq!(l.worktree_list(&repo).unwrap().len(), 2);
        l.worktree_prune(&repo).unwrap();
        assert_eq!(l.worktree_list(&repo).unwrap(), std::slice::from_ref(&repo));
        assert!(l.worktree_prune(&repo.join("missing")).is_err());
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[test]
    fn start_runs_claude_on_exactly_the_given_environment_and_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out");
        let claude = script(
            tmp.path(),
            "claude",
            &format!(
                "echo \"$PWD|$CROOK_CHILD|${{LEAK-unset}}|$*\" > {}",
                out.display()
            ),
        );
        let env = path_env();
        let cmd = StartCmd {
            program: claude,
            args: vec!["--bg".into(), "--".into(), "prompt words".into()],
            env: vec![("CROOK_CHILD".into(), "c1".into())],
            cwd: std::fs::canonicalize(tmp.path()).unwrap(),
        };
        RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap();
        let got = std::fs::read_to_string(out).unwrap();
        let want = format!("{}|c1|unset|--bg -- prompt words\n", cmd.cwd.display());
        assert_eq!(got, want);
    }

    #[test]
    fn start_does_not_wait_for_a_background_session_holding_the_pipes() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = script(tmp.path(), "claude", "sleep 8 &\necho started");
        let env = path_env();
        let cmd = StartCmd {
            program: claude,
            args: Vec::new(),
            env: vec![("PATH".into(), "/bin:/usr/bin".into())],
            cwd: tmp.path().into(),
        };
        let t = Instant::now();
        RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap();
        assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    }

    #[test]
    fn start_failure_carries_stderr() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = script(
            tmp.path(),
            "claude",
            "echo 'Workspace not trusted' >&2\nexit 1",
        );
        let env = path_env();
        let cmd = StartCmd {
            program: claude,
            args: Vec::new(),
            env: Vec::new(),
            cwd: tmp.path().into(),
        };
        let e = RealLauncher::new(&env).start(Mode::Bg, &cmd).unwrap_err();
        assert!(e.to_string().contains("Workspace not trusted"), "{e}");
    }

    #[test]
    fn stop_command_clears_claudecode_and_crook_variables() {
        let c = stop_command(Path::new("/x/claude"), "sid");
        let removed: Vec<_> = c
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert!(removed.contains(&"CLAUDECODE".to_string()), "{removed:?}");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["stop", "sid"]);
    }

    #[test]
    fn worktree_add_gets_longer_than_the_other_commands_and_fits_the_spawn_budget() {
        use crate::launch::child::{CLAUDE_START_TIMEOUT, GIT_TIMEOUT, WORKTREE_ADD_TIMEOUT};
        assert!(WORKTREE_ADD_TIMEOUT > GIT_TIMEOUT);
        assert!(WORKTREE_ADD_TIMEOUT + CLAUDE_START_TIMEOUT < Duration::from_secs(120));
    }

    #[test]
    fn herdr_is_not_yet_startable() {
        let env = path_env();
        let l = RealLauncher::new(&env);
        assert!(l.check(Mode::Bg).is_ok());
        assert!(
            l.check(Mode::Herdr)
                .unwrap_err()
                .to_string()
                .contains("Task 20")
        );
    }
}
