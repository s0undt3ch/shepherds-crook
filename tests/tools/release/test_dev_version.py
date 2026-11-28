"""Tests for `toolr release dev-version` and `bump --dev` (D124): ToolR's dev versions, in crook's SemVer form."""

from __future__ import annotations

import subprocess
from pathlib import Path

import pytest
from toolr.testing import make_context
from toolr.types import ResolvedPath

from tools.release import compute_dev_version
from tools.release import dev_version

GIT = ("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false")


def _git(repo: Path, *args: str) -> None:
    subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True)  # noqa: S603


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    path = tmp_path / "repo"
    path.mkdir()
    _git(path, "init", "-q", "-b", "main")
    return path


def _commits(repo: Path, n: int) -> None:
    for _ in range(n):
        _git(repo, "commit", "-q", "--allow-empty", "-m", "fix: x")


def test_before_any_tag_the_base_is_0_0_0_and_every_commit_counts(repo: Path) -> None:
    _commits(repo, 3)
    assert compute_dev_version(repo) == "0.0.1-dev3"


def test_after_a_final_tag_the_patch_goes_up_and_the_count_restarts(repo: Path) -> None:
    _commits(repo, 2)
    _git(repo, "tag", "v0.1.0")
    _commits(repo, 4)
    assert compute_dev_version(repo) == "0.1.1-dev4"


def test_after_a_release_candidate_the_candidate_goes_up(repo: Path) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v0.1.0-rc.1")
    _commits(repo, 2)
    assert compute_dev_version(repo) == "0.1.0-rc.2.dev2"


def test_on_the_tag_itself_the_count_is_zero(repo: Path) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v1.2.3")
    assert compute_dev_version(repo) == "1.2.4-dev0"


def test_a_sha_is_added_as_build_metadata(repo: Path) -> None:
    _commits(repo, 1)
    assert compute_dev_version(repo, "abc1234") == "0.0.1-dev1+gabc1234"


def test_a_tag_that_is_not_a_release_tag_fails(repo: Path) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v1.2.3-beta")
    with pytest.raises(RuntimeError, match="not a release tag"):
        compute_dev_version(repo)


@pytest.mark.parametrize("sha", ["xyz", "ABC1234", "abc"])
def test_the_command_refuses_a_bad_sha(tmp_path: Path, repo: Path, sha: str) -> None:
    _commits(repo, 1)
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit):
        dev_version(ctx, sha=sha, repo=ResolvedPath(repo))


def test_this_repository_has_a_dev_version(tmp_path: Path) -> None:
    ctx = make_context(tmp_path)
    dev_version(ctx)
    assert "-dev" in ctx.stdout.strip()
