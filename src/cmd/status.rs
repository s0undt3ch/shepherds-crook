//! `crook status [<slug>] [--json]`: the roster, `hc ls` and the result files and marks, merged
//! (ADR 0014, 0020, 0024; Q5, D54).
//!
//! hc being unreachable is not a failure: the file side is still shown, every `terminal` is `null`
//! and the JSON carries the reason as `hc_error`, because the mod parses stdout only on exit 0.

use std::path::Path;

use serde_json::{Value, json};

use super::{Io, find_flock, is_closed, live_terminals};
use crate::config::{self, Config};
use crate::files::marks::{self, Marks};
use crate::files::result::{self, Status};
use crate::files::roster::{self, Child, Roster, State};
use crate::flock::{self, Flock};
use crate::hc::preflight;
use crate::hc::{Hc, Terminal};
use crate::{Ctx, Error, install, names};

/// How long `status` may talk to hc in all, so a hung hc cannot hold up the mod's list.
pub const HC_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

/// The `status --json` format number (`schema/status.schema.json`).
pub const FORMAT_VERSION: u32 = 1;

pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    slug: Option<&str>,
    json: bool,
) -> Result<(), Error> {
    let config = config::load(ctx.env)?;
    let crook_version = install::find_own(ctx)?.version()?;
    let flocks = match slug {
        Some(slug) => {
            let found = find_flock(ctx, slug)?;
            vec![(found.clone(), roster::load(&found.folder)?)]
        }
        None => every_flock(ctx, io)?,
    };
    // Preflight starts a scrubbed core when none runs, as every command that touches hc does
    // (ADR 0020); a failure is reported, not raised.
    let mut hc_error = preflight::run(hc, &(ctx.vars)())
        .err()
        .map(|e| e.to_string());
    let views: Vec<FlockView> = flocks
        .iter()
        .map(|(flock, roster)| view(hc, &mut hc_error, &config, flock, roster))
        .collect();
    if json {
        let mut doc = json!({
            "version": FORMAT_VERSION,
            "crook_version": crook_version,
            "flocks": views.iter().map(FlockView::to_json).collect::<Vec<_>>(),
        });
        if let Some(error) = &hc_error {
            doc["hc_error"] = json!(error);
        }
        io.say(doc);
    } else {
        print_table(io, &views, hc_error.as_deref());
    }
    Ok(())
}

/// Every flock whose roster loads. A broken one is warned about and left out, so one bad folder does
/// not hide the rest.
fn every_flock(ctx: &Ctx, io: &mut Io) -> Result<Vec<(Flock, Roster)>, Error> {
    let mut out = Vec::new();
    for flock in flock::list(ctx.env)? {
        match roster::load(&flock.folder) {
            Ok(r) if r.flock == flock.slug => out.push((flock, r)),
            Ok(r) => io.warn(format!(
                "{} is a broken flock: its roster.json says flock {:?}, not {:?}",
                flock.folder.display(),
                r.flock,
                flock.slug
            )),
            Err(e) => io.warn(format!("{} is a broken flock: {e}", flock.folder.display())),
        }
    }
    Ok(out)
}

struct TerminalView {
    id: String,
    state: String,
    app: Option<String>,
}

impl TerminalView {
    fn to_json(&self) -> Value {
        json!({ "id": self.id, "state": self.state, "app": self.app })
    }

    fn text(&self) -> String {
        format!("{} {}", self.id, self.state)
    }
}

struct ResultView {
    path: String,
    status: &'static str,
    delivered: bool,
}

struct ChildView<'a> {
    child: &'a Child,
    brief: String,
    terminal: Option<TerminalView>,
    result: ResultView,
}

struct FlockView<'a> {
    flock: &'a Flock,
    roster: &'a Roster,
    notes_folder: String,
    coordinator: Option<TerminalView>,
    children: Vec<ChildView<'a>>,
}

fn state_word(state: State) -> &'static str {
    match state {
        State::Active => "active",
        State::Done => "done",
    }
}

/// An agent in the terminal makes it live (D52): a bare shell is a restored terminal, not the session.
/// `hc` is only down once preflight or an `ls` has failed (D72); a failed `get` is not that.
fn live_view(hc: &dyn Hc, t: &Terminal) -> Option<TerminalView> {
    if !t.is_live() || t.is_shell() {
        return None;
    }
    let state = t
        .status
        .as_ref()
        .map_or_else(|| "live".to_string(), |s| s.state.clone());
    // `hc ls` has no app; `get` does. A failed `get` loses that terminal's app and nothing else: the
    // terminal is still listed, with `app: null` (the mod reads neither, and the coordinator skill
    // only needs `id` and `state`).
    let app = hc
        .get(&t.address)
        .ok()
        .flatten()
        .and_then(|full| full.status)
        .and_then(|s| s.app);
    Some(TerminalView {
        id: t.address.clone(),
        state,
        app,
    })
}

fn result_view(flock: &Flock, child: &str, marks: &Marks) -> ResultView {
    let path = flock.result_path(child);
    let (status, delivered) = match result::read(&path) {
        Ok(info) => {
            let delivered = info.sha256.is_some()
                && info.sha256.as_deref() == marks.get(child).map(String::as_str);
            let word = match info.status {
                Status::Working => "working",
                Status::Done => "done",
                Status::Failed => "failed",
                Status::Unreadable => "unreadable",
                Status::Missing => "missing",
            };
            (word, delivered)
        }
        Err(_) => ("unreadable", false),
    };
    ResultView {
        path: path.display().to_string(),
        status,
        delivered,
    }
}

fn view<'a>(
    hc: &dyn Hc,
    hc_error: &mut Option<String>,
    config: &Config,
    flock: &'a Flock,
    roster: &'a Roster,
) -> FlockView<'a> {
    // No hc, no terminals: once preflight or an `ls` has failed the rest are not tried.
    let live: Vec<Terminal> = if hc_error.is_some() {
        Vec::new()
    } else {
        live_terminals(hc, &flock.slug).unwrap_or_else(|e| {
            *hc_error = Some(e.to_string());
            Vec::new()
        })
    };
    let named = |name: &str| live.iter().find(|t| t.name.as_deref() == Some(name));
    let coordinator = named(&names::hc_coordinator(&flock.slug)).and_then(|t| live_view(hc, t));
    let marks = marks::load(&flock.folder).unwrap_or_default();
    let children = roster
        .children
        .iter()
        .map(|child| ChildView {
            child,
            brief: flock.brief_path(&child.brief).display().to_string(),
            terminal: named(&names::hc_child(&flock.slug, &child.name))
                .and_then(|t| live_view(hc, t)),
            result: result_view(flock, &child.name, &marks),
        })
        .collect();
    FlockView {
        flock,
        roster,
        notes_folder: flock.notes_folder(config).display().to_string(),
        coordinator,
        children,
    }
}

fn path_text(p: &Path) -> String {
    p.display().to_string()
}

impl FlockView<'_> {
    fn to_json(&self) -> Value {
        let slug = &self.flock.slug;
        json!({
            "slug": slug,
            "space": names::herdr_space(slug),
            "folder": path_text(&self.flock.folder),
            "notes_folder": self.notes_folder,
            "state": state_word(if is_closed(self.roster) { State::Done } else { State::Active }),
            "coordinator": {
                "session_id": self.roster.coordinator.session_id,
                "terminal": self.coordinator.as_ref().map(TerminalView::to_json),
            },
            "children": self.children.iter().map(|c| json!({
                "name": c.child.name,
                "role": c.child.role,
                "model": c.child.model,
                "state": state_word(c.child.state),
                "folder": path_text(&c.child.folder),
                "repo": c.child.repo.as_deref().map(path_text),
                "branch": c.child.branch,
                "brief": c.brief,
                "terminal": c.terminal.as_ref().map(TerminalView::to_json),
                "result": {
                    "path": c.result.path,
                    "status": c.result.status,
                    "delivered": c.result.delivered,
                },
            })).collect::<Vec<_>>(),
        })
    }
}

fn print_table(io: &mut Io, views: &[FlockView], hc_error: Option<&str>) {
    if let Some(error) = hc_error {
        io.say(format!("hc is not reachable: {error}"));
    }
    if views.is_empty() {
        io.say("no flocks");
        return;
    }
    for v in views {
        let state = if is_closed(v.roster) {
            "closed"
        } else {
            "active"
        };
        let coordinator = v
            .coordinator
            .as_ref()
            .map_or_else(|| "not running".to_string(), TerminalView::text);
        io.say(format!(
            "flock {} ({state}), coordinator: {coordinator}",
            v.flock.slug
        ));
        if v.children.is_empty() {
            io.say("  no children");
            continue;
        }
        let rows: Vec<[String; 6]> = v
            .children
            .iter()
            .map(|c| {
                [
                    c.child.name.clone(),
                    c.child.role.clone(),
                    c.child.model.clone(),
                    state_word(c.child.state).to_string(),
                    c.terminal
                        .as_ref()
                        .map_or_else(|| "-".to_string(), TerminalView::text),
                    format!(
                        "{}{}",
                        c.result.status,
                        if c.result.delivered {
                            " (delivered)"
                        } else {
                            ""
                        }
                    ),
                ]
            })
            .collect();
        let head = ["CHILD", "ROLE", "MODEL", "STATE", "TERMINAL", "RESULT"].map(String::from);
        let mut widths = [0usize; 6];
        for row in std::iter::once(&head).chain(rows.iter()) {
            for (w, cell) in widths.iter_mut().zip(row) {
                *w = (*w).max(cell.chars().count());
            }
        }
        for row in std::iter::once(&head).chain(rows.iter()) {
            let line: Vec<String> = row
                .iter()
                .zip(widths)
                .map(|(cell, w)| format!("{cell:<w$}"))
                .collect();
            io.say(format!("  {}", line.join("  ").trim_end()));
        }
    }
}

#[cfg(test)]
mod tests;
