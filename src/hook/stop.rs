//! `crook hook stop`: in a child, report a changed result to the coordinator; in every session,
//! the self-reload (ADR 0008, 0009, 0018, 0019, 0020). Never prints a `block` decision.

use super::{Input, Reply, Who, version};
use crate::cmd::Io;
use crate::files::{marks, result};
use crate::flock;
use crate::hc::{Hc, SendOutcome};
use crate::{Ctx, Error, names};

#[cfg(test)]
mod tests;

/// The app a Claude Code session reports in its hc status (ADR 0018).
/// What the coordinator is sent, followed by the result's absolute path (D69).
pub const RESULT_PREFIX: &str = "Result updated: ";

const CLAUDE_APP: &str = "claude-code";

pub fn run(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, input: &Input, who: &Who) -> Reply {
    if let Some(child) = &who.child
        && let Err(e) = deliver(ctx, hc, io, who, child)
    {
        // A failure here must not skip the reload below.
        io.warn(format!("crook hook: could not report the result: {e}"));
    }
    match &input.session_id {
        Some(session) => version::self_reload(ctx, hc, io, session, who),
        None => io.warn("crook hook: no session_id in the input, so no version check"),
    }
    Reply::default()
}

/// 0009: nothing unless the result is new or changed; send only to a live Claude Code parent; mark
/// only after a successful send. Any other outcome leaves the mark old for the rescan (0008).
fn deliver(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, who: &Who, child: &str) -> Result<(), Error> {
    let flock = flock::find(ctx.env, &who.flock)?;
    let path = flock.result_path(child);
    let Some(hash) = result::read(&path)?.sha256 else {
        return Ok(());
    };
    // A corrupt marks file counts as empty, and `mark` below replaces it.
    let (marks, broken) = marks::load_or_empty(&flock.folder);
    if let Some(e) = broken {
        io.warn(format!("crook hook: {e}; treating it as empty"));
    }
    if marks.get(child) == Some(&hash) {
        return Ok(());
    }
    let Some(parent) = &who.parent else {
        io.warn(format!(
            "crook hook: {} has no parent terminal",
            who.address
        ));
        return Ok(());
    };
    // The coordinator of this flock, with Claude Code running in it: a bare shell would run the text.
    let reachable = hc.get(parent)?.is_some_and(|t| {
        t.is_live()
            && t.labels.get(names::LABEL_FLOCK) == Some(&who.flock)
            && !t.labels.contains_key(names::LABEL_CHILD)
            && t.status.and_then(|s| s.app).as_deref() == Some(CLAUDE_APP)
    });
    if !reachable {
        return Ok(());
    }
    // `Result updated:` first, as spike 14 had it: a text that starts with `/` could be read as a
    // slash command or never delivered mid-turn (D69).
    match hc.send(parent, &format!("{RESULT_PREFIX}{}", path.display()))? {
        SendOutcome::Sent => marks::mark(&flock.folder, child, &hash),
        other => {
            io.warn(format!(
                "crook hook: the result was not delivered ({other:?}); the next prompt rescan will report it"
            ));
            Ok(())
        }
    }
}
