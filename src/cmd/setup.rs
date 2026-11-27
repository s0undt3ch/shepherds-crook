//! `crook setup`: installs the herdr-app mod, moves `current`, makes the flocks root, prints the
//! trust commands and prunes old session records (ADR 0013, 0019; Q17, Q22, Q41).
//!
//! The mod is built in a staging folder next to `mods/`, never inside it, because herdr-app treats a
//! new folder under `mods/` as a mod and reloads an open one on every file written. The marker goes in
//! last and the folder is swapped in with renames. Only this routine installs or refreshes the mod;
//! `doctor --fix` calls it.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use super::Io;
use crate::config;
use crate::hook::version::prune;
use crate::install::{self, Install, LinkOutcome, compare_versions};
use crate::{Ctx, Error, paths};

#[cfg(test)]
mod tests;

/// The marker file that says crook installed the folder, and which version.
pub const MARKER: &str = ".crook-version";

/// Files and folders of `mod/` that are not installed: the herdr.dev recipe, the preview, and what
/// `herdr-app check` writes into a mod folder. Dot files are left out too.
const NOT_INSTALLED: [&str; 4] = ["RECIPE.md", "preview.png", "jsconfig.json", "gpui-kit.d.ts"];

pub fn run(ctx: &Ctx, io: &mut Io) -> Result<(), Error> {
    let found = install::find_own(ctx)?;
    perform(ctx, io, &found, install::DEBUG_BUILD)
}

/// `debug` is [`install::DEBUG_BUILD`] outside tests: a debug build never moves `current`.
pub fn perform(ctx: &Ctx, io: &mut Io, found: &Install, debug: bool) -> Result<(), Error> {
    let version = found.version()?;
    let dest = install_mod(ctx, found, &version)?;
    io.say(format!("installed the mod {version} in {}", dest.display()));

    match install::point_current(ctx.env, found, debug) {
        Ok(LinkOutcome::Created | LinkOutcome::Moved) => io.say(format!(
            "pointed {} at {}",
            paths::current_link(ctx.env)?.display(),
            found.root().display()
        )),
        Ok(LinkOutcome::Unchanged) => {
            io.say("the current link already points here or at a newer install")
        }
        Ok(LinkOutcome::DebugBuild) => io.say("debug build: the current link is left alone"),
        Ok(LinkOutcome::LeftAlone(why)) => io.warn(why),
        Err(e) => io.warn(format!("could not update the current link: {e}")),
    }

    let flocks = paths::flocks_root(ctx.env)?;
    make_dir(&flocks)?;
    let mut roots = vec![flocks];
    match config::load(ctx.env) {
        Ok(config) => {
            if let Some(notes) = config.notes_root {
                make_dir(&notes)?;
                roots.push(notes);
            }
        }
        Err(e) => io.warn(format!(
            "could not read the config, so the notes root is not listed: {e}"
        )),
    }

    match prune(ctx) {
        Ok(n) => io.say(format!("pruned {n} old session record(s)")),
        Err(e) => io.warn(format!("could not prune the session records: {e}")),
    }

    io.say("To trust each root once, run these and accept the prompt in each:");
    for root in &roots {
        io.say(format!("  cd {} && claude", shell_quote(root)));
    }
    Ok(())
}

/// What stands at `<app home>/mods/crook` (ADR 0019).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModState {
    Missing,
    /// Something that crook did not install: no marker, or not a folder.
    Unmarked,
    /// A folder crook installed, with the text of its marker.
    Marked(String),
}

pub fn inspect_mod(env: crate::Env) -> Result<ModState, Error> {
    let dest = paths::mod_install(env)?;
    match std::fs::symlink_metadata(&dest) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ModState::Missing),
        Err(source) => return Err(Error::File { path: dest, source }),
        Ok(meta) if !meta.is_dir() => return Ok(ModState::Unmarked),
        Ok(_) => {}
    }
    match std::fs::read_to_string(dest.join(MARKER)) {
        Ok(text) => Ok(ModState::Marked(text.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ModState::Unmarked),
        Err(source) => Err(Error::File {
            path: dest.join(MARKER),
            source,
        }),
    }
}

/// Whether setup may replace what is installed, with the reason it may not (Q22).
pub fn replaceable(state: &ModState, version: &str, dest: &Path) -> Result<(), Error> {
    match state {
        ModState::Missing => Ok(()),
        ModState::Unmarked => Err(Error::Install(format!(
            "{} exists without a {MARKER} marker, so it belongs to someone else and crook leaves it alone; move it aside and run `crook setup` again",
            dest.display()
        ))),
        ModState::Marked(marker) => match compare_versions(marker, version) {
            Some(Ordering::Greater) => Err(Error::Install(format!(
                "the mod installed in {} is {marker}, newer than this crook ({version}); crook never replaces a newer mod. Upgrade crook, then run `crook setup`",
                dest.display()
            ))),
            Some(_) => Ok(()),
            None => Err(Error::Install(format!(
                "{} says {marker:?}, which is not a version, so crook does not replace the mod; remove the folder and run `crook setup` again",
                dest.join(MARKER).display()
            ))),
        },
    }
}

/// Installs the install's `mod/` as `<app home>/mods/crook/` and returns that folder.
pub fn install_mod(ctx: &Ctx, found: &Install, version: &str) -> Result<PathBuf, Error> {
    install_mod_with(ctx, found, version, &|_| {})
}

/// As [`install_mod`]; `between` sees the staging folder after the files are copied and before the
/// marker is written, for the test of that order.
pub fn install_mod_with(
    ctx: &Ctx,
    found: &Install,
    version: &str,
    between: &dyn Fn(&Path),
) -> Result<PathBuf, Error> {
    let source = found.mod_dir();
    if !source.join("mod.json").is_file() {
        return Err(Error::Install(format!(
            "the install has no mod to install: {} is missing",
            source.join("mod.json").display()
        )));
    }
    let dest = paths::mod_install(ctx.env)?;
    let app_home = paths::herdr_app_home(ctx.env)?;
    let staging = app_home.join(".crook-mod-staging");
    let old = app_home.join(".crook-mod-old");
    restore_interrupted(&dest, &old)?;
    replaceable(&inspect_mod(ctx.env)?, version, &dest)?;

    for stale in [&staging, &old] {
        remove_dir(stale)?;
    }
    make_dir(&staging)?;
    let staged = copy_files(&source, &staging)
        .map(|()| between(&staging))
        .and_then(|()| write_marker(&staging, version))
        .and_then(|()| swap(&staging, &dest, &old));
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    staged?;
    Ok(dest)
}

/// A setup killed between its two renames leaves the old mod aside and none in place. Put the old one
/// back before deciding anything, so the marker checks see what was installed (D85).
fn restore_interrupted(dest: &Path, old: &Path) -> Result<(), Error> {
    let gone = matches!(
        std::fs::symlink_metadata(dest),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound
    );
    if gone && old.is_dir() {
        if let Some(mods) = dest.parent() {
            make_dir(mods)?;
        }
        std::fs::rename(old, dest).map_err(io_err(dest))?;
    }
    Ok(())
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> Error + use<> {
    let path = path.to_path_buf();
    move |source| Error::File { path, source }
}

fn make_dir(path: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(io_err(path))
}

fn remove_dir(path: &Path) -> Result<(), Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err(path)(e)),
    }
}

fn copy_files(from: &Path, to: &Path) -> Result<(), Error> {
    for entry in std::fs::read_dir(from).map_err(io_err(from))? {
        let entry = entry.map_err(io_err(from))?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text.starts_with('.') || NOT_INSTALLED.contains(&name_text.as_ref()) {
            continue;
        }
        let kind = entry.file_type().map_err(io_err(&entry.path()))?;
        let target = to.join(&name);
        if kind.is_dir() {
            make_dir(&target)?;
            copy_files(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map_err(io_err(&target))?;
        }
    }
    Ok(())
}

fn write_marker(staging: &Path, version: &str) -> Result<(), Error> {
    let path = staging.join(MARKER);
    std::fs::write(&path, format!("{version}\n")).map_err(io_err(&path))
}

/// Renames the old folder aside, the staged one in, and deletes the old one. A failure to put the
/// new one in puts the old one back.
fn swap(staging: &Path, dest: &Path, old: &Path) -> Result<(), Error> {
    if let Some(mods) = dest.parent() {
        make_dir(mods)?;
    }
    let renamed = |from: &Path, to: &Path| {
        std::fs::rename(from, to).map_err(|e| {
            if e.kind() == std::io::ErrorKind::CrossesDevices {
                Error::Install(format!(
                    "{} and {} are on different filesystems, so the mod cannot be swapped in with a rename; make `mods/` a folder of the herdr-app home itself",
                    from.display(),
                    to.display()
                ))
            } else {
                io_err(to)(e)
            }
        })
    };
    let had_old = std::fs::symlink_metadata(dest).is_ok();
    if had_old {
        renamed(dest, old)?;
    }
    if let Err(e) = renamed(staging, dest) {
        if had_old {
            let _ = std::fs::rename(old, dest);
        }
        return Err(e);
    }
    if had_old {
        // The new mod is in place; a leftover folder here is only untidy, and the next run removes it.
        let _ = std::fs::remove_dir_all(old);
    }
    Ok(())
}

/// A path as one shell word.
fn shell_quote(path: &Path) -> String {
    let text = path.display().to_string();
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:@,~".contains(&b));
    if plain {
        text
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}
