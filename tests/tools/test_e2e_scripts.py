"""The scripts/e2e/ guards that keep a driven e2e run inside its own throwaway directory.

herdr, claude and crook are stubs on PATH that log their argv, so nothing real is touched.
"""

from __future__ import annotations

import json
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
    """A TMPDIR, logging stubs and a run made by setup.sh.

    The herdr stub answers from files: `pane get` with pane.json, `pane read` with screen.txt,
    `pane list` with panes.json and `workspace list` with workspaces.json. A `send-keys ... enter`
    replaces the screen with a session's status line, as accepting a dialog does.
    """

    def __init__(self, tmp_path: Path, *setup_args: str) -> None:
        self.root = tmp_path
        tmp_path.mkdir(parents=True, exist_ok=True)
        self.bin = tmp_path / "bin"
        self.bin.mkdir()
        self.tmp = tmp_path / "tmp"
        self.tmp.mkdir()
        self.log = tmp_path / "calls.log"
        for name, value in [
            ("screen.txt", ""),
            ("pane.json", '{"result":{"pane":{"cwd":"/elsewhere"}}}'),
            ("panes.json", '{"result":{"panes":[]}}'),
            ("workspaces.json", '{"result":{"workspaces":[]}}'),
        ]:
            (tmp_path / name).write_text(value, encoding="utf-8")
        _stub(
            self.bin / "crook",
            f'echo "crook $* CLAUDECODE=$CLAUDECODE CROOK_CHILD=$CROOK_CHILD CONFIG=$XDG_CONFIG_HOME" >> {self.log}',
        )
        _stub(self.bin / "claude", f'echo "claude $*" >> {self.log}; echo "[]"')
        _stub(
            self.bin / "herdr",
            f'echo "herdr $*" >> {self.log}\n'
            'case "$*" in\n'
            f"  'pane get'*) cat {tmp_path}/pane.json;;\n"
            f"  'pane read'*) cat {tmp_path}/screen.txt;;\n"
            f"  'pane list'*) cat {tmp_path}/panes.json;;\n"
            f"  'workspace list'*) cat {tmp_path}/workspaces.json;;\n"
            f"  'pane send-keys'*enter*) echo '⚠ crook: 0 waiting on you' > {tmp_path}/screen.txt;;\n"
            "esac",
        )
        self.env = {
            "PATH": os.pathsep.join([str(self.bin), "/usr/bin", "/bin", "/usr/sbin"]),
            "HOME": str(tmp_path / "home"),
            "TMPDIR": str(self.tmp),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "CROOK": str(self.bin / "crook"),
        }
        self.setup = self.run("setup.sh", *setup_args)
        self.envf = Path(self.setup.stdout.strip()) if self.setup.returncode == 0 else None
        self.run_dir = self.envf.parent if self.envf else None

    def write(self, name: str, value: object) -> None:
        text = value if isinstance(value, str) else json.dumps(value)
        (self.root / name).write_text(text, encoding="utf-8")

    def pane_at(self, cwd: Path | str) -> None:
        self.write("pane.json", {"result": {"pane": {"cwd": str(cwd)}}})

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

    def env_with(self, key: str, value: str) -> Path:
        assert self.envf is not None
        lines = [f"{key}={value}" if line.startswith(f"{key}=") else line for line in self.envf.read_text().splitlines()]
        bad = self.root / "bad.env"
        bad.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return bad


def test_setup_makes_a_unique_valid_slug(tmp_path: Path) -> None:
    a = World(tmp_path / "a")
    b = World(tmp_path / "b")
    assert a.setup.returncode == 0, a.setup.stderr
    slugs = [line for w in (a, b) for line in w.envf.read_text().splitlines() if line.startswith("SLUG=")]
    assert len(set(slugs)) == 2, slugs
    for line in slugs:
        assert line.removeprefix("SLUG=").isalnum() and line.removeprefix("SLUG=").islower(), line


def test_setup_refuses_a_slug_whose_workspace_exists(tmp_path: Path) -> None:
    w = World(tmp_path, "taken")  # the first setup creates the stubs; a second one reuses them
    w.write("workspaces.json", {"result": {"workspaces": [{"label": "crook-dup", "workspace_id": "w1"}]}})
    r = w.run("setup.sh", "dup")
    assert r.returncode != 0
    assert "crook-dup already exists" in r.stderr
    assert len(list(w.tmp.iterdir())) == 1, "the refused run's directory was left behind"


def test_crook_runs_without_the_callers_identity_or_config(tmp_path: Path) -> None:
    w = World(tmp_path)
    r = w.run("crook.sh", str(w.envf), "list", extra={"CLAUDECODE": "1", "CROOK_CHILD": "real"})
    assert r.returncode == 0, r.stderr
    assert w.calls()[-1] == f"crook list CLAUDECODE= CROOK_CHILD= CONFIG={w.run_dir}/config"


def test_every_script_refuses_an_env_file_that_points_outside_its_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    before = len(w.calls())
    cases = [("RUN", v) for v in ["", "/", str(tmp_path), str(w.tmp), f"{w.run_dir}/../{w.run_dir.name}"]]
    cases += [(k, str(tmp_path / "real")) for k in ["S", "D", "C", "R", "N"]]
    cases += [("SLUG", "'Bad Slug'"), ("S", f"{w.run_dir}/../escape")]
    for key, value in cases:
        bad = w.env_with(key, value)
        for script in ["teardown.sh", "crook.sh", "db.sh"]:
            r = w.run(script, str(bad), "select 1")
            assert r.returncode != 0, (script, key, value)
            assert f"e2e: {key}" in r.stderr, (script, key, value, r.stderr)
    assert len(w.calls()) == before, w.calls()[before:]
    assert w.run_dir.is_dir()


def test_db_refuses_dot_commands(tmp_path: Path) -> None:
    w = World(tmp_path)
    r = w.run("db.sh", str(w.envf), ".shell id")
    assert r.returncode != 0 and "dot-commands" in r.stderr


def test_pane_helpers_refuse_a_pane_outside_the_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    w.pane_at("/Users/someone")
    for script, args in [
        ("say.sh", ["w9:p1", "hello"]),
        ("keys.sh", ["w9:p1", "esc"]),
        ("run_in.sh", ["w9:p1", "list"]),
        ("trust.sh", ["w9:p1"]),
    ]:
        r = w.run(script, str(w.envf), *args)
        assert r.returncode == 1, script
        assert "not inside the run" in r.stderr, (script, r.stderr)
    assert not any(k in c for c in w.calls() for k in ("send-text", "send-keys", "pane run")), w.calls()


def test_kill_refuses_a_process_outside_the_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    r = w.run("kill_in_run.sh", str(w.envf), str(os.getpid()))
    assert r.returncode == 1 and "outside the run" in r.stderr, r.stderr


def test_trust_refuses_paths_outside_its_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    w.pane_at(w.run_dir / "repo")
    sibling = w.tmp.resolve() / "crook-e2e-run.other"
    for path in [f"{w.run_dir}/../../etc", str(sibling), "/Users/someone/project"]:
        w.write("screen.txt", f"Accessing workspace:\n {path}\n❯ No, exit\n  Yes, I trust this folder\n")
        r = w.run("trust.sh", str(w.envf), "w1:p1")
        assert r.returncode == 1, (path, r.stdout, r.stderr)
        assert "REFUSED: not inside the run" in r.stderr, (path, r.stderr)
    assert not any("send-keys" in c for c in w.calls()), w.calls()


def test_trust_accepts_both_dialogs_inside_its_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    w.pane_at(w.run_dir / "repo")
    for dialog, answer in [
        ("Accessing workspace:\n {path}\n", "Yes, I trust this folder"),
        (" Adding a new directory:\n {path}\n This session hasn't worked here before.\n", "Yes, add it"),
    ]:
        path = w.run_dir / "notes"
        w.write("screen.txt", dialog.format(path=path) + f"❯ {answer}\n")
        r = w.run("trust.sh", str(w.envf), "w1:p1")
        assert r.returncode == 0, r.stderr
        assert f"accepted ({answer}): {path}" in r.stdout, r.stdout


def test_teardown_closes_only_workspaces_wholly_inside_the_run(tmp_path: Path) -> None:
    w = World(tmp_path)
    inside, outside = str(w.run_dir / "repo"), "/Users/someone"
    w.write(
        "panes.json",
        {
            "result": {
                "panes": [
                    {"workspace_id": "wRun", "cwd": inside},
                    {"workspace_id": "wRun", "cwd": str(w.run_dir / "data")},
                    {"workspace_id": "wMixed", "cwd": inside},
                    {"workspace_id": "wMixed", "cwd": outside},
                    {"workspace_id": "wUser", "cwd": outside},
                ]
            }
        },
    )
    r = w.run("teardown.sh", str(w.envf))
    assert r.returncode == 0, r.stderr
    closes = [c for c in w.calls() if c.startswith("herdr workspace close")]
    assert closes == ["herdr workspace close wRun"], closes
    assert not w.run_dir.exists()
