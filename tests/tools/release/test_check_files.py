"""Tests for the release's owned-files check, `.github/actions/apply-release-patch/check-files.sh`."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[3]
SCRIPT = REPO_ROOT / ".github" / "actions" / "apply-release-patch" / "check-files.sh"
OWNED = ("Cargo.toml", "Cargo.lock", "plugin/.claude-plugin/plugin.json", "CHANGELOG.md")
GIT = ("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false")


def _git(repo: Path, *args: str) -> None:
    subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True)  # noqa: S603


def _write(repo: Path, rel: str, text: str) -> None:
    path = repo / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _check(repo: Path, mode: str, *allowed: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603
        [str(SCRIPT), mode, *allowed], cwd=repo, capture_output=True, text=True, check=False
    )


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    _git(tmp_path, "init", "-q", "-b", "main")
    for rel in (*OWNED, "src/main.rs", ".pre-commit-config.yaml"):
        _write(tmp_path, rel, "a\n")
    _git(tmp_path, "add", ".")
    _git(tmp_path, "commit", "-q", "-m", "base")
    return tmp_path


def _commit(repo: Path, *rels: str) -> None:
    for rel in rels:
        _write(repo, rel, "b\n")
    _git(repo, "add", *rels)
    _git(repo, "commit", "-q", "-m", "change")


def test_a_commit_of_owned_files_passes(repo: Path) -> None:
    _commit(repo, "Cargo.toml", "Cargo.lock", "plugin/.claude-plugin/plugin.json")
    assert _check(repo, "commit", *OWNED).returncode == 0


@pytest.mark.parametrize("extra", ["src/main.rs", ".pre-commit-config.yaml", ".github/workflows/ci.yml"])
def test_a_commit_that_touches_anything_else_fails_and_names_it(repo: Path, extra: str) -> None:
    _commit(repo, "Cargo.toml", extra)
    done = _check(repo, "commit", *OWNED)
    assert done.returncode == 1
    assert extra in done.stderr
    assert "Cargo.toml" not in done.stderr.replace("::error::The release changed files it does not own:", "")


def test_a_deleted_file_counts(repo: Path) -> None:
    _git(repo, "rm", "-q", "src/main.rs")
    _git(repo, "commit", "-q", "-m", "gone")
    assert _check(repo, "commit", *OWNED).returncode == 1


def test_an_empty_entry_does_not_allow_everything(repo: Path) -> None:
    _commit(repo, "src/main.rs")
    assert _check(repo, "commit", "", "Cargo.toml").returncode == 1


def test_no_allowed_file_at_all_is_an_error(repo: Path) -> None:
    _commit(repo, "Cargo.toml")
    assert _check(repo, "commit", "").returncode == 2


def test_the_worktree_mode_sees_staged_and_unstaged_changes(repo: Path) -> None:
    _write(repo, "Cargo.toml", "b\n")
    _write(repo, "src/main.rs", "b\n")
    _git(repo, "add", "src/main.rs")
    done = _check(repo, "worktree", *OWNED)
    assert done.returncode == 1
    assert "src/main.rs" in done.stderr


def test_the_worktree_mode_passes_on_owned_changes_only(repo: Path) -> None:
    _write(repo, "Cargo.lock", "b\n")
    _write(repo, "untracked.txt", "x\n")
    assert _check(repo, "worktree", *OWNED).returncode == 0


def test_a_bad_mode_is_a_usage_error(repo: Path) -> None:
    assert _check(repo, "everything", *OWNED).returncode == 2
