use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::Error;
use crate::run::{Cmd, Output, RealRunner, Runner};
use std::time::Duration;

/// A throwaway repository. The helper's own git calls read no developer config at all; the code under
/// test inherits the process environment, so the repository sets identity, signing, hooks and the
/// excludes file locally, which overrides the developer's.
struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn raw(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn commit(cwd: &Path, file: &str, text: &str) {
    std::fs::write(cwd.join(file), text).expect("write");
    raw(cwd, &["add", file]);
    raw(cwd, &["commit", "-q", "-m", file]);
}

impl Repo {
    fn new() -> Repo {
        let dir = tempfile::tempdir().expect("tempdir");
        // Canonical, so paths compare equal to what git prints on macOS (`/var` is a link).
        let root = dir.path().canonicalize().expect("canonical").join("repo");
        std::fs::create_dir(&root).expect("mkdir");
        raw(&root, &["init", "-q", "-b", "main"]);
        let empty_hooks = root.join(".git/no-hooks");
        std::fs::create_dir(&empty_hooks).expect("mkdir");
        let empty_excludes = root.join(".git/no-excludes");
        std::fs::write(&empty_excludes, "").expect("write");
        for (k, v) in [
            ("user.name", "Crook Test"),
            ("user.email", "crook@example.invalid"),
            ("commit.gpgsign", "false"),
            ("core.hooksPath", empty_hooks.to_str().expect("utf8")),
            ("core.excludesFile", empty_excludes.to_str().expect("utf8")),
        ] {
            raw(&root, &["config", k, v]);
        }
        commit(&root, "a.txt", "one\n");
        Repo { _dir: dir, root }
    }

    /// A bare repository beside this one, named `origin`.
    fn with_remote(&self) -> PathBuf {
        let bare = self.root.parent().expect("parent").join("origin.git");
        raw(
            self.root.parent().expect("parent"),
            &[
                "init",
                "-q",
                "--bare",
                "-b",
                "main",
                bare.to_str().expect("utf8"),
            ],
        );
        raw(
            &self.root,
            &["remote", "add", "origin", bare.to_str().expect("utf8")],
        );
        bare
    }

    fn add(&self, child: &str) -> Worktree {
        worktree_add(&RealRunner, &self.root, "demo", child).expect("adds")
    }
}

#[test]
fn toplevel_resolves_a_subfolder_to_the_repository_root() {
    let repo = Repo::new();
    let sub = repo.root.join("sub");
    std::fs::create_dir(&sub).expect("mkdir");
    assert_eq!(toplevel(&RealRunner, &sub).expect("resolves"), repo.root);
}

#[test]
fn toplevel_of_a_plain_folder_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(toplevel(&RealRunner, dir.path()).is_err());
}

#[test]
fn add_creates_the_path_and_the_branch_from_head() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    assert_eq!(wt.path, repo.root.join(".worktrees/demo-fix"));
    assert_eq!(wt.branch, "crook/demo/fix");
    assert!(wt.path.join("a.txt").is_file());
    assert_eq!(
        raw(&wt.path, &["branch", "--show-current"]),
        "crook/demo/fix"
    );
    assert_eq!(
        raw(&wt.path, &["rev-parse", "HEAD"]),
        raw(&repo.root, &["rev-parse", "main"])
    );
}

#[test]
fn add_refuses_an_existing_path() {
    let repo = Repo::new();
    std::fs::create_dir_all(repo.root.join(".worktrees/demo-fix")).expect("mkdir");
    let msg = worktree_add(&RealRunner, &repo.root, "demo", "fix")
        .expect_err("fails")
        .to_string();
    assert!(msg.contains("already exists"), "{msg}");
    assert_eq!(raw(&repo.root, &["branch", "--list", "crook/*"]), "");
}

#[test]
fn add_refuses_an_existing_branch() {
    let repo = Repo::new();
    raw(&repo.root, &["branch", "crook/demo/fix"]);
    let msg = worktree_add(&RealRunner, &repo.root, "demo", "fix")
        .expect_err("fails")
        .to_string();
    assert!(
        msg.contains("branch crook/demo/fix already exists"),
        "{msg}"
    );
    assert!(!repo.root.join(".worktrees/demo-fix").exists());
}

#[test]
fn add_refuses_a_bad_name_before_running_git() {
    let repo = Repo::new();
    assert!(worktree_add(&RealRunner, &repo.root, "demo", "../x").is_err());
    assert!(!repo.root.join(".worktrees").exists());
}

#[test]
fn a_dirty_worktree_blocks_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    std::fs::write(wt.path.join("a.txt"), "changed\n").expect("write");
    let Removal::Kept(reasons) = remove_worktree(&RealRunner, &repo.root, &wt).expect("runs")
    else {
        panic!("removed a dirty worktree");
    };
    assert_eq!(reasons, ["it has uncommitted changes"]);
    assert!(wt.path.exists());
}

#[test]
fn a_staged_change_blocks_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    std::fs::write(wt.path.join("b.txt"), "new\n").expect("write");
    raw(&wt.path, &["add", "b.txt"]);
    let Removal::Kept(reasons) = remove_worktree(&RealRunner, &repo.root, &wt).expect("runs")
    else {
        panic!("removed a dirty worktree");
    };
    assert_eq!(reasons, ["it has uncommitted changes"]);
}

#[test]
fn an_untracked_file_blocks_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    std::fs::write(wt.path.join("scratch.txt"), "x\n").expect("write");
    let Removal::Kept(reasons) = remove_worktree(&RealRunner, &repo.root, &wt).expect("runs")
    else {
        panic!("removed a worktree with untracked files");
    };
    assert_eq!(reasons, ["it has untracked files"]);
}

#[test]
fn unpushed_unmerged_commits_block_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    let Removal::Kept(reasons) = remove_worktree(&RealRunner, &repo.root, &wt).expect("runs")
    else {
        panic!("removed unpushed work");
    };
    assert_eq!(reasons.len(), 1);
    assert!(
        reasons[0].contains("neither pushed nor merged"),
        "{reasons:?}"
    );
    assert!(wt.path.exists());
}

#[test]
fn every_reason_is_reported_at_once() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    std::fs::write(wt.path.join("a.txt"), "changed\n").expect("write");
    std::fs::write(wt.path.join("scratch.txt"), "x\n").expect("write");
    let Removal::Kept(reasons) = remove_worktree(&RealRunner, &repo.root, &wt).expect("runs")
    else {
        panic!("removed");
    };
    assert_eq!(reasons.len(), 3, "{reasons:?}");
}

#[test]
fn a_branch_pushed_to_a_bare_remote_allows_removal() {
    let repo = Repo::new();
    let bare = repo.with_remote();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    raw(&wt.path, &["push", "-q", "origin", "crook/demo/fix"]);
    assert!(bare.exists());
    assert_eq!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Removed
    );
    assert!(!wt.path.exists());
}

#[test]
fn a_commit_made_after_the_push_blocks_removal() {
    let repo = Repo::new();
    repo.with_remote();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    raw(&wt.path, &["push", "-q", "origin", "crook/demo/fix"]);
    commit(&wt.path, "c.txt", "three\n");
    assert!(matches!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Kept(_)
    ));
}

#[test]
fn a_branch_merged_into_the_main_worktrees_branch_allows_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    raw(&repo.root, &["merge", "-q", "--no-edit", "crook/demo/fix"]);
    assert_eq!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Removed
    );
}

#[test]
fn merged_follows_whatever_the_main_worktree_has_checked_out() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    // `main` has the work, but the main worktree is on another branch that does not.
    raw(&repo.root, &["merge", "-q", "--no-edit", "crook/demo/fix"]);
    raw(&repo.root, &["checkout", "-q", "-b", "side", "HEAD~1"]);
    assert!(matches!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Kept(_)
    ));
}

#[test]
fn a_branch_with_no_new_commits_counts_as_merged() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    assert_eq!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Removed
    );
}

#[test]
fn the_branch_survives_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    remove_worktree(&RealRunner, &repo.root, &wt).expect("runs");
    assert_eq!(
        raw(&repo.root, &["branch", "--list", "crook/demo/fix"]),
        "crook/demo/fix"
    );
    // Only the folder is gone, so the same child name still collides with the kept branch.
    assert!(
        worktree_add(&RealRunner, &repo.root, "demo", "fix").is_err(),
        "the branch is kept, so a re-add by the same name is refused"
    );
}

#[test]
fn removing_a_worktree_that_is_already_gone_is_not_an_error() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    remove_worktree(&RealRunner, &repo.root, &wt).expect("runs");
    assert_eq!(
        remove_worktree(&RealRunner, &repo.root, &wt).expect("runs"),
        Removal::Gone
    );
}

#[test]
fn is_ignored_reflects_the_gitignore_in_both_spellings() {
    let repo = Repo::new();
    assert!(!is_ignored(&RealRunner, &repo.root).expect("checks"));
    std::fs::write(repo.root.join(".gitignore"), ".worktrees/\n").expect("write");
    assert!(is_ignored(&RealRunner, &repo.root).expect("checks"));
    std::fs::write(repo.root.join(".gitignore"), ".worktrees\n").expect("write");
    assert!(is_ignored(&RealRunner, &repo.root).expect("checks"));
}

#[test]
fn is_ignored_reads_the_info_exclude_file_too() {
    let repo = Repo::new();
    std::fs::create_dir_all(repo.root.join(".git/info")).expect("mkdir");
    std::fs::write(repo.root.join(".git/info/exclude"), ".worktrees/\n").expect("write");
    assert!(is_ignored(&RealRunner, &repo.root).expect("checks"));
}

#[test]
fn is_ignored_outside_a_repository_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(is_ignored(&RealRunner, dir.path()).is_err());
}

fn kept(repo: &Repo, wt: &Worktree) -> Vec<String> {
    match remove_worktree(&RealRunner, &repo.root, wt).expect("runs") {
        Removal::Kept(reasons) => reasons,
        other => panic!("expected the worktree to be kept, got {other:?}"),
    }
}

#[test]
fn a_commit_on_a_detached_head_blocks_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    raw(&wt.path, &["checkout", "-q", "--detach"]);
    commit(&wt.path, "b.txt", "lost\n");
    let reasons = kept(&repo, &wt);
    assert!(
        reasons.contains(&"its HEAD is not on crook/demo/fix".to_string()),
        "{reasons:?}"
    );
    assert!(
        reasons
            .iter()
            .any(|r| r.contains("its HEAD commit") && r.contains("neither pushed nor merged")),
        "{reasons:?}"
    );
    assert!(wt.path.exists());
}

#[test]
fn a_head_on_another_branch_blocks_removal_even_when_clean_and_merged() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    raw(&wt.path, &["checkout", "-q", "-b", "other"]);
    assert_eq!(kept(&repo, &wt), ["its HEAD is not on crook/demo/fix"]);
}

#[test]
fn a_detached_head_that_is_already_pushed_still_blocks_on_the_branch_name() {
    let repo = Repo::new();
    repo.with_remote();
    let wt = repo.add("fix");
    commit(&wt.path, "b.txt", "two\n");
    raw(&wt.path, &["push", "-q", "origin", "crook/demo/fix"]);
    raw(&wt.path, &["checkout", "-q", "--detach"]);
    assert_eq!(kept(&repo, &wt), ["its HEAD is not on crook/demo/fix"]);
}

#[test]
fn an_operation_in_progress_blocks_removal() {
    for (marker, what, is_dir) in [
        ("rebase-merge", "rebase", true),
        ("rebase-apply", "rebase", true),
        ("MERGE_HEAD", "merge", false),
        ("CHERRY_PICK_HEAD", "cherry-pick", false),
        ("REVERT_HEAD", "revert", false),
        ("BISECT_LOG", "bisect", false),
    ] {
        let repo = Repo::new();
        let wt = repo.add("fix");
        let git_dir = PathBuf::from(raw(&wt.path, &["rev-parse", "--absolute-git-dir"]));
        let at = git_dir.join(marker);
        if is_dir {
            std::fs::create_dir(&at).expect("mkdir");
        } else {
            std::fs::write(&at, "x\n").expect("write");
        }
        let reasons = kept(&repo, &wt);
        assert!(
            reasons.contains(&format!("a {what} is in progress")),
            "{marker}: {reasons:?}"
        );
    }
}

#[test]
fn a_real_conflicted_rebase_blocks_removal() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    commit(&wt.path, "a.txt", "branch\n");
    commit(&repo.root, "a.txt", "main\n");
    let out = Command::new("git")
        .args(["rebase", "main"])
        .current_dir(&wt.path)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(!out.status.success(), "the rebase should conflict");
    let reasons = kept(&repo, &wt);
    assert!(
        reasons.contains(&"a rebase is in progress".to_string()),
        "{reasons:?}"
    );
}

#[test]
fn untracked_files_are_seen_even_with_show_untracked_files_off() {
    let repo = Repo::new();
    raw(&repo.root, &["config", "status.showUntrackedFiles", "no"]);
    let wt = repo.add("fix");
    std::fs::write(wt.path.join("new-work.txt"), "x\n").expect("write");
    assert_eq!(kept(&repo, &wt), ["it has untracked files"]);
    assert!(wt.path.join("new-work.txt").exists());
}

#[test]
fn ignored_files_keep_the_worktree_and_are_named() {
    let repo = Repo::new();
    commit(&repo.root, ".gitignore", "*.env\n");
    let wt = repo.add("fix");
    std::fs::write(wt.path.join("keys.env"), "secret\n").expect("write");
    assert_eq!(kept(&repo, &wt), ["ignored files present: keys.env"]);
    assert!(wt.path.join("keys.env").exists());
}

#[test]
fn many_ignored_files_are_named_three_at_a_time_with_a_count() {
    let repo = Repo::new();
    commit(&repo.root, ".gitignore", "*.env\n");
    let wt = repo.add("fix");
    for i in 1..=5 {
        std::fs::write(wt.path.join(format!("k{i}.env")), "s\n").expect("write");
    }
    assert_eq!(
        kept(&repo, &wt),
        ["ignored files present: k1.env, k2.env, k3.env (+2 more)"]
    );
}

#[test]
fn a_worktree_with_a_missing_git_file_is_not_trusted() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    std::fs::remove_file(wt.path.join(".git")).expect("remove");
    let reasons = kept(&repo, &wt);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("is not a git worktree"), "{reasons:?}");
    assert!(wt.path.exists());
}

/// Runs the real git and remembers every argv.
struct Recording(std::cell::RefCell<Vec<Cmd>>);

impl Runner for Recording {
    fn run(&self, cmd: Cmd, timeout: Duration) -> Result<Output, Error> {
        self.0.borrow_mut().push(cmd.clone());
        RealRunner.run(cmd, timeout)
    }
}

#[test]
fn removal_never_passes_force() {
    let repo = Repo::new();
    let wt = repo.add("fix");
    let runner = Recording(std::cell::RefCell::new(Vec::new()));
    assert_eq!(
        remove_worktree(&runner, &repo.root, &wt).expect("runs"),
        Removal::Removed
    );
    let calls = runner.0.borrow();
    let removes: Vec<_> = calls
        .iter()
        .filter(|c| {
            c.args.first().map(String::as_str) == Some("worktree")
                && c.args.get(1).map(String::as_str) == Some("remove")
        })
        .collect();
    assert_eq!(removes.len(), 1);
    for call in calls.iter() {
        assert!(
            !call
                .args
                .iter()
                .any(|a| a == "--force" || a == "-f" || a == "-ff"),
            "{call:?}"
        );
    }
    assert!(
        !calls.iter().any(|c| c.args.iter().any(|a| a == "branch")),
        "the branch is never touched"
    );
}

#[test]
fn delete_merged_branch_removes_an_empty_branch_and_keeps_one_with_work() {
    let repo = Repo::new();
    let empty = repo.add("empty");
    assert_eq!(
        remove_worktree(&RealRunner, &repo.root, &empty).expect("removes"),
        Removal::Removed
    );
    assert!(delete_merged_branch(&RealRunner, &repo.root, &empty.branch).expect("runs"));
    assert_eq!(raw(&repo.root, &["branch", "--list", "crook/*"]), "");

    let busy = repo.add("busy");
    commit(&busy.path, "b.txt", "work\n");
    // The worktree has to go first, since git will not delete a branch that is checked out.
    raw(
        &repo.root,
        &["worktree", "remove", busy.path.to_str().expect("utf8")],
    );
    assert!(!delete_merged_branch(&RealRunner, &repo.root, &busy.branch).expect("runs"));
    assert_eq!(
        raw(&repo.root, &["branch", "--list", "crook/*"]),
        "crook/demo/busy"
    );
}
