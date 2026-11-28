"""Tests for ``toolr release changelog-range`` in ``tools/release.py``, on throwaway git repos."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest

from toolr.testing import make_context
from toolr.types import ResolvedPath

from tools.release import CHANGELOG_BASE
from tools.release import changelog_range
from tools.release import final_tags

GIT = ("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false")


def _git(repo: Path, *args: str) -> None:
    subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True)  # noqa: S603


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    _git(tmp_path, "init", "-q", "-b", "main")
    return tmp_path


def _commit(repo: Path, name: str) -> None:
    (repo / name).write_text(name)
    _git(repo, "add", name)
    _git(repo, "commit", "-q", "-m", f"feat: {name}")


def _range(tmp_path: Path, repo: Path) -> str:
    ctx = make_context(tmp_path)
    changelog_range(ctx, repo=ResolvedPath(repo))
    return ctx.stdout.strip()


def test_no_tags_reads_from_after_the_v1_base(tmp_path: Path, repo: Path) -> None:
    _commit(repo, "a")
    assert _range(tmp_path, repo) == f"{CHANGELOG_BASE}..HEAD"


@pytest.mark.parametrize("tag", ["v0.1.0-rc.1", "v0.1.0-rc.2", "v1", "v1.2", "vx.y.z", "v01.2.3"])
def test_pre_release_and_odd_tags_do_not_end_the_range(tmp_path: Path, repo: Path, tag: str) -> None:
    _commit(repo, "a")
    _git(repo, "tag", "-a", "-m", "t", tag)
    _commit(repo, "b")
    assert _range(tmp_path, repo) == f"{CHANGELOG_BASE}..HEAD"


def test_a_final_tag_leaves_the_range_empty(tmp_path: Path, repo: Path) -> None:
    _commit(repo, "a")
    _git(repo, "tag", "-a", "-m", "t", "v0.1.0")
    _commit(repo, "b")
    assert _range(tmp_path, repo) == ""


def test_a_final_tag_on_another_branch_does_not_count(tmp_path: Path, repo: Path) -> None:
    _commit(repo, "a")
    _git(repo, "switch", "-q", "-c", "side")
    _commit(repo, "b")
    _git(repo, "tag", "-a", "-m", "t", "v0.1.0")
    _git(repo, "switch", "-q", "main")
    _commit(repo, "c")
    assert final_tags(repo) == []
    assert _range(tmp_path, repo) == f"{CHANGELOG_BASE}..HEAD"


def test_a_folder_that_is_not_a_repo_fails(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        changelog_range(ctx, repo=ResolvedPath(tmp_path / "nope"))
    assert exc.value.code == 1
