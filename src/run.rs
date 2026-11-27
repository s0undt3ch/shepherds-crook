//! Subprocesses with captured output and a deadline. Every call to `hc`, `git` or `claude` goes
//! through a [`Runner`], so hooks cannot hang (ADR 0020) and tests can script the answers.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

use crate::Error;

#[cfg(test)]
pub mod fake;
#[cfg(test)]
mod tests;

/// What to run. `env: Some(..)` is the child's whole environment; `None` inherits crook's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Option<Vec<(String, String)>>,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Cmd {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: None,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: &Path) -> Self {
        self.cwd = Some(cwd.to_path_buf());
        self
    }

    pub fn env(mut self, env: impl IntoIterator<Item = (String, String)>) -> Self {
        self.env = Some(env.into_iter().collect());
        self
    }
}

/// A finished process. A non-zero exit is still an `Output`: callers such as `hc send` read
/// meaning into specific exit codes. `code` is `None` when a signal ended the process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(stdout: impl Into<String>) -> Self {
        Output {
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        }
    }

    pub fn is_success(&self) -> bool {
        self.code == Some(0)
    }
}

pub trait Runner {
    fn run(&self, cmd: Cmd, timeout: Duration) -> Result<Output, Error>;
}

pub struct RealRunner;

/// How long to keep reading after the process exits. A descendant that inherited the pipes (a
/// background `claude`, say) can hold them open, or keep writing, for ever; this bounds the
/// wait, and never past the caller's own deadline.
const PIPE_DRAIN: Duration = Duration::from_millis(300);

/// The most of each stream that is kept. The rest is read and dropped so the child never blocks.
const MAX_CAPTURE: usize = 16 * 1024 * 1024;

enum Chunk {
    Out(Vec<u8>),
    Err(Vec<u8>),
    Eof,
}

#[derive(Default)]
struct Capture {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    open: u8,
}

impl Capture {
    fn take(&mut self, chunk: Chunk) {
        let (buf, bytes) = match chunk {
            Chunk::Out(b) => (&mut self.stdout, b),
            Chunk::Err(b) => (&mut self.stderr, b),
            Chunk::Eof => {
                self.open = self.open.saturating_sub(1);
                return;
            }
        };
        let room = MAX_CAPTURE.saturating_sub(buf.len());
        buf.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }
}

impl Runner for RealRunner {
    fn run(&self, cmd: Cmd, timeout: Duration) -> Result<Output, Error> {
        let program = cmd.program.clone();
        let io = |source| Error::Io {
            program: program.clone(),
            source,
        };
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args);
        if let Some(cwd) = &cmd.cwd {
            std::fs::metadata(cwd)
                .and_then(|m| {
                    m.is_dir()
                        .then_some(())
                        .ok_or_else(|| std::io::Error::other("not a folder"))
                })
                .map_err(|source| Error::WorkDir {
                    path: cwd.clone(),
                    source,
                })?;
            command.current_dir(cwd);
        }
        if let Some(env) = &cmd.env {
            command.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
        }
        // Its own process group, so a timeout can take the whole tree down with it.
        let mut child = command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Spawn {
                program: program.clone(),
                source,
            })?;

        let (tx, rx) = mpsc::channel();
        let mut cap = Capture::default();
        if let Some(pipe) = child.stdout.take() {
            pump(pipe, tx.clone(), Chunk::Out);
            cap.open += 1;
        }
        if let Some(pipe) = child.stderr.take() {
            pump(pipe, tx.clone(), Chunk::Err);
            cap.open += 1;
        }
        drop(tx);

        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = child.try_wait().map_err(io)? {
                break status;
            }
            if Instant::now() >= deadline {
                if let Ok(pid) = i32::try_from(child.id()) {
                    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Timeout {
                    program,
                    after: timeout,
                });
            }
            // Read while waiting, so a chatty child does not pile output up in the channel.
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(chunk) => cap.take(chunk),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };

        let end = deadline.min(Instant::now() + PIPE_DRAIN);
        while cap.open > 0 {
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                break;
            };
            match rx.recv_timeout(left) {
                Ok(chunk) => cap.take(chunk),
                Err(_) => break,
            }
        }
        Ok(Output {
            code: status.code(),
            stdout: String::from_utf8_lossy(&cap.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&cap.stderr).into_owned(),
        })
    }
}

/// Reads `pipe` to the end on its own thread, so a full pipe never blocks the child.
fn pump<R: Read + Send + 'static>(
    mut pipe: R,
    tx: mpsc::Sender<Chunk>,
    wrap: fn(Vec<u8>) -> Chunk,
) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 || tx.send(wrap(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        let _ = tx.send(Chunk::Eof);
    });
}
