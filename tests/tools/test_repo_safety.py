"""Workflow and script settings that keep a public repo safe to contribute to and to run.

They are configuration, not code `tools/` runs, so these tests read the files as written.
"""

from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path
from typing import Any

from ruamel.yaml import YAML

REPO_ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = REPO_ROOT / ".github" / "workflows"


def _steps(workflow: str) -> list[dict[str, Any]]:
    doc = YAML(typ="safe").load((WORKFLOWS / workflow).read_text(encoding="utf-8"))
    return [step for job in doc["jobs"].values() for step in job.get("steps", [])]


def _step_using(workflow: str, action: str) -> dict[str, Any]:
    found = [s for s in _steps(workflow) if str(s.get("uses", "")).startswith(f"{action}@")]
    assert len(found) == 1, found
    return found[0]


def test_attest_is_skipped_on_pull_requests_from_forks() -> None:
    """A fork's pull request gets no OIDC token: attesting would fail its CI."""
    step = _step_using("_build-binary-archive.yml", "actions/attest")
    cond = " ".join(str(step.get("if", "")).split())
    assert "github.event_name != 'pull_request'" in cond, cond
    assert "github.event.pull_request.head.repo.full_name == github.repository" in cond, cond


def test_release_admin_check_fails_the_job() -> None:
    """Without `error-if-missing` the action only sets an output nothing reads."""
    step = _step_using("release.yml", "actions-cool/check-user-permission")
    assert step["with"]["require"] == "admin"
    assert str(step["with"].get("error-if-missing")).lower() == "true", step


def _stub(path: Path, body: str) -> None:
    path.write_text(f"#!/bin/sh\n{body}\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


def test_e2e_never_inherits_the_users_xdg_dirs(tmp_path: Path) -> None:
    """The e2e run creates and wipes a project: it must run on fresh dirs only."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "crook.log"
    _stub(bin_dir / "crook", f'echo "$XDG_STATE_HOME $XDG_DATA_HOME" >> {log}')
    real_state = tmp_path / "real-state"
    real_data = tmp_path / "real-data"
    scratch = tmp_path / "tmp"
    scratch.mkdir()
    env = {
        "PATH": f"{bin_dir}{os.pathsep}/usr/bin{os.pathsep}/bin",
        "HOME": str(tmp_path / "home"),
        "TMPDIR": str(scratch),
        "XDG_STATE_HOME": str(real_state),
        "XDG_DATA_HOME": str(real_data),
        "GIT_CONFIG_GLOBAL": os.devnull,
        "CROOK": str(bin_dir / "crook"),
    }
    out = subprocess.run(
        ["bash", str(REPO_ROOT / "scripts" / "e2e" / "setup.sh")],
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    run = Path(out.strip()).parent
    assert run.parent == scratch.resolve(), out
    seen = log.read_text(encoding="utf-8").split()
    assert seen, "setup.sh never ran crook"
    for path in seen:
        assert Path(path).parent == run, seen
    assert not real_state.exists() and not real_data.exists()
