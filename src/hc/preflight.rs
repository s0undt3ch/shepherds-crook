//! The checks before a command uses hc (ADR 0016, amended by 0020): the version floor, the schema,
//! and starting the core with a scrubbed environment.
//!
//! Every hc command except `--version` starts a core when none runs, with the caller's whole
//! environment (checked against hc 0.1.22: `hc schema` and `hc server status` both did). So the
//! first call that talks to the core must be `server start` with the scrubbed environment, and it
//! is made every time: against a running core it only reports it.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::time::{Duration, Instant};

use super::Hc;
use super::scrub::scrub;
use crate::Error;
use crate::install::compare_versions;

#[cfg(test)]
mod tests;

/// The oldest hc crook works with: `terminal.run` takes `labels` from here (ADR 0016, spike 9).
pub const MIN_HC_VERSION: &str = "0.1.22";

#[derive(Clone, Debug)]
pub struct Options {
    /// How long a started core has to answer.
    pub start_deadline: Duration,
    pub poll_interval: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            start_deadline: Duration::from_secs(10),
            poll_interval: Duration::from_millis(100),
        }
    }
}

/// Preflight with the default timings. `vars` is crook's whole process environment.
pub fn run(hc: &dyn Hc, vars: &[(OsString, OsString)]) -> Result<(), Error> {
    run_with(hc, vars, &Options::default())
}

pub fn run_with(
    hc: &dyn Hc,
    vars: &[(OsString, OsString)],
    options: &Options,
) -> Result<(), Error> {
    check_version(hc)?;
    let env = scrub(vars.iter().cloned());
    hc.server_start(&env)
        .map_err(|e| Error::Preflight(format!("could not start the hc core: {e}")))?;
    let schema = wait_for_core(hc, &env, options)?;
    if !schema.has_param("terminal.run", "labels") {
        return Err(Error::Preflight(format!(
            "hc's schema has no `labels` parameter on `terminal.run`, which crook needs (hc {MIN_HC_VERSION} or newer)"
        )));
    }
    Ok(())
}

fn check_version(hc: &dyn Hc) -> Result<(), Error> {
    let found = hc.version()?;
    match compare_versions(&found, MIN_HC_VERSION) {
        Some(Ordering::Less) => Err(Error::Preflight(format!(
            "hc {found} is too old: crook needs hc {MIN_HC_VERSION} or newer"
        ))),
        Some(_) => Ok(()),
        None => Err(Error::Preflight(format!(
            "could not read hc's version ({found:?}); crook needs hc {MIN_HC_VERSION} or newer"
        ))),
    }
}

/// `server start` returns once the core is up in practice; polling `schema` covers a slow one. A
/// `schema` call that finds no core starts one with crook's own, unscrubbed environment, so a core
/// that stopped meanwhile is started again by `server start` before each retry, never by `schema`.
fn wait_for_core(
    hc: &dyn Hc,
    env: &[(OsString, OsString)],
    options: &Options,
) -> Result<super::Schema, Error> {
    let deadline = Instant::now() + options.start_deadline;
    loop {
        match hc.schema() {
            Ok(schema) => return Ok(schema),
            Err(e) if Instant::now() >= deadline => {
                return Err(Error::Preflight(format!(
                    "the hc core did not come up within {:?}: {e}",
                    options.start_deadline
                )));
            }
            Err(_) => {
                std::thread::sleep(options.poll_interval);
                // Its own failure is not final: the `schema` call after it tells.
                let _ = hc.server_start(env);
            }
        }
    }
}
