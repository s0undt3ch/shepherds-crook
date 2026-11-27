//! herdr-app renames after a coordinator starts (ADR 0021, spike 25). crook never creates a space
//! (ADR 0015, Q24): the space comes from the coordinator's `herdr.space` label, and these calls only
//! name it. A missing or closed app is a warning, never a failure.

use std::time::{Duration, Instant};

use crate::Error;
use crate::names;
use crate::run::{Cmd, Runner};

#[cfg(test)]
mod tests;

const PROGRAM: &str = "herdr-app";
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct Options {
    pub interval: Duration,
    /// The whole retry, both renames together.
    pub budget: Duration,
}

impl Default for Options {
    /// Half a second apart for about three seconds in all: the time for the space to appear after
    /// `hc run` returns.
    fn default() -> Self {
        Options {
            interval: Duration::from_millis(500),
            budget: Duration::from_secs(3),
        }
    }
}

/// Renames the flock's space to its slug and the coordinator's tab to `coordinator`. Returns the
/// text of a warning when it could not.
pub fn rename_coordinator(runner: &dyn Runner, slug: &str) -> Option<String> {
    rename_coordinator_with(runner, slug, &Options::default())
}

pub fn rename_coordinator_with(
    runner: &dyn Runner,
    slug: &str,
    options: &Options,
) -> Option<String> {
    let space = serde_json::json!({ "id": names::herdr_space(slug), "name": slug });
    let tab = serde_json::json!({ "id": names::herdr_tab(slug), "name": "coordinator" });
    let deadline = Instant::now() + options.budget;
    // A space that cannot be renamed means the app is not there, so the tab would fail the same way.
    for (command, arg) in [("space.rename", space), ("tab.rename", tab)] {
        if let Err(why) = do_with_retry(runner, command, &arg.to_string(), options, deadline) {
            return Some(format!(
                "could not run {PROGRAM} {command}: {why}; the herdr-app space and tab keep their default names until the next `crook open`"
            ));
        }
    }
    None
}

fn do_with_retry(
    runner: &dyn Runner,
    command: &str,
    json: &str,
    options: &Options,
    deadline: Instant,
) -> Result<(), String> {
    let mut last = String::new();
    loop {
        if !last.is_empty() {
            if Instant::now() + options.interval > deadline {
                return Err(last);
            }
            std::thread::sleep(options.interval);
        }
        let cmd = Cmd::new(PROGRAM).args(["do", command, json]);
        match runner.run(cmd, CALL_TIMEOUT) {
            // Not installed: waiting would not change that.
            Err(Error::Spawn { .. }) => return Err(format!("{PROGRAM} is not on PATH")),
            // A hung app will not answer a second try any sooner.
            Err(e @ Error::Timeout { .. }) => return Err(e.to_string()),
            Err(e) => last = e.to_string(),
            Ok(out) => {
                let refused = serde_json::from_str::<serde_json::Value>(out.stdout.trim())
                    .ok()
                    .and_then(|v| v.get("ok").and_then(serde_json::Value::as_bool))
                    == Some(false);
                if out.is_success() && !refused {
                    return Ok(());
                }
                let text = if out.stderr.trim().is_empty() {
                    out.stdout.trim()
                } else {
                    out.stderr.trim()
                };
                last = format!("exit {:?}: {text}", out.code);
            }
        }
    }
}
