//! One file per subcommand, and what they share: where output goes, finding the flock and the
//! calling terminal, the launch inputs and worktree retirement (ADR 0020).

use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::cli::Command;
use crate::config::{self, Config};
use crate::files::roster::{Child, Roster, State};
use crate::flock::{self, Flock};
use crate::git::{self, Removal, Worktree};
use crate::hc::client::HcClient;
use crate::hc::{Hc, Terminal};
use crate::{Ctx, Error, install, launch, names, paths};

pub mod close;
pub mod completion;
pub mod doctor;
pub mod new;
pub mod open;
pub mod read;
pub mod resume;
pub mod send;
pub mod setup;
pub mod spawn;
pub mod status;
pub mod stop;

/// Where a command's output goes. A result is one line on stdout; a problem that does not stop the
/// command is a `warning:` line on stderr, so a caller that parses stdout never sees it.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

impl Io<'_> {
    pub fn say(&mut self, line: impl Display) {
        let _ = writeln!(self.out, "{line}");
        let _ = self.out.flush();
    }

    pub fn warn(&mut self, line: impl Display) {
        let _ = writeln!(self.err, "warning: {line}");
        let _ = self.err.flush();
    }
}

/// Runs a parsed command against the real hc, stdout and stderr.
pub fn dispatch(command: &Command, ctx: &Ctx) -> Result<(), Error> {
    // `status` talks to hc for a bounded total time; every other command has its own per-call limits.
    let budgeted = crate::run::Budgeted::new(std::time::Instant::now() + status::HC_BUDGET);
    let hc = match command {
        Command::Status { .. } => HcClient::new(&budgeted),
        _ => HcClient::new(ctx.runner),
    };
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let (mut out, mut err) = (stdout.lock(), stderr.lock());
    let mut io = Io {
        out: &mut out,
        err: &mut err,
    };
    dispatch_with(command, ctx, &hc, &mut io)
}

pub fn dispatch_with(command: &Command, ctx: &Ctx, hc: &dyn Hc, io: &mut Io) -> Result<(), Error> {
    match command {
        Command::New { slug, task } => new::run(ctx, hc, io, slug, task.as_deref()),
        Command::Open { slug } => open::run(ctx, hc, io, &flock::slug_for(ctx.env, slug)?, None),
        Command::Close { slug } => close::run(ctx, hc, io, &flock::slug_for(ctx.env, slug)?),
        Command::Spawn {
            role,
            child,
            brief,
            repo,
            model,
            flock,
        } => spawn::run(
            ctx,
            hc,
            io,
            &spawn::Args {
                role,
                child,
                brief,
                repo: repo.as_deref(),
                model: model.as_deref(),
                flock: flock.as_deref(),
            },
        ),
        Command::Stop { child, flock } => stop::run(ctx, hc, io, child, flock.as_deref()),
        Command::Send { child, text, flock } => {
            send::run(ctx, hc, io, child, text, flock.as_deref())
        }
        Command::Read {
            child,
            lines,
            flock,
        } => read::run(ctx, hc, io, child, *lines, flock.as_deref()),
        Command::Resume {
            child,
            model,
            flock,
        } => resume::run(
            ctx,
            hc,
            io,
            child.as_deref(),
            model.as_deref(),
            flock.as_deref(),
        ),
        Command::Status { slug, json, flock } => {
            let name = slug.as_deref().or(flock.as_deref());
            let slug = name.map(|n| flock::slug_for(ctx.env, n)).transpose()?;
            status::run(ctx, hc, io, slug.as_deref(), *json)
        }
        Command::Completion(what) => completion::run(ctx, io, what),
        Command::Setup => setup::run(ctx, io),
        Command::Doctor { fix, json } => doctor::run(ctx, hc, io, *fix, *json),
        other => Err(Error::NotImplemented(other.name())),
    }
}

/// The flock with this slug. An unknown slug is refused with the slugs that exist (Q7).
pub fn find_flock(ctx: &Ctx, slug: &str) -> Result<Flock, Error> {
    names::validate_slug(slug)?;
    let all = flock::list(ctx.env)?;
    if !all.iter().any(|f| f.slug == slug) {
        let known: Vec<&str> = all.iter().map(|f| f.slug.as_str()).collect();
        let hint = if known.is_empty() {
            "there are no flocks yet; `crook new <slug>` makes one".to_string()
        } else {
            format!("flocks: {}", known.join(", "))
        };
        return Err(Error::Flock(format!("no flock named {slug:?} ({hint})")));
    }
    flock::find(ctx.env, slug)
}

pub fn is_closed(roster: &Roster) -> bool {
    roster.coordinator.state == State::Done
}

/// What a launch needs besides the flock: the config, this install's plugin folder and version, and
/// the `CROOK_BIN` to pass on.
pub struct Setup {
    pub config: Config,
    pub version: String,
    pub plugin_dir: PathBuf,
    pub crook_bin: Option<String>,
}

impl Setup {
    pub fn load(ctx: &Ctx) -> Result<Setup, Error> {
        let config = config::load(ctx.env)?;
        let found = install::find_own(ctx)?;
        Ok(Setup {
            config,
            version: found.version()?,
            plugin_dir: plugin_dir(ctx.env, &found, install::DEBUG_BUILD)?,
            crook_bin: install::crook_bin(ctx.env, ctx.current_exe, install::DEBUG_BUILD)?,
        })
    }

    pub fn common<'a>(
        &'a self,
        flock_folder: &'a Path,
        notes_folder: &'a Path,
    ) -> launch::Common<'a> {
        launch::Common {
            plugin_dir: &self.plugin_dir,
            crook_bin: self.crook_bin.as_deref(),
            version: &self.version,
            flock_folder,
            notes_folder,
        }
    }
}

/// The literal `current/plugin` link path (ADR 0019, spike 18); a debug build uses its own
/// repository's `plugin/` (Q16).
pub fn plugin_dir(
    env: crate::Env,
    found: &install::Install,
    debug: bool,
) -> Result<PathBuf, Error> {
    if debug {
        Ok(found.plugin_dir())
    } else {
        Ok(paths::current_link(env)?.join("plugin"))
    }
}

/// The terminal a command runs in, from `$HC_TERMINAL_ID` and its labels (Q4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub address: String,
    pub flock: Option<String>,
    pub child: Option<String>,
}

fn terminal_id(ctx: &Ctx) -> Option<String> {
    (ctx.env)("HC_TERMINAL_ID").filter(|v| !v.is_empty())
}

/// A command that finds its flock from its terminal must have one, or be given `--flock`. Checked
/// before any hc call, so it fails fast.
pub fn require_context(ctx: &Ctx, flag: Option<&str>) -> Result<(), Error> {
    if flag.is_none() && terminal_id(ctx).is_none() {
        return Err(Error::Env(
            "this command finds its flock from the terminal it runs in, and $HC_TERMINAL_ID is not set; run it from the coordinator's terminal or pass --flock <slug>".to_string(),
        ));
    }
    Ok(())
}

fn identify(ctx: &Ctx, hc: &dyn Hc) -> Result<Option<Identity>, Error> {
    let Some(id) = terminal_id(ctx) else {
        return Ok(None);
    };
    let terminal = hc
        .get(&id)?
        .ok_or_else(|| Error::Flock(format!("hc has no terminal {id:?} ($HC_TERMINAL_ID)")))?;
    Ok(Some(Identity {
        address: terminal.address,
        flock: terminal.labels.get(names::LABEL_FLOCK).cloned(),
        child: terminal.labels.get(names::LABEL_CHILD).cloned(),
    }))
}

/// The flock a command is about: `--flock`, else the calling terminal's `crook.flock` label (Q4).
/// A child's terminal is refused, `--flock` or not: children run no crook commands (Q30).
pub fn resolve_slug(
    ctx: &Ctx,
    hc: &dyn Hc,
    flag: Option<&str>,
) -> Result<(String, Option<Identity>), Error> {
    let identity = match identify(ctx, hc) {
        Ok(identity) => identity,
        // `--flock` makes the calling terminal optional, so a stale id is not worth failing for.
        Err(_) if flag.is_some() => None,
        Err(e) => return Err(e),
    };
    if identity.as_ref().is_some_and(|i| i.child.is_some()) {
        return Err(child_terminal_error());
    }
    if let Some(name) = flag {
        let slug = flock::slug_for(ctx.env, name)?;
        names::validate_slug(&slug)?;
        return Ok((slug, identity));
    }
    let Some(identity) = identity else {
        require_context(ctx, None)?;
        return Err(Error::Env("no flock given".to_string()));
    };
    match identity.flock.clone() {
        Some(slug) => Ok((slug, Some(identity))),
        None => Err(Error::Flock(format!(
            "terminal {} has no {} label, so it is not a crook terminal; pass --flock <slug>",
            identity.address,
            names::LABEL_FLOCK
        ))),
    }
}

fn child_terminal_error() -> Error {
    Error::Flock("this terminal belongs to a child, and children run no crook commands".to_string())
}

/// For commands that take the flock as an argument: refuses a child's terminal. A calling terminal
/// hc cannot describe is not a reason to refuse.
pub fn refuse_child_terminal(ctx: &Ctx, hc: &dyn Hc) -> Result<(), Error> {
    match identify(ctx, hc) {
        Ok(Some(i)) if i.child.is_some() => Err(child_terminal_error()),
        _ => Ok(()),
    }
}

/// Whether a failed `hc run` may still have started the terminal: a timeout or a broken pipe says
/// nothing about what the core did, where hc's own refusal does.
pub fn outcome_unknown(e: &Error) -> bool {
    matches!(e, Error::Timeout { .. } | Error::Io { .. })
}

/// The live terminal with this hc name, if any.
pub fn live_named(hc: &dyn Hc, name: &str) -> Result<Option<Terminal>, Error> {
    Ok(hc.get(name)?.filter(Terminal::is_live))
}

/// The flock's live terminals: its coordinator and its children, as `hc ls` lists them (no labels).
pub fn live_terminals(hc: &dyn Hc, slug: &str) -> Result<Vec<Terminal>, Error> {
    hc.ls_by_label(false, &[(names::LABEL_FLOCK.to_string(), slug.to_string())])
}

/// The live terminal of `child`, a child in this flock's roster, for `send` and `read` (ADR 0028). Only a
/// terminal carrying this flock's label and the child's name counts, so neither command can reach a
/// terminal outside the flock.
pub fn live_child_terminal(
    ctx: &Ctx,
    hc: &dyn Hc,
    child: &str,
    flag: Option<&str>,
) -> Result<(String, Terminal), Error> {
    require_context(ctx, flag)?;
    names::validate_child(child)?;
    crate::hc::preflight::run(hc, &(ctx.vars)())?;
    let (slug, _) = resolve_slug(ctx, hc, flag)?;
    let flock = find_flock(ctx, &slug)?;
    let on_disk = crate::files::roster::load(&flock.folder)?;
    if !on_disk.children.iter().any(|c| c.name == child) {
        let known: Vec<&str> = on_disk.children.iter().map(|c| c.name.as_str()).collect();
        return Err(Error::Flock(format!(
            "flock {slug:?} has no child named {child:?} (children: {})",
            if known.is_empty() {
                "none".to_string()
            } else {
                known.join(", ")
            }
        )));
    }
    let name = names::hc_child(&slug, child);
    live_terminals(hc, &slug)?
        .into_iter()
        .find(|t| t.name.as_deref() == Some(name.as_str()))
        .map(|t| (slug.clone(), t))
        .ok_or_else(|| {
            Error::Flock(format!(
                "{name} has no live terminal; `crook resume {child}` starts it again"
            ))
        })
}

/// The live terminal named `<slug>`, whatever runs in it.
pub fn coordinator_terminal(hc: &dyn Hc, slug: &str) -> Result<Option<Terminal>, Error> {
    let name = names::hc_coordinator(slug);
    Ok(live_terminals(hc, slug)?
        .into_iter()
        .find(|t| t.name.as_deref() == Some(name.as_str())))
}

/// The coordinator's live terminal, if it has one: a terminal that is a bare shell is not it (D52).
pub fn live_coordinator(hc: &dyn Hc, slug: &str) -> Result<Option<Terminal>, Error> {
    Ok(coordinator_terminal(hc, slug)?.filter(|t| !t.is_shell()))
}

/// How long a killed terminal has to finish before giving up.
const FINISH_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
const FINISH_POLL: std::time::Duration = std::time::Duration::from_millis(100);

/// Kills a terminal and waits until hc says it has finished, so its name can be used again.
pub fn end_terminal(hc: &dyn Hc, address: &str) -> Result<(), Error> {
    hc.kill(address)?;
    let deadline = std::time::Instant::now() + FINISH_DEADLINE;
    loop {
        match hc.get(address)? {
            Some(t) if t.is_live() => {}
            _ => return Ok(()),
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::Flock(format!(
                "terminal {address} did not end within {FINISH_DEADLINE:?} after it was killed"
            )));
        }
        std::thread::sleep(FINISH_POLL);
    }
}

/// The terminal to be a new child's parent: the calling terminal when it is the coordinator, else
/// the coordinator's live terminal; a flock with none is refused (ADR 0020).
pub fn parent_terminal(
    hc: &dyn Hc,
    slug: &str,
    identity: Option<&Identity>,
) -> Result<String, Error> {
    if let Some(i) = identity
        && i.flock.as_deref() == Some(slug)
        && i.child.is_none()
    {
        return Ok(i.address.clone());
    }
    live_coordinator(hc, slug)?
        .map(|t| t.address)
        .ok_or_else(|| {
            Error::Flock(format!(
                "flock {slug:?} has no live coordinator to be the parent; start it with `crook open {slug}`"
            ))
        })
}

/// Tries to remove a child's worktree and warns when it is kept, with the reason (Q6, Q44). Never
/// fails: by now the roster says what it says, and the maintainer can still clean up by hand.
pub fn retire_worktree(ctx: &Ctx, io: &mut Io, child: &Child) {
    let (Some(repo), Some(branch)) = (&child.repo, &child.branch) else {
        return;
    };
    let worktree = Worktree {
        path: child.folder.clone(),
        branch: branch.clone(),
    };
    match git::remove_worktree(ctx.runner, repo, &worktree) {
        Ok(Removal::Removed | Removal::Gone) => {}
        Ok(Removal::Kept(reasons)) => io.warn(format!(
            "kept worktree {}: {}",
            child.folder.display(),
            reasons.join("; ")
        )),
        Err(e) => io.warn(format!(
            "kept worktree {}: could not check it: {e}",
            child.folder.display()
        )),
    }
}

#[cfg(test)]
mod tests;
