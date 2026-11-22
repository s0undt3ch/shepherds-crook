"""Tests for ``toolr ci targets`` in ``tools/ci.py``."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import TARGETS
from tools.ci import targets
from tools.ci import targets_json


def test_ci_targets_are_a_subset_of_release() -> None:
    ci = json.loads(targets_json(release=False))
    release = json.loads(targets_json(release=True))
    assert ci == [
        {"triple": "x86_64-unknown-linux-musl", "runner": "ubuntu-latest"},
        {"triple": "aarch64-apple-darwin", "runner": "macos-15"},
    ]
    assert len(release) == len(TARGETS) == 4
    assert all(t in release for t in ci)


def test_triples_are_unique() -> None:
    triples = [t.triple for t in TARGETS]
    assert len(triples) == len(set(triples))


@pytest.mark.parametrize("release", [False, True])
def test_targets_writes_step_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, release: bool
) -> None:
    output = tmp_path / "github_output"
    output.write_text("earlier=1\n")
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    targets(make_context(tmp_path), release=release)
    assert output.read_text() == f"earlier=1\ntargets={targets_json(release)}\n"


def test_targets_without_github_output(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("GITHUB_OUTPUT", raising=False)
    targets(make_context(tmp_path), release=False)
