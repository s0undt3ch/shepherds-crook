//! The variables removed from the call that starts the hc core (ADR 0020, spike 23). The call that
//! starts the core fixes the environment of every terminal it runs, so a core started inside a
//! Claude session would hand that session's markers to every child.

use std::ffi::{OsStr, OsString};

#[cfg(test)]
mod tests;

/// Exact names.
const REMOVED: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "AI_AGENT",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
    "HC_SCOPE",
];

const REMOVED_PREFIXES: &[&str] = &[
    "CLAUDE_CODE_SESSION_",
    "CLAUDE_CODE_CHILD_",
    "CLAUDE_CODE_MESSAGING_",
];

/// A name that is not text cannot be one of these.
fn removed(name: &OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| REMOVED.contains(&n) || REMOVED_PREFIXES.iter().any(|p| n.starts_with(p)))
}

/// `vars` without the session and terminal identity variables; everything else, including
/// `CLAUDE_CONFIG_DIR`, `HC_SESSION` and the XDG ones, stays.
pub fn scrub(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.into_iter().filter(|(k, _)| !removed(k)).collect()
}
