//! Everything a command reads from outside the process: the environment, subprocesses and the
//! clock. `main.rs` builds the real one and tests build fakes, so no other module touches the
//! process environment (ADR 0020).

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use crate::Env;
use crate::run::Runner;

pub trait Clock {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub struct Ctx<'a> {
    pub env: Env<'a>,
    /// The whole process environment, for the one call that passes it on (`hc server start`,
    /// ADR 0020). Everything else asks `env` for a name.
    pub vars: &'a dyn Fn() -> Vec<(OsString, OsString)>,
    pub runner: &'a dyn Runner,
    pub clock: &'a dyn Clock,
    /// The running executable, which the install paths are derived from (ADR 0019). A lookup
    /// that can fail, so it runs only for the commands that need it.
    pub current_exe: &'a dyn Fn() -> std::io::Result<PathBuf>,
    /// A fresh session id (a v4 UUID), so tests can pin them.
    pub new_id: &'a dyn Fn() -> String,
}
