//! `crook new <slug> [--task <text>]`: create the flock, then open it (ADR 0004, 0012, 0020; Q1, Q2).

use std::time::UNIX_EPOCH;

use super::{Io, open};
use crate::files::roster::{self, Roster};
use crate::hc::Hc;
use crate::run::Cmd;
use crate::{Ctx, Error, config, flock, names};

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    slug: &str,
    task: Option<&str>,
) -> Result<(), Error> {
    names::validate_slug(slug)?;
    let config = config::load(ctx.env)?;
    let made = flock::create(ctx.env, &config, &today(ctx), slug)?;
    let first = Roster::new(slug, &(ctx.new_id)(), &made.folder);
    if let Err(e) = roster::create(&made.folder, &first) {
        // A folder without a roster would burn the slug.
        let _ = std::fs::remove_dir_all(&made.folder);
        return Err(e);
    }
    open::run(ctx, hc, io, slug, task).inspect_err(|_| {
        io.warn(format!(
            "flock {slug} was created in {}, but its coordinator did not start; once the problem is fixed, run `crook open {slug}`",
            made.folder.display()
        ));
    })
}

const DATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The local date, from `date +%F`, so a flock made late in the evening is dated that evening. Where
/// `date` fails, today's date in UTC from the clock (no date crate, per the plan).
pub fn today(ctx: &Ctx) -> String {
    let local = ctx
        .runner
        .run(Cmd::new("date").args(["+%F"]), DATE_TIMEOUT)
        .ok()
        .filter(crate::run::Output::is_success)
        .map(|o| o.stdout.trim().to_string())
        .filter(|d| flock::is_date(d));
    local.unwrap_or_else(|| {
        let secs = ctx
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        utc_date(secs)
    })
}

/// `yyyy-mm-dd` of a Unix time, by the civil-from-days algorithm.
pub fn utc_date(unix_secs: u64) -> String {
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests;
