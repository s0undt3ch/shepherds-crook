"""Tests for ``toolr release bump`` in ``tools/release.py``.

The version validation and the ``cargo set-version`` call run against a stubbed
``ctx.run``. One test runs the real ``cargo set-version`` on a temp crate, so it
proves ``Cargo.toml`` and ``Cargo.lock`` move together.
"""

from __future__ import annotations

import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest

from toolr.testing import make_command_result
from toolr.testing import make_context
from toolr.types import ResolvedPath
from toolr.utils.command import CommandResult

from tools.release import REPO_ROOT
from tools.release import bump

CARGO_TOML = """\
[package]
name = "demo"
version = "0.1.0"
edition = "2024"
"""

needs_cargo_edit = pytest.mark.skipif(
    shutil.which("cargo-set-version") is None, reason="cargo-edit is not installed"
)


@pytest.fixture
def crate(tmp_path: Path) -> Path:
    """A minimal crate with a lock file, so `cargo set-version` has both files to move."""
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "lib.rs").write_text("")
    (tmp_path / "Cargo.toml").write_text(CARGO_TOML, encoding="utf-8")
    subprocess.run(  # noqa: S603
        ["cargo", "generate-lockfile", "--offline"],  # noqa: S607
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )
    return tmp_path


def _version_of(crate: Path) -> str:
    return tomllib.loads((crate / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]


def _locked_version(crate: Path) -> str:
    lock = tomllib.loads((crate / "Cargo.lock").read_text(encoding="utf-8"))
    return next(p["version"] for p in lock["package"] if p["name"] == "demo")


@needs_cargo_edit
@pytest.mark.parametrize("version", ["1.2.3", "1.0.0-rc.1", "10.20.30-rc.12"])
def test_bump_updates_cargo_toml_and_cargo_lock(crate: Path, version: str) -> None:
    ctx = make_context(crate)
    bump(ctx, version, manifest=ResolvedPath(crate / "Cargo.toml"))
    assert _version_of(crate) == version
    assert _locked_version(crate) == version


@pytest.mark.parametrize(
    "version",
    [
        "1.2",
        "v1.2.3",
        "1.2.3.4",
        "1.2.3rc1",
        "1.2.3-rc1",
        "1.2.3-beta.1",
        "1.2.3-rc.",
        "01.2.3",
        "",
        "latest",
    ],
)
def test_bump_refuses_anything_but_x_y_z_or_x_y_z_rc_n(version: str, tmp_path: Path) -> None:
    calls: list[tuple[str, ...]] = []

    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return make_command_result(args=list(cmdline))

    ctx = make_context(tmp_path, run=_run)
    with pytest.raises(SystemExit) as exc:
        bump(ctx, version)
    assert exc.value.code == 1
    assert f"{version!r}" in ctx.stderr
    assert calls == []


def test_bump_runs_cargo_set_version_on_the_repo_manifest(tmp_path: Path) -> None:
    calls: list[tuple[str, ...]] = []

    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return make_command_result(args=list(cmdline))

    ctx = make_context(tmp_path, run=_run)
    bump(ctx, "0.2.0")
    assert calls == [
        ("cargo", "set-version", "--manifest-path", str(REPO_ROOT / "Cargo.toml"), "0.2.0")
    ]


def test_bump_passes_cargo_failure_through(tmp_path: Path) -> None:
    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        return make_command_result(args=list(cmdline), returncode=3)

    ctx = make_context(tmp_path, run=_run)
    with pytest.raises(SystemExit) as exc:
        bump(ctx, "0.2.0")
    assert exc.value.code == 3
    assert "cargo set-version failed" in ctx.stderr
