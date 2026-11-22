"""Tests for ``toolr ci targets`` in ``tools/ci.py``."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import targets

CI = [
    {"triple": "x86_64-unknown-linux-musl", "runner": "ubuntu-latest"},
    {"triple": "aarch64-apple-darwin", "runner": "macos-15"},
]
RELEASE = [
    {"triple": "x86_64-unknown-linux-musl", "runner": "ubuntu-latest"},
    {"triple": "aarch64-unknown-linux-musl", "runner": "ubuntu-24.04-arm"},
    {"triple": "aarch64-apple-darwin", "runner": "macos-15"},
    {"triple": "x86_64-apple-darwin", "runner": "macos-15-intel"},
]


@pytest.mark.parametrize(("release", "expected"), [(False, CI), (True, RELEASE)])
def test_targets_prints_and_writes_step_output(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
    release: bool,
    expected: list[dict[str, str]],
) -> None:
    output = tmp_path / "github_output"
    output.write_text("earlier=1\n")
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    targets(make_context(tmp_path), release=release)
    printed = capsys.readouterr().out
    assert printed.count("\n") == 1
    assert json.loads(printed) == expected
    earlier, line = output.read_text().splitlines()
    assert earlier == "earlier=1"
    key, _, value = line.partition("=")
    assert key == "targets"
    assert json.loads(value) == expected


def test_targets_without_github_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    monkeypatch.delenv("GITHUB_OUTPUT", raising=False)
    targets(make_context(tmp_path), release=False)
    assert json.loads(capsys.readouterr().out) == CI
