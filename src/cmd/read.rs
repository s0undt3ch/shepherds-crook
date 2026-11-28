//! `crook read <child>`: prints a live child's screen, and only a child of this flock (ADR 0028).

use super::{Io, live_child_terminal};
use crate::hc::Hc;
use crate::{Ctx, Error};

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    child: &str,
    lines: u32,
    flag: Option<&str>,
) -> Result<(), Error> {
    let (_, terminal) = live_child_terminal(ctx, hc, child, flag)?;
    let screen = hc.read(&terminal.address, lines)?;
    io.say(screen.trim_end_matches('\n'));
    Ok(())
}

#[cfg(test)]
mod tests;
