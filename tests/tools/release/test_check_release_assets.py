"""Tests for the release's exact-file-set check, `scripts/check-release-assets.sh`."""

from __future__ import annotations

import hashlib
import subprocess
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[3] / "scripts" / "check-release-assets.sh"
TRIPLES = ("x86_64-unknown-linux-musl", "aarch64-apple-darwin")
VERSION = "0.1.0-rc.1"


def _check(folder: Path, count: int = 2, version: str = VERSION) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603
        [str(SCRIPT), str(folder), version, str(count)], capture_output=True, text=True, check=False
    )


@pytest.fixture
def assets(tmp_path: Path) -> Path:
    folder = tmp_path / "assets"
    folder.mkdir()
    for triple in TRIPLES:
        name = f"crook-{VERSION}-{triple}.tar.gz"
        body = triple.encode()
        (folder / name).write_bytes(body)
        (folder / f"{name}.sha256").write_text(f"{hashlib.sha256(body).hexdigest()}  {name}\n")
    return folder


def test_the_exact_set_passes(assets: Path) -> None:
    assert _check(assets).returncode == 0


def test_a_missing_archive_fails(assets: Path) -> None:
    done = _check(assets, count=3)
    assert done.returncode == 1
    assert "expected 3 archives" in done.stderr


def test_a_missing_sidecar_fails(assets: Path) -> None:
    next(assets.glob("*.sha256")).unlink()
    assert "sha256 is missing" in _check(assets).stderr


@pytest.mark.parametrize("extra", ["notes.md", "crook-0.1.0-rc.1-extra.txt", ".hidden"])
def test_an_extra_file_fails(assets: Path, extra: str) -> None:
    (assets / extra).write_text("x")
    done = _check(assets)
    assert done.returncode == 1
    assert "unexpected file" in done.stderr


def test_an_archive_of_another_version_fails(assets: Path) -> None:
    assert _check(assets, version="0.2.0").returncode == 1


def test_a_changed_archive_fails(assets: Path) -> None:
    next(assets.glob("*.tar.gz")).write_bytes(b"tampered")
    done = _check(assets)
    assert done.returncode == 1
    assert "does not match" in done.stderr


def test_a_bad_count_fails(assets: Path) -> None:
    assert _check(assets, count=0).returncode == 1
