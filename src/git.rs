//! Child worktrees (ADR 0023). The only module that runs `git`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::run::{Cmd, Output, Runner};
use crate::{Error, names};

#[cfg(test)]
mod tests;

const TIMEOUT: Duration = Duration::from_secs(60);

/// A worktree crook made for a child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    Removed,
    /// The folder was already gone, so there was nothing to remove.
    Gone,
    /// Kept, with every reason (the caller prints a `warning:` line).
    Kept(Vec<String>),
}

fn git_error(args: &[&str], message: impl Into<String>) -> Error {
    Error::Git {
        args: args.join(" "),
        message: message.into(),
    }
}

fn run(runner: &dyn Runner, cwd: &Path, args: &[&str]) -> Result<Output, Error> {
    runner.run(Cmd::new("git").args(args.iter().copied()).cwd(cwd), TIMEOUT)
}

/// Runs git and returns trimmed stdout. Any non-zero exit is an error carrying git's message.
fn run_ok(runner: &dyn Runner, cwd: &Path, args: &[&str]) -> Result<String, Error> {
    let out = run(runner, cwd, args)?;
    if out.is_success() {
        return Ok(out.stdout.trim().to_string());
    }
    Err(git_error(args, out.stderr.trim()))
}

/// Runs a git question whose exit code is the answer: 0 is yes, 1 is no, anything else is an error.
fn run_yes_no(runner: &dyn Runner, cwd: &Path, args: &[&str]) -> Result<bool, Error> {
    let out = run(runner, cwd, args)?;
    match out.code {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(git_error(args, out.stderr.trim())),
    }
}

/// Resolves `--repo` to the top level of its repository.
pub fn toplevel(runner: &dyn Runner, path: &Path) -> Result<PathBuf, Error> {
    run_ok(runner, path, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// `git worktree add -b crook/<slug>/<child> <repo>/.worktrees/<slug>-<child>`, from the current
/// `HEAD`. An existing folder or branch is refused, never reused.
pub fn worktree_add(
    runner: &dyn Runner,
    repo: &Path,
    slug: &str,
    child: &str,
) -> Result<Worktree, Error> {
    names::validate_slug(slug)?;
    names::validate_child(child)?;
    let branch = names::branch(slug, child);
    let path = repo
        .join(".worktrees")
        .join(names::worktree_dir(slug, child));
    if path.symlink_metadata().is_ok() {
        return Err(git_error(
            &["worktree", "add"],
            format!("{} already exists", path.display()),
        ));
    }
    let ref_name = format!("refs/heads/{branch}");
    if run_yes_no(
        runner,
        repo,
        &["show-ref", "--verify", "--quiet", &ref_name],
    )? {
        return Err(git_error(
            &["worktree", "add"],
            format!("the branch {branch} already exists"),
        ));
    }
    let path_arg = path.to_string_lossy();
    run_ok(runner, repo, &["worktree", "add", "-b", &branch, &path_arg])?;
    Ok(Worktree { path, branch })
}

/// The commit the main worktree has checked out, which is the first record of the porcelain list.
fn main_head(runner: &dyn Runner, repo: &Path) -> Result<Option<String>, Error> {
    let list = run_ok(runner, repo, &["worktree", "list", "--porcelain"])?;
    let first = list.split("\n\n").next().unwrap_or_default();
    Ok(first
        .lines()
        .find_map(|l| l.strip_prefix("HEAD "))
        .map(str::to_string))
}

/// Files in the git dir that mean an operation is half done, and its name.
const IN_PROGRESS: [(&str, &str); 6] = [
    ("rebase-merge", "rebase"),
    ("rebase-apply", "rebase"),
    ("MERGE_HEAD", "merge"),
    ("CHERRY_PICK_HEAD", "cherry-pick"),
    ("REVERT_HEAD", "revert"),
    ("BISECT_LOG", "bisect"),
];

/// How many ignored paths a reason names before it counts the rest.
const NAMED_IGNORED: usize = 3;

fn ignored_reason(paths: &[&str]) -> String {
    let named = paths[..paths.len().min(NAMED_IGNORED)].join(", ");
    match paths.len().checked_sub(NAMED_IGNORED) {
        Some(more) if more > 0 => format!("ignored files present: {named} (+{more} more)"),
        _ => format!("ignored files present: {named}"),
    }
}

/// Why this worktree may not be removed; empty means it may (Q13, ADR 0023). Nothing is changed.
///
/// Everything that `git worktree remove` would delete or orphan counts: changes, untracked and ignored
/// files, a HEAD that is not on the child's branch (its commits are on no branch), an operation in
/// progress, and commits that are neither pushed nor merged.
pub fn removal_blockers(
    runner: &dyn Runner,
    repo: &Path,
    worktree: &Worktree,
) -> Result<Vec<String>, Error> {
    let wt = &worktree.path;
    // A worktree whose `.git` file is gone makes git walk up into the parent repository, and
    // `status` would then describe the wrong tree.
    let top = run(runner, wt, &["rev-parse", "--show-toplevel"])?;
    let is_here = top.is_success()
        && wt
            .canonicalize()
            .is_ok_and(|real| real == Path::new(top.stdout.trim()));
    if !is_here {
        return Ok(vec![format!("{} is not a git worktree", wt.display())]);
    }

    let mut reasons = Vec::new();
    let git_dir = PathBuf::from(run_ok(runner, wt, &["rev-parse", "--absolute-git-dir"])?);
    for (marker, what) in IN_PROGRESS {
        if git_dir.join(marker).exists() {
            reasons.push(format!("a {what} is in progress"));
        }
    }

    let on_branch = run(runner, wt, &["symbolic-ref", "--quiet", "HEAD"])?;
    let ref_name = format!("refs/heads/{}", worktree.branch);
    if !(on_branch.is_success() && on_branch.stdout.trim() == ref_name) {
        reasons.push(format!("its HEAD is not on {}", worktree.branch));
    }

    // Forced settings, so a developer's `status.showUntrackedFiles=no` cannot hide files.
    let status = run_ok(
        runner,
        wt,
        &[
            "-c",
            "status.showUntrackedFiles=all",
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignore-submodules=none",
            "--ignored=matching",
        ],
    )?;
    let lines: Vec<&str> = status.lines().collect();
    if lines
        .iter()
        .any(|l| !l.starts_with("??") && !l.starts_with("!!"))
    {
        reasons.push("it has uncommitted changes".to_string());
    }
    if lines.iter().any(|l| l.starts_with("??")) {
        reasons.push("it has untracked files".to_string());
    }
    let ignored: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("!! ")).collect();
    if !ignored.is_empty() {
        reasons.push(ignored_reason(&ignored));
    }

    // Both the branch tip and the commit the worktree is really on, which differ after a detached
    // commit.
    let tip = run_ok(runner, repo, &["rev-parse", "--verify", &ref_name])?;
    let head = run_ok(runner, wt, &["rev-parse", "--verify", "HEAD"])?;
    let main = main_head(runner, repo)?;
    let mut commits = vec![(tip.clone(), format!("its branch {}", worktree.branch))];
    if head != tip {
        commits.push((head, "its HEAD commit".to_string()));
    }
    for (commit, what) in &commits {
        let remotes = run_ok(
            runner,
            repo,
            &[
                "for-each-ref",
                "--contains",
                commit,
                "--format=%(refname)",
                "refs/remotes",
            ],
        )?;
        let merged = match &main {
            Some(m) => run_yes_no(runner, repo, &["merge-base", "--is-ancestor", commit, m])?,
            None => false,
        };
        if remotes.is_empty() && !merged {
            reasons.push(format!(
                "{what} has commits that are neither pushed nor merged"
            ));
        }
    }
    Ok(reasons)
}

/// Removes the worktree when it has no uncommitted or untracked changes and its branch is pushed or
/// merged. Never forces, and never deletes the branch (ADR 0023).
pub fn remove_worktree(
    runner: &dyn Runner,
    repo: &Path,
    worktree: &Worktree,
) -> Result<Removal, Error> {
    if worktree.path.symlink_metadata().is_err() {
        return Ok(Removal::Gone);
    }
    let reasons = removal_blockers(runner, repo, worktree)?;
    if !reasons.is_empty() {
        return Ok(Removal::Kept(reasons));
    }
    let path_arg = worktree.path.to_string_lossy();
    let out = run(runner, repo, &["worktree", "remove", &path_arg])?;
    if out.is_success() {
        Ok(Removal::Removed)
    } else {
        Ok(Removal::Kept(vec![out.stderr.trim().to_string()]))
    }
}

/// Whether `<repo>/.worktrees/` is ignored, by the repository's `.gitignore` or any other ignore
/// file git reads, for `doctor` (ADR 0023). Probes a path inside the folder, which also matches a
/// pattern written as `.worktrees/`.
pub fn is_ignored(runner: &dyn Runner, repo: &Path) -> Result<bool, Error> {
    run_yes_no(
        runner,
        repo,
        &["check-ignore", "--quiet", ".worktrees/probe"],
    )
}
