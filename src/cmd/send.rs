//! `crook send <child> <text>`: types one line into a live child of the flock, never with `--force`, and
//! exits with hc's own code when it does not land (ADR 0028).

use super::{Io, live_child_terminal};
use crate::hc::{Hc, SendOutcome};
use crate::{Ctx, Error};

/// hc's exit codes for a send that did not land (spike 2).
pub const BLOCKED: u8 = 9;
pub const FINISHED: u8 = 11;

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    child: &str,
    text: &str,
    flag: Option<&str>,
) -> Result<(), Error> {
    if text.trim().is_empty() {
        return Err(Error::Flock("crook send: the text is empty".to_string()));
    }
    let (_, terminal) = live_child_terminal(ctx, hc, child, flag)?;
    let name = terminal.name.as_deref().unwrap_or(&terminal.address);
    match hc.send(&terminal.address, text)? {
        SendOutcome::Sent => {
            io.say(format!("sent to {name}"));
            Ok(())
        }
        SendOutcome::Blocked => Err(Error::Send {
            code: BLOCKED,
            message: format!(
                "{name} is blocked on a question or dialog, so nothing was typed; the maintainer answers it in its pane"
            ),
        }),
        SendOutcome::Finished | SendOutcome::NotFound => Err(Error::Send {
            code: FINISHED,
            message: format!("{name}'s terminal has ended; `crook resume {child}` starts it again"),
        }),
        SendOutcome::Failed(why) => Err(Error::Flock(format!("crook send to {name}: {why}"))),
    }
}

#[cfg(test)]
mod tests;
