"""Tests for `toolr release next-version` (D124)."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
from toolr.testing import make_context
from toolr.types import ResolvedPath

from tools.release import next_version

GIT = ("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false")


def _git(repo: Path, *args: str) -> None:
    subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True)  # noqa: S603


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    path = tmp_path / "repo"
    path.mkdir()
    _git(path, "init", "-q", "-b", "main")
    return path


def _commit(repo: Path, message: str) -> None:
    name = f"f{len(list(repo.iterdir()))}"
    (repo / name).write_text(message)
    _git(repo, "add", name)
    _git(repo, "commit", "-q", "-m", message)


def _next(tmp_path: Path, repo: Path) -> str:
    ctx = make_context(tmp_path)
    next_version(ctx, repo=ResolvedPath(repo))
    return ctx.stdout.strip()


def test_before_any_final_tag_the_first_release_is_0_1_0(tmp_path: Path, repo: Path) -> None:
    _commit(repo, "feat: a")
    assert _next(tmp_path, repo) == "0.1.0"


@pytest.mark.parametrize(("message", "want"), [("fix: b", "0.1.1"), ("feat: b", "0.2.0")])
def test_after_a_final_tag_the_commits_set_the_bump(tmp_path: Path, repo: Path, message: str, want: str) -> None:
    _commit(repo, "feat: a")
    _git(repo, "tag", "v0.1.0")
    _commit(repo, message)
    assert _next(tmp_path, repo) == want


def test_a_pre_release_tag_is_not_a_base(tmp_path: Path, repo: Path) -> None:
    _commit(repo, "feat: a")
    _git(repo, "tag", "v0.1.0-rc.1")
    _commit(repo, "fix: b")
    assert _next(tmp_path, repo) == "0.1.0"


def test_this_repository_has_a_next_version(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    next_version(ctx)
    assert ctx.stdout.strip().count(".") == 2
