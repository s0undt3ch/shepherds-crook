"""The ToolR pins in this repo are one release.

`mise.toml` installs the binary for local use, `.github/env` names the binary CI installs,
the setup-toolr action's `uses:` comment names the action release, and `tools/uv.lock` pins the
`toolr-py` runtime the tools venv imports. Renovate's "toolr release" group bumps them together;
these tests fail when they drift.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest
from ruamel.yaml import YAML

from tools.ci import REPO_ROOT

GITHUB_DIR = REPO_ROOT / ".github"
ACTION = GITHUB_DIR / "actions" / "setup-toolr" / "action.yml"
USES = re.compile(r"^\s*uses:\s*s0undt3ch/ToolR@(?P<ref>\S+)\s*#\s*v(?P<version>\S+)\s*$", re.M)


def _action_steps() -> list[dict[str, Any]]:
    return YAML(typ="safe").load(ACTION.read_text(encoding="utf-8"))["runs"]["steps"]


def _action_version_comment() -> str:
    found = USES.findall(ACTION.read_text(encoding="utf-8"))
    assert len(found) == 1, f"{ACTION} needs exactly one `uses: s0undt3ch/ToolR@<sha> # vX.Y.Z` step"
    return found[0][1]


def test_toolr_pins_agree() -> None:
    env = (GITHUB_DIR / "env").read_text(encoding="utf-8").splitlines()
    pinned = [line.removeprefix("TOOLR_VERSION=") for line in env if line.startswith("TOOLR_VERSION=")]
    assert pinned, "add `TOOLR_VERSION=<version>` to .github/env"
    mise = tomllib.loads((REPO_ROOT / "mise.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((REPO_ROOT / "tools" / "uv.lock").read_text(encoding="utf-8"))
    locked = [p["version"] for p in lock["package"] if p["name"] == "toolr-py"]
    assert locked, "tools/uv.lock has no toolr-py package"
    pins = {
        ".github/env TOOLR_VERSION": pinned[0],
        "mise.toml aqua:s0undt3ch/ToolR": mise["tools"]["aqua:s0undt3ch/ToolR"],
        "setup-toolr action comment": _action_version_comment(),
        "tools/uv.lock toolr-py": locked[0],
    }
    assert len(set(pins.values())) == 1, f"set these to the same version: {pins}"


def test_the_action_ref_is_a_full_sha() -> None:
    (ref, _version) = USES.findall(ACTION.read_text(encoding="utf-8"))[0]
    assert re.fullmatch(r"[0-9a-f]{40}", ref), f"pin s0undt3ch/ToolR to a commit SHA, got {ref}"


def test_the_action_passes_the_pin_through() -> None:
    calls = [s for s in _action_steps() if str(s.get("uses", "")).startswith("s0undt3ch/ToolR@")]
    assert len(calls) == 1
    assert calls[0]["with"]["version"] == "${{ steps.pin.outputs.version }}"


def test_every_workflow_goes_through_the_wrapper() -> None:
    offences = []
    calls = 0
    for path in sorted((GITHUB_DIR / "workflows").glob("*.yml")):
        text = path.read_text(encoding="utf-8")
        calls += text.count("uses: ./.github/actions/setup-toolr")
        offences.extend(
            f"{path.name}: use `./.github/actions/setup-toolr`, not {m}"
            for m in re.findall(r"uses:\s*(s0undt3ch/ToolR@\S+)", text)
        )
    assert not offences, "\n".join(offences)
    assert calls, "no workflow calls ./.github/actions/setup-toolr"


def _pin_script() -> str:
    scripts = [s["run"] for s in _action_steps() if ".github/env" in (s.get("run") or "")]
    assert len(scripts) == 1, "the action needs exactly one step that reads TOOLR_VERSION"
    return scripts[0]


@pytest.mark.parametrize(
    ("env_file", "expected"),
    [
        ("TOOLR_VERSION=9.9.9-test\n", "9.9.9-test"),
        ("TOOLR_VERSION=\n", None),
        ("MISE_VERSION=v1.0.0\n", None),
    ],
    ids=["set", "empty", "missing"],
)
def test_the_action_reads_the_pin_and_fails_without_one(
    tmp_path: Path, env_file: str, expected: str | None
) -> None:
    bash = shutil.which("bash")
    if bash is None:
        pytest.skip("bash not available; the action only runs on ubuntu runners")
    (tmp_path / ".github").mkdir()
    (tmp_path / ".github" / "env").write_text(env_file, encoding="utf-8")
    github_output = tmp_path / "github_output"
    github_output.touch()
    result = subprocess.run(  # noqa: S603
        [bash, "-e", "-c", _pin_script()],
        cwd=tmp_path,
        env={**os.environ, "GITHUB_OUTPUT": str(github_output)},
        capture_output=True,
        text=True,
        check=False,
    )
    if expected is None:
        assert result.returncode != 0, result
        assert "::error" in result.stdout, result
    else:
        assert result.returncode == 0, result
        assert github_output.read_text(encoding="utf-8") == f"version={expected}\n"
