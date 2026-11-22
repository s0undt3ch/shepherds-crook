"""The scripts/e2e/ guards that keep a driven e2e run inside its own throwaway directory.

herdr, claude and crook are stubs on PATH that log their argv, so nothing real is touched.
"""

from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
E2E = REPO_ROOT / "scripts" / "e2e"


def _stub(path: Path, body: str) -> None:
    path.write_text(f"#!/bin/sh\n{body}\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


class World:
    """A TMPDIR, logging stubs and a run made by setup.sh."""

    def __init__(self, tmp_path: Path, screen: str = "") -> None:
        self.bin = tmp_path / "bin"
        self.bin.mkdir()
        self.tmp = tmp_path / "tmp"
        self.tmp.mkdir()
        self.log = tmp_path / "calls.log"
        self.screen = tmp_path / "screen.txt"
        self.screen.write_text(screen, encoding="utf-8")
        _stub(self.bin / "crook", f'echo "crook $* CLAUDECODE=$CLAUDECODE CROOK_CHILD=$CROOK_CHILD" >> {self.log}')
        _stub(self.bin / "claude", f'echo "claude $*" >> {self.log}; echo "[]"')
        self.env = {
            "PATH": f"{self.bin}{os.pathsep}/usr/bin{os.pathsep}/bin{os.pathsep}/opt/homebrew/bin",
            "HOME": str(tmp_path / "home"),
            "TMPDIR": str(self.tmp),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "CROOK": str(self.bin / "crook"),
        }
        self.envf = Path(self.run("setup.sh").stdout.strip())
        self.run_dir = self.envf.parent
        # herdr: every pane sits in the run's repo, and `pane read` prints the screen file.
        _stub(
            self.bin / "herdr",
            f'echo "herdr $*" >> {self.log}\n'
            f'case "$*" in\n'
            f"  'pane get'*) printf '%s' '{{\"result\":{{\"pane\":{{\"cwd\":\"{self.run_dir}/repo\"}}}}}}';;\n"
            f"  'pane read'*) cat {self.screen};;\n"
            f"  'pane list'*) printf '%s' '{{\"result\":{{\"panes\":[]}}}}';;\n"
            "esac",
        )

    def run(self, script: str, *args: str, extra: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(E2E / script), *args],
            env={**self.env, **(extra or {})},
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
        )

    def calls(self) -> list[str]:
        return self.log.read_text(encoding="utf-8").splitlines() if self.log.exists() else []


def test_crook_runs_without_the_callers_identity(tmp_path: Path) -> None:
    w = World(tmp_path)
    r = w.run("crook.sh", str(w.envf), "list", extra={"CLAUDECODE": "1", "CROOK_CHILD": "real"})
    assert r.returncode == 0, r.stderr
    assert w.calls()[-1] == "crook list CLAUDECODE= CROOK_CHILD="


def test_teardown_refuses_an_env_file_that_points_outside_a_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    before = len(w.calls())
    for run in ["", "/", str(tmp_path), str(w.tmp), f"{w.run_dir}/../{w.run_dir.name}"]:
        bad = tmp_path / "bad.env"
        bad.write_text(w.envf.read_text(encoding="utf-8").replace(f"RUN={w.run_dir}", f"RUN={run}"), encoding="utf-8")
        r = w.run("teardown.sh", str(bad))
        assert r.returncode != 0, run
        assert "e2e: RUN" in r.stderr, (run, r.stderr)
    assert len(w.calls()) == before, w.calls()[before:]
    assert w.run_dir.is_dir() and tmp_path.is_dir()


def test_trust_refuses_paths_outside_its_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    sibling = w.tmp.resolve() / "crook-e2e-run.other"
    for path in [f"{w.run_dir}/../../etc", str(sibling), "/Users/someone/project"]:
        w.screen.write_text(f"Accessing workspace:\n {path}\n❯ No, exit\n  Yes, I trust this folder\n", encoding="utf-8")
        r = w.run("trust.sh", str(w.envf), "w1:p1")
        assert r.returncode == 1, (path, r.stdout, r.stderr)
        assert "REFUSED" in r.stderr, (path, r.stderr)
    assert not any("send-keys" in c for c in w.calls()), w.calls()


def test_say_refuses_a_pane_outside_the_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    _stub(w.bin / "herdr", f'echo "herdr $*" >> {w.log}; printf \'%s\' \'{{"result":{{"pane":{{"cwd":"/Users/someone"}}}}}}\'')
    r = w.run("say.sh", str(w.envf), "w9:p1", "hello")
    assert r.returncode == 1
    assert not any("send-text" in c for c in w.calls()), w.calls()
