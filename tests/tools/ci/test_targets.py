"""Tests for ``toolr ci targets`` in ``tools/ci.py``."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import targets

TARGETS = [
    {"triple": "x86_64-unknown-linux-musl", "runner": "ubuntu-latest"},
    {"triple": "aarch64-unknown-linux-musl", "runner": "ubuntu-24.04-arm"},
    {"triple": "aarch64-apple-darwin", "runner": "macos-15"},
    {"triple": "x86_64-apple-darwin", "runner": "macos-15-intel"},
]
PR_TARGETS = TARGETS[:3]


@pytest.fixture(autouse=True)
def _no_event(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("GITHUB_EVENT_NAME", raising=False)


@pytest.mark.parametrize(
    ("event", "expected"),
    [
        ("pull_request", PR_TARGETS),
        ("push", TARGETS),
        ("workflow_dispatch", TARGETS),
    ],
)
def test_pull_requests_skip_slow_targets(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    event: str,
    expected: list[dict[str, str]],
) -> None:
    monkeypatch.delenv("GITHUB_OUTPUT", raising=False)
    monkeypatch.setenv("GITHUB_EVENT_NAME", event)
    ctx = make_context(tmp_path)
    targets(ctx)
    assert json.loads(ctx.stdout) == expected


def test_targets_prints_and_writes_step_output(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    output = tmp_path / "github_output"
    output.write_text("earlier=1\n")
    monkeypatch.setenv("GITHUB_OUTPUT", str(output))
    # Narrow, so a rich line wrap would split the JSON.
    ctx = make_context(tmp_path, width=20)
    targets(ctx)
    printed = ctx.stdout
    assert printed.count("\n") == 1
    assert json.loads(printed) == TARGETS
    earlier, line = output.read_text().splitlines()
    assert earlier == "earlier=1"
    key, _, value = line.partition("=")
    assert key == "targets"
    assert json.loads(value) == TARGETS


def test_targets_without_github_output(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("GITHUB_OUTPUT", raising=False)
    ctx = make_context(tmp_path)
    targets(ctx)
    assert json.loads(ctx.stdout) == TARGETS
