//! `crook doctor [--fix] [--json]`: checks the install and, with `--fix`, repairs what `setup` can
//! (ADR 0016, 0019, 0023, 0026; Q23, Q38, Q41).
//!
//! `--fix` runs `setup`'s mod install when the mod is missing or older, moves the `current` link when
//! `setup` would, and prunes old session records. Everything else is report-only.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::json;

use super::Io;
use super::setup::{self, ModState};
use crate::files::roster::{self, State};
use crate::hc::{Hc, preflight};
use crate::hook::version::prune;
use crate::install::{self, Install, compare_versions};
use crate::run::Cmd;
use crate::{Ctx, Error, config, flock, git, paths};

#[cfg(test)]
mod tests;

/// The `doctor --json` format number.
pub const FORMAT_VERSION: u32 = 1;

const LOGIN_SHELL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub status: Status,
    pub detail: String,
    pub fix: Option<String>,
}

fn check(id: &'static str, status: Status, detail: impl Into<String>, fix: Option<&str>) -> Check {
    Check {
        id,
        status,
        detail: detail.into(),
        fix: fix.map(str::to_string),
    }
}

/// The install under test, or why it could not be found.
pub type Subject<'a> = Result<&'a Install, String>;

/// A broken install is no reason to print nothing: it is reported as failing checks (D84).
pub fn run(ctx: &Ctx, hc: &dyn Hc, io: &mut Io, fix: bool, json: bool) -> Result<(), Error> {
    let found = install::find_own(ctx).map_err(|e| e.to_string());
    report(
        ctx,
        hc,
        io,
        found.as_ref().map_err(Clone::clone),
        install::DEBUG_BUILD,
        fix,
        json,
    )
}

pub fn perform(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    found: &Install,
    debug: bool,
    fix: bool,
    json: bool,
) -> Result<(), Error> {
    report(ctx, hc, io, Ok(found), debug, fix, json)
}

pub fn report(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    found: Subject,
    debug: bool,
    fix: bool,
    json: bool,
) -> Result<(), Error> {
    let mut checks = collect(ctx, hc, found.clone(), debug);
    let mut fixes = Vec::new();
    if fix {
        fixes = apply_fixes(ctx, found.clone(), debug, &checks);
        checks = collect(ctx, hc, found.clone(), debug);
    }
    let failed = checks.iter().filter(|c| c.status == Status::Fail).count();
    let app_home = paths::herdr_app_home(ctx.env).ok();
    let known = found.as_ref().ok().copied();
    if json {
        let doc = json!({
            "version": FORMAT_VERSION,
            "crook_version": known.and_then(|f| f.version().ok()),
            "plugin_dir": known.map(Install::plugin_dir),
            "mod_dir": known.map(Install::mod_dir),
            "herdr_app_home": app_home,
            "ok": failed == 0,
            "checks": checks.iter().map(|c| json!({
                "id": c.id,
                "status": c.status.word(),
                "detail": c.detail,
                "fix": c.fix,
            })).collect::<Vec<_>>(),
            "fixes": fixes,
        });
        io.say(doc);
    } else {
        let shown = |p: Option<std::path::PathBuf>| {
            p.map_or_else(|| "unknown".to_string(), |p| p.display().to_string())
        };
        io.say(format!("plugin: {}", shown(known.map(Install::plugin_dir))));
        io.say(format!("mod:    {}", shown(known.map(Install::mod_dir))));
        io.say(format!("herdr-app home: {}", shown(app_home)));
        for f in &fixes {
            io.say(format!("fixed: {f}"));
        }
        for c in &checks {
            io.say(format!("{:<5} {}: {}", c.status.word(), c.id, c.detail));
            if let Some(fix) = &c.fix {
                io.say(format!("      fix: {fix}"));
            }
        }
    }
    if failed > 0 {
        return Err(Error::Checks(failed));
    }
    Ok(())
}

/// What `--fix` does, going by the checks that failed. Each failure to fix is recorded as text and
/// leaves its check failing in the report.
fn apply_fixes(ctx: &Ctx, found: Subject, debug: bool, checks: &[Check]) -> Vec<String> {
    let failing = |id: &str| {
        checks
            .iter()
            .any(|c| c.id == id && c.status == Status::Fail)
    };
    let mut done = Vec::new();
    if let Ok(found) = found
        && let Ok(version) = found.version()
    {
        if failing("mod")
            && let Ok(state) = setup::inspect_mod(ctx.env)
            && let Ok(dest) = paths::mod_install(ctx.env)
            && setup::replaceable(&state, &version, &dest).is_ok()
        {
            match setup::install_mod(ctx, found, &version) {
                Ok(dest) => done.push(format!("installed the mod {version} in {}", dest.display())),
                Err(e) => done.push(format!("could not install the mod: {e}")),
            }
        }
        if failing("current-link") && !debug && link_state(ctx, found).is_some_and(|s| s.fixable())
        {
            match install::point_current(ctx.env, found, debug) {
                Ok(_) => done.push(format!(
                    "pointed the current link at {}",
                    found.root().display()
                )),
                Err(e) => done.push(format!("could not move the current link: {e}")),
            }
        }
    }
    match prune(ctx) {
        Ok(n) => done.push(format!("pruned {n} old session record(s)")),
        Err(e) => done.push(format!("could not prune the session records: {e}")),
    }
    done
}

/// Every check, none of which stops the others: an error inside one is that check failing.
pub fn collect(ctx: &Ctx, hc: &dyn Hc, found: Subject, debug: bool) -> Vec<Check> {
    vec![
        hc_check(ctx, hc),
        herdr_app_check(ctx),
        login_shell_check(ctx),
        guard("shim", shim_check(ctx)),
        guard("mod", mod_check(ctx, found.clone())),
        guard("current-link", link_check(ctx, found, debug)),
        config_check(ctx),
        worktrees_check(ctx),
    ]
}

fn guard(id: &'static str, result: Result<Check, Error>) -> Check {
    result.unwrap_or_else(|e| check(id, Status::Fail, e.to_string(), None))
}

fn unusable(id: &'static str, why: &str) -> Check {
    check(
        id,
        Status::Fail,
        format!("this crook's install cannot be read: {why}"),
        Some("reinstall crook with mise"),
    )
}

fn hc_check(ctx: &Ctx, hc: &dyn Hc) -> Check {
    let version = match hc.version() {
        Ok(v) => v,
        Err(e) => {
            return check(
                "hc",
                Status::Fail,
                format!("hc did not answer: {e}"),
                Some(&format!(
                    "install hc {} or newer and put it on PATH",
                    preflight::MIN_HC_VERSION
                )),
            );
        }
    };
    match preflight::run(hc, &(ctx.vars)()) {
        Ok(()) => check(
            "hc",
            Status::Ok,
            format!("hc {version}, core up, schema has what crook needs"),
            None,
        ),
        Err(e) => check(
            "hc",
            Status::Fail,
            e.to_string(),
            Some(&format!(
                "install hc {} or newer",
                preflight::MIN_HC_VERSION
            )),
        ),
    }
}

fn on_path(path_var: &str, program: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path_var)
        .map(|dir| dir.join(program))
        .find(|p| {
            std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

fn herdr_app_check(ctx: &Ctx) -> Check {
    let path = (ctx.env)("PATH").unwrap_or_default();
    match on_path(&path, "herdr-app") {
        Some(p) => check(
            "herdr-app",
            Status::Ok,
            format!("found {}", p.display()),
            None,
        ),
        None => check(
            "herdr-app",
            Status::Fail,
            "herdr-app is not on PATH",
            Some("install herdr-app and put it on PATH"),
        ),
    }
}

fn login_shell_check(ctx: &Ctx) -> Check {
    const FIX: &str = "activate mise in the login shell's startup files; crook's hooks use the shim and do not need this";
    let Some(shell) = (ctx.env)("SHELL").filter(|s| !s.is_empty()) else {
        return check(
            "login-shell",
            Status::Warn,
            "SHELL is not set, so the login shell was not asked",
            Some(FIX),
        );
    };
    // A minimal environment, so a PATH crook inherited (from mise, say) cannot answer for the login
    // shell's own startup files (D82).
    let mut env: Vec<(String, String)> = vec![("PATH".into(), "/usr/bin:/bin".into())];
    for key in ["HOME", "USER", "TERM"] {
        if let Some(v) = (ctx.env)(key).filter(|v| !v.is_empty()) {
            env.push((key.into(), v));
        }
    }
    env.push(("SHELL".into(), shell.clone()));
    let cmd = Cmd::new(shell.clone())
        .args(["-l", "-c", "command -v crook"])
        .env(env);
    match ctx.runner.run(cmd, LOGIN_SHELL_TIMEOUT) {
        Ok(out) if out.is_success() => {
            match out.stdout.lines().map(str::trim).rfind(|l| !l.is_empty()) {
                Some(found) => check(
                    "login-shell",
                    Status::Ok,
                    format!("`{shell} -l` finds crook at {found}"),
                    None,
                ),
                None => check(
                    "login-shell",
                    Status::Warn,
                    format!("`{shell} -l` does not find crook"),
                    Some(FIX),
                ),
            }
        }
        Ok(_) => check(
            "login-shell",
            Status::Warn,
            format!("`{shell} -l` does not find crook"),
            Some(FIX),
        ),
        Err(e) => check(
            "login-shell",
            Status::Warn,
            format!("could not run `{shell} -l`: {e}"),
            Some(FIX),
        ),
    }
}

fn shim_check(ctx: &Ctx) -> Result<Check, Error> {
    let shim = paths::mise_shim(ctx.env)?;
    if shim.exists() {
        return Ok(check(
            "shim",
            Status::Ok,
            format!("found {}", shim.display()),
            None,
        ));
    }
    if let Some(bin) = (ctx.env)("CROOK_BIN").filter(|b| !b.is_empty())
        && Path::new(&bin).is_absolute()
        && Path::new(&bin).is_file()
    {
        return Ok(check(
            "shim",
            Status::Ok,
            format!(
                "{} is missing, but CROOK_BIN={bin} stands in for it",
                shim.display()
            ),
            None,
        ));
    }
    Ok(check(
        "shim",
        Status::Warn,
        format!("the mise shim {} does not exist", shim.display()),
        Some("install crook with mise, or set CROOK_BIN to the absolute path of the crook binary"),
    ))
}

fn mod_check(ctx: &Ctx, found: Subject) -> Result<Check, Error> {
    let found = match found {
        Ok(f) => f,
        Err(why) => return Ok(unusable("mod", &why)),
    };
    let version = match found.version() {
        Ok(v) => v,
        Err(e) => return Ok(unusable("mod", &e.to_string())),
    };
    let dest = paths::mod_install(ctx.env)?;
    let state = setup::inspect_mod(ctx.env)?;
    Ok(match &state {
        ModState::Missing => check(
            "mod",
            Status::Fail,
            format!("the mod is not installed in {}", dest.display()),
            Some("run `crook setup`, or `crook doctor --fix`"),
        ),
        ModState::Unmarked => check(
            "mod",
            Status::Fail,
            format!("{} belongs to someone else (no marker)", dest.display()),
            Some("move it aside, then run `crook setup`"),
        ),
        ModState::Marked(marker) => match compare_versions(marker, &version) {
            Some(Ordering::Equal) => check(
                "mod",
                Status::Ok,
                format!("the mod {marker} is installed in {}", dest.display()),
                None,
            ),
            Some(Ordering::Less) => check(
                "mod",
                Status::Fail,
                format!("the installed mod is {marker}, this crook is {version}"),
                Some("run `crook setup`, or `crook doctor --fix`"),
            ),
            Some(Ordering::Greater) => check(
                "mod",
                Status::Fail,
                format!("the installed mod is {marker}, newer than this crook ({version})"),
                Some("upgrade crook; setup never replaces a newer mod"),
            ),
            None => check(
                "mod",
                Status::Fail,
                format!("the mod's marker says {marker:?}, which is not a version"),
                Some("remove the mod folder, then run `crook setup`"),
            ),
        },
    })
}

#[derive(Debug, PartialEq, Eq)]
enum LinkState {
    Missing,
    Dangling,
    NotALink,
    Here,
    /// Points at an install that is not this one and is not older: the link only moves forward.
    Elsewhere,
    Older,
}

impl LinkState {
    /// Whether `setup`'s link move repairs it.
    fn fixable(&self) -> bool {
        matches!(
            self,
            LinkState::Missing | LinkState::Dangling | LinkState::Older
        )
    }
}

fn link_state(ctx: &Ctx, found: &Install) -> Option<LinkState> {
    let link = paths::current_link(ctx.env).ok()?;
    let meta = match std::fs::symlink_metadata(&link) {
        Ok(m) => m,
        Err(_) => return Some(LinkState::Missing),
    };
    if !meta.file_type().is_symlink() {
        return Some(LinkState::NotALink);
    }
    let raw = std::fs::read_link(&link).ok()?;
    let target = link.parent().unwrap_or(Path::new("/")).join(raw);
    if !target.exists() {
        return Some(LinkState::Dangling);
    }
    if target.canonicalize().ok().as_deref() == Some(found.root()) {
        return Some(LinkState::Here);
    }
    let older = match (install::plugin_version(&target), found.version()) {
        (Ok(theirs), Ok(mine)) => compare_versions(&mine, &theirs) == Some(Ordering::Greater),
        _ => false,
    };
    Some(if older {
        LinkState::Older
    } else {
        LinkState::Elsewhere
    })
}

fn link_check(ctx: &Ctx, found: Subject, debug: bool) -> Result<Check, Error> {
    let link = paths::current_link(ctx.env)?;
    if debug {
        return Ok(check(
            "current-link",
            Status::Ok,
            "a debug build uses its own plugin/ and never moves the link",
            None,
        ));
    }
    let found = match found {
        Ok(f) => f,
        Err(why) => return Ok(unusable("current-link", &why)),
    };
    const FIX: &str = "run `crook setup`, or `crook doctor --fix`";
    let here = link.display();
    Ok(match link_state(ctx, found) {
        Some(LinkState::Here) => check(
            "current-link",
            Status::Ok,
            format!("{here} points at {}", found.root().display()),
            None,
        ),
        Some(LinkState::Elsewhere) => check(
            "current-link",
            Status::Warn,
            format!("{here} points at another install that is not older than this one"),
            None,
        ),
        Some(LinkState::Older) => check(
            "current-link",
            Status::Fail,
            format!("{here} points at an older install"),
            Some(FIX),
        ),
        Some(LinkState::Dangling) => check(
            "current-link",
            Status::Fail,
            format!("{here} points at a folder that is gone"),
            Some(FIX),
        ),
        Some(LinkState::Missing) => check(
            "current-link",
            Status::Fail,
            format!("{here} does not exist"),
            Some(FIX),
        ),
        Some(LinkState::NotALink) => check(
            "current-link",
            Status::Fail,
            format!("{here} is not a link, so crook leaves it alone"),
            Some("move it aside, then run `crook setup`"),
        ),
        None => check(
            "current-link",
            Status::Fail,
            format!("could not read {here}"),
            Some(FIX),
        ),
    })
}

fn config_check(ctx: &Ctx) -> Check {
    let path = match paths::config_file(ctx.env) {
        Ok(p) => p,
        Err(e) => return check("config", Status::Fail, e.to_string(), None),
    };
    match config::load(ctx.env) {
        Ok(_) if path.exists() => check(
            "config",
            Status::Ok,
            format!("read {}", path.display()),
            None,
        ),
        Ok(_) => check(
            "config",
            Status::Ok,
            format!("no {}, so the defaults apply", path.display()),
            None,
        ),
        Err(e) => check(
            "config",
            Status::Fail,
            e.to_string(),
            Some("fix the file named above"),
        ),
    }
}

fn worktrees_check(ctx: &Ctx) -> Check {
    const FIX: &str = "add `.worktrees/` to the repository's .gitignore or .git/info/exclude";
    let flocks = match flock::list(ctx.env) {
        Ok(f) => f,
        Err(e) => {
            return check(
                "worktrees-ignored",
                Status::Warn,
                format!("could not list the flocks: {e}"),
                None,
            );
        }
    };
    let mut repos: Vec<PathBuf> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for flock in &flocks {
        let roster = match roster::load(&flock.folder) {
            Ok(r) => r,
            Err(e) => {
                problems.push(format!("could not read the roster of {}: {e}", flock.slug));
                continue;
            }
        };
        if roster.coordinator.state != State::Active {
            continue;
        }
        for repo in roster.children.iter().filter_map(|c| c.repo.clone()) {
            if !repos.contains(&repo) {
                repos.push(repo);
            }
        }
    }
    let mut unignored = Vec::new();
    for repo in &repos {
        match git::is_ignored(ctx.runner, repo) {
            Ok(true) => {}
            Ok(false) => unignored.push(repo.display().to_string()),
            Err(e) => problems.push(format!("could not check {}: {e}", repo.display())),
        }
    }
    if !unignored.is_empty() {
        let mut detail = format!(".worktrees/ is not ignored in {}", unignored.join(", "));
        for p in &problems {
            detail.push_str(&format!("; {p}"));
        }
        return check("worktrees-ignored", Status::Warn, detail, Some(FIX));
    }
    if !problems.is_empty() {
        return check("worktrees-ignored", Status::Warn, problems.join("; "), None);
    }
    check(
        "worktrees-ignored",
        Status::Ok,
        format!(
            "{} repository(ies) of active flocks ignore .worktrees/",
            repos.len()
        ),
        None,
    )
}
