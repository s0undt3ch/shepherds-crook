"""Tests for CI's dev versions (D124), ported from ToolR's `tools/version.py`."""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

import pytest
from toolr.testing import make_context
from toolr.types import ResolvedPath

from tools.release import PLUGIN_MANIFEST
from tools.release import TODAY_VERSION
from tools.release import bump
from tools.release import compute_dev_version

GIT = ("git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false")


@pytest.fixture(autouse=True)
def _not_a_pull_request(monkeypatch: pytest.MonkeyPatch) -> None:
    """CI sets GITHUB_EVENT_NAME; only the pull-request test may see one."""
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)


def _git(repo: Path, *args: str) -> str:
    done = subprocess.run([*GIT, "-C", str(repo), *args], check=True, capture_output=True, text=True)  # noqa: S603
    return done.stdout.strip()


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    path = tmp_path / "repo"
    path.mkdir()
    _git(path, "init", "-q", "-b", "main")
    return path


def _commits(repo: Path, n: int) -> None:
    for _ in range(n):
        _git(repo, "commit", "-q", "--allow-empty", "-m", "fix: x")


def test_with_no_tag_the_base_is_the_dated_fallback_and_every_commit_counts(repo: Path) -> None:
    _commits(repo, 3)
    major, minor, _ = TODAY_VERSION.split(".")
    assert compute_dev_version(repo) == f"{major}.{minor}.1-dev3"


def test_after_a_final_tag_the_patch_goes_up(repo: Path) -> None:
    _commits(repo, 2)
    _git(repo, "tag", "v0.1.0")
    _commits(repo, 4)
    assert compute_dev_version(repo) == "0.1.1-dev4"


def test_after_a_release_candidate_the_candidate_goes_up(repo: Path) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v0.1.0-rc.1")
    _commits(repo, 2)
    assert compute_dev_version(repo) == "0.1.0-rc.2.dev2"


def test_a_pull_request_adds_the_short_sha(repo: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v0.1.0")
    _commits(repo, 1)
    monkeypatch.setenv("GITHUB_EVENT_NAME", "pull_request")
    sha = _git(repo, "rev-parse", "--short", "HEAD")
    assert compute_dev_version(repo) == f"0.1.1-dev1+g{sha}"


def test_a_tag_that_is_not_a_release_fails(repo: Path) -> None:
    _commits(repo, 1)
    _git(repo, "tag", "v1.2.3.dev1")
    with pytest.raises(ValueError, match="not a release version"):
        compute_dev_version(repo)


@pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo is not installed")
def test_bump_with_no_version_stamps_the_dev_version(repo: Path, tmp_path: Path) -> None:
    (repo / "src").mkdir()
    (repo / "src" / "lib.rs").write_text("")
    (repo / "Cargo.toml").write_text('[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2021"\n')
    subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=repo, check=True, capture_output=True)  # noqa: S607
    plugin = tmp_path / "plugin.json"
    shutil.copy(PLUGIN_MANIFEST, plugin)
    _git(repo, "add", ".")
    _commits(repo, 1)
    _git(repo, "tag", "v0.1.0")
    _commits(repo, 2)
    ctx = make_context(repo)
    bump(ctx, manifest=ResolvedPath(repo / "Cargo.toml"), plugin_manifest=ResolvedPath(plugin))
    assert ctx.stdout.strip().splitlines()[-1] == "0.1.1-dev2"
    assert json.loads(plugin.read_text())["version"] == "0.1.1-dev2"
