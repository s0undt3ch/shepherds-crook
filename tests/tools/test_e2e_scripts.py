"""The guards of ``scripts/e2e/``: they keep an e2e run inside its own throwaway folders and its own hc core.

Every program the scripts call is a stub on ``PATH`` that logs its argument and environment, so nothing real is
touched: no hc core is started and the real home is never read. The tests run in CI, where there is no hc.
"""

from __future__ import annotations

import os
import json
import re
import shlex
import shutil
import signal
import socket
import stat
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[2]
E2E = REPO_ROOT / "scripts" / "e2e"

# Names a caller's Claude session and hc terminal carry; none of them may reach hc or crook.
LEAKS = {
    "HC_TERMINAL_ID": "t9",
    "HC_TERMINAL_SOCKET": "/real/core.sock",
    "HC_SCOPE": "scope-1",
    "HC_OTHER": "1",
    "CLAUDECODE": "1",
    "CLAUDE_PID": "4242",
    "CLAUDE_EFFORT": "high",
    "CLAUDE_CODE_ENTRYPOINT": "cli",
    "CLAUDE_CODE_EXECPATH": "/real/claude",
    "AI_AGENT": "claude-code_x",
    "GIT_DIR": "/real/repo/.git",
    "GIT_WORK_TREE": "/real/repo",
    "GIT_INDEX_FILE": "/real/index",
    "GIT_CONFIG_COUNT": "1",
    "GIT_CONFIG_KEY_0": "core.hooksPath",
    "GIT_CONFIG_VALUE_0": "/real/hooks",
    "CLAUDE_CODE_SESSION_ID": "abc",
    "CLAUDE_CODE_SESSION_ATTENDED": "1",
    "CLAUDE_CODE_CHILD_NOTES_DIR": "/real/notes",
    "CLAUDE_CODE_MESSAGING_SOCKET": "/real/msg.sock",
    "HERDR_ENV": "1",
    "CROOK_PROJECT": "real",
}
# User settings stay (ADR 0020).
KEPT = {"CLAUDE_CODE_SUBAGENT_MODEL": "sonnet"}

needs_jq = pytest.mark.skipif(shutil.which("jq") is None, reason="jq is not installed")
needs_lsof = pytest.mark.skipif(shutil.which("lsof") is None, reason="lsof is not installed")


def _stub(path: Path, body: str) -> None:
    path.write_text(f"#!/bin/sh\n{body}\n", encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR)


class World:
    """A TMPDIR, stub programs and a run made by setup.sh."""

    def __init__(self, tmp_path: Path, *setup_args: str) -> None:
        self.root = tmp_path
        self.bin = tmp_path / "bin"
        self.bin.mkdir(parents=True)
        self.tmp = tmp_path / "tmp"
        self.tmp.mkdir()
        self.log = tmp_path / "calls.log"
        self.screen = tmp_path / "screen.txt"
        self.get_json = tmp_path / "get.json"
        self.screen.write_text("", encoding="utf-8")
        _stub(
            self.bin / "hc",
            f'echo "hc $*" >> {self.log}\n'
            f"env | sort > {tmp_path}/hc.env\n"
            'case "$1" in\n'
            f"  get) cat {self.get_json} ;;\n"
            f"  read) cat {self.screen} ;;\n"
            f"  keys) echo \"$*\" >> {tmp_path}/keys.log; : > {self.screen} ;;\n"
            "  server) [ \"$2\" = stop ] && { rm -f \"$XDG_RUNTIME_DIR\"/*.sock; exit \"${STOP_RC:-0}\"; } ;;\n"
            "esac",
        )
        self.env = {
            "PATH": os.pathsep.join([str(self.bin), os.environ.get("PATH", "/usr/bin:/bin")]),
            "HOME": str(tmp_path / "home"),
            "TMPDIR": str(self.tmp),
            "GIT_CONFIG_GLOBAL": os.devnull,
            # A mise shim under a scratch HOME finds no trust record; that is mise's business, not a leak.
            **({"MISE_TRUSTED_CONFIG_PATHS": str(Path.home() / ".config" / "mise")} if shutil.which("mise") else {}),
            **LEAKS,
            **KEPT,
        }
        self.setup = self.run("setup.sh", *setup_args)
        self.envf = Path(self.setup.stdout.strip()) if self.setup.returncode == 0 else None
        self.run_dir = self.envf.parent if self.envf else None

    def run(
        self, script: str, *args: str, extra: dict[str, str] | None = None, timeout: int = 60
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(  # noqa: S603
            ["/bin/bash", str(E2E / script), *args],
            env={**self.env, **(extra or {})},
            stdin=subprocess.DEVNULL,
            cwd=self.root,
            capture_output=True,
            text=True,
            check=False,
            timeout=timeout,
        )

    def vars(self) -> dict[str, str]:
        """The run.env values, one name per line (setup.sh writes them with printf %q; none holds a space)."""
        assert self.envf is not None
        pairs = (line.split("=", 1) for line in self.envf.read_text(encoding="utf-8").splitlines())
        return {k: v for k, v in pairs}

    def calls(self) -> list[str]:
        return self.log.read_text(encoding="utf-8").splitlines() if self.log.exists() else []

    def last_env(self) -> dict[str, str]:
        text = (self.root / "hc.env").read_text(encoding="utf-8")
        return dict(line.split("=", 1) for line in text.splitlines() if "=" in line)

    def env_with(self, key: str, value: str) -> Path:
        assert self.envf is not None
        lines = [f"{key}={shlex.quote(value)}" if line.startswith(f"{key}=") else line for line in self.envf.read_text().splitlines()]
        bad = self.root / "bad.env"
        bad.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return bad

    def cleanup(self) -> None:
        """setup.sh makes the socket folder under /tmp, outside tmp_path; remove it."""
        if self.envf is not None and self.envf.exists():
            shutil.rmtree(self.vars()["RT"], ignore_errors=True)


@pytest.fixture
def world(tmp_path: Path) -> Iterator[World]:
    w = World(tmp_path / "w")
    yield w
    w.cleanup()


def _staged(root: Path, name: str = "crook-1.0.0-test") -> Path:
    """A staged archive folder as `stage-archive` makes it, with a crook that logs its environment."""
    folder = root / name
    (folder / "plugin" / ".claude-plugin").mkdir(parents=True)
    (folder / "mod").mkdir()
    (folder / "plugin" / ".claude-plugin" / "plugin.json").write_text('{"name":"crook","version":"0.0.0-dev"}')
    (folder / "mod" / "mod.json").write_text("{}")
    _stub(folder / "crook", f'echo "crook $*" >> {root}/crook.log; env | sort > {root}/crook.env')
    return folder


# ---------------------------------------------------------------------------------------------------------------
# setup.sh


def test_setup_makes_a_run_and_a_short_socket_folder(world: World) -> None:
    assert world.setup.returncode == 0, world.setup.stderr
    v = world.vars()
    run = Path(v["RUN"])
    assert run.parent == world.tmp.resolve() and run.name.startswith("crook-e2e-run.")
    assert v["RT"].startswith(str(Path("/tmp").resolve() / "crook-e2e-rt."))
    assert len(v["RT"]) < 50, "a socket in it must fit the platform's path limit"
    assert v["SESSION"] == "crook-e2e-" + run.name.rsplit(".", 1)[1].lower()
    for name in ["repo", "remote.git", "data", "config", "state", "cache", "herdr-app", "install", "shims", "stubs"]:
        assert (run / name).is_dir(), name
    config = (run / "config" / "crook" / "config.toml").read_text(encoding="utf-8")
    assert 'coordinator_model = "haiku"' in config, "a cheap coordinator, so the default model is never used (Q32)"
    assert (run / "repo" / ".gitignore").read_text(encoding="utf-8").strip() == ".worktrees/"


def test_setup_makes_a_repository_with_a_commit_and_a_pushed_remote(world: World) -> None:
    run = Path(world.vars()["RUN"])
    env = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull}
    head = subprocess.run(["git", "-C", str(run / "repo"), "rev-parse", "HEAD"], capture_output=True, text=True, env=env, check=True)  # noqa: S603, S607
    remote = subprocess.run(["git", "-C", str(run / "remote.git"), "rev-parse", "main"], capture_output=True, text=True, env=env, check=True)  # noqa: S603, S607
    assert head.stdout == remote.stdout, "the remote has main, so the push rule (0023) has something to compare with"


def test_setup_puts_the_stub_herdr_app_in_every_run_and_the_stub_claude_in_tier_2(world: World, tmp_path: Path) -> None:
    run = Path(world.vars()["RUN"])
    assert (run / "stubs" / "herdr-app").is_file() and (run / "stubs" / "claude").is_file()
    t3 = World(tmp_path / "t3", "--tier", "3")
    try:
        r3 = Path(t3.vars()["RUN"])
        assert (r3 / "stubs" / "herdr-app").is_file()
        assert not (r3 / "stubs" / "claude").exists(), "tier 3 uses the real claude"
    finally:
        t3.cleanup()


def test_setup_refuses_a_bad_slug_and_leaves_nothing(tmp_path: Path) -> None:
    w = World(tmp_path / "w", "Bad_Slug")
    try:
        assert w.setup.returncode != 0
        assert "slug" in w.setup.stderr
        assert list(w.tmp.iterdir()) == [], "the refused run's folder was left behind"
        long = w.run("setup.sh", "a" * 17)
        assert long.returncode != 0 and "16" in long.stderr
    finally:
        w.cleanup()


def test_an_exported_git_dir_cannot_make_setup_commit_into_the_callers_repository(tmp_path: Path) -> None:
    decoy = tmp_path / "decoy"
    clean = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    subprocess.run(["git", "init", "-q", str(decoy)], check=True, env=clean)  # noqa: S603, S607
    w = World(tmp_path / "w")  # LEAKS exports GIT_DIR, GIT_WORK_TREE and GIT_INDEX_FILE
    try:
        assert w.setup.returncode == 0, w.setup.stderr
        v = w.vars()
        env = {**w.env, "GIT_DIR": str(decoy / ".git"), "GIT_WORK_TREE": str(decoy)}
        again = subprocess.run(["/bin/bash", str(E2E / "setup.sh")], env=env, cwd=w.root, capture_output=True, text=True, check=False)  # noqa: S603
        assert again.returncode == 0, again.stderr
        other = Path(again.stdout.strip()).parent
        shutil.rmtree(Path(dict(line.split("=", 1) for line in (other / "run.env").read_text().splitlines())["RT"]), ignore_errors=True)
        log = subprocess.run(["git", "-C", str(decoy), "log", "--oneline"], capture_output=True, text=True, check=False, env=clean)  # noqa: S603, S607
        assert log.stdout.strip() == "", "setup.sh committed into the caller's repository"
        head = subprocess.run(["git", "-C", str(other / "repo"), "log", "--oneline"], capture_output=True, text=True, check=True, env=clean)  # noqa: S603, S607
        assert "init" in head.stdout and v["RUN"] != str(other)
    finally:
        w.cleanup()


# ---------------------------------------------------------------------------------------------------------------
# hc.sh and crook.sh: the scrubbed wrappers


def test_hc_runs_in_the_runs_own_environment_without_the_callers_identity(world: World) -> None:
    r = world.run("hc.sh", str(world.envf), "ls", "--all")
    assert r.returncode == 0, r.stderr
    assert world.calls() == ["hc ls --all"]
    seen = world.last_env()
    v = world.vars()
    for name in LEAKS:
        assert name not in seen, f"{name} reached hc"
    assert seen["HC_SESSION"] == v["SESSION"]
    for name in ["XDG_STATE_HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"]:
        assert seen[name].startswith(v["RUN"] + "/"), name
    assert seen["XDG_RUNTIME_DIR"] == v["RT"]
    assert seen["HOME"] == v["RUN"] + "/home"
    assert seen["HERDR_APP_HOME"] == v["RUN"] + "/herdr-app"
    assert seen["CLAUDE_CONFIG_DIR"] == v["RUN"] + "/claude"
    assert seen["CROOK_BIN"] == v["RUN"] + "/shims/crook"
    assert seen["PATH"].startswith(v["RUN"] + "/stubs:"), "the run's stubs come first"
    assert seen["CLAUDE_CODE_SUBAGENT_MODEL"] == "sonnet", "user settings stay (ADR 0020)"
    assert seen["GIT_CONFIG_GLOBAL"] == os.devnull and seen["GIT_CONFIG_NOSYSTEM"] == "1"


def test_a_name_the_fixed_list_lacks_is_still_removed(world: World) -> None:
    """The names come from the live environment, so a prefix match catches what nobody listed."""
    r = world.run("hc.sh", str(world.envf), "ls", extra={"CLAUDE_CODE_CHILD_FUTURE_THING": "1", "HC_FUTURE": "1"})
    assert r.returncode == 0, r.stderr
    seen = world.last_env()
    assert "CLAUDE_CODE_CHILD_FUTURE_THING" not in seen and "HC_FUTURE" not in seen


def test_the_scrub_is_checked_on_the_result_not_the_command(world: World) -> None:
    """A scrub list kept in one string (what zsh does with `-u NAME` flags) removes nothing; the check says so."""
    lib = E2E / "lib.sh"
    script = (
        f'source {lib}; load_run "$1"; E2E_ENV=("-u HC_TERMINAL_SOCKET -u CLAUDECODE" "HC_SESSION=$SESSION"); '
        "e2e_verify_env"
    )
    r = subprocess.run(  # noqa: S603
        ["/bin/bash", "-c", script, "x", str(world.envf)],  # noqa: S607
        env=world.env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert r.returncode != 0, "a one-string list must not pass the check"
    script = f'source {lib}; load_run "$1"; E2E_ENV=(); e2e_verify_env'
    r = subprocess.run(["/bin/bash", "-c", script, "x", str(world.envf)], env=world.env, capture_output=True, text=True, check=False)  # noqa: S603, S607
    assert r.returncode != 0 and "still set after the scrub" in r.stderr, r.stderr


@needs_jq
def test_crook_runs_the_active_version_through_the_fake_shim_with_the_same_environment(world: World, tmp_path: Path) -> None:
    v = world.vars()
    staged = _staged(tmp_path / "staged", "crook-1.0.0-test")
    r = world.run("install.sh", str(world.envf), "1.0.0", extra={"E2E_STAGED": str(staged)})
    assert r.returncode == 0, r.stderr
    assert (Path(v["RUN"]) / "install" / "1.0.0" / "crook").is_file()
    assert Path(v["RUN"], "active").read_text().strip() == "1.0.0"
    r = world.run("crook.sh", str(world.envf), "status", "--json")
    assert r.returncode == 0, r.stderr
    assert (tmp_path / "staged" / "crook.log").read_text().splitlines() == ["crook status --json"]
    seen = dict(
        line.split("=", 1) for line in (tmp_path / "staged" / "crook.env").read_text().splitlines() if "=" in line
    )
    for name in LEAKS:
        assert name not in seen, f"{name} reached crook"
    assert seen["HC_SESSION"] == v["SESSION"] and seen["CROOK_BIN"] == v["RUN"] + "/shims/crook"
    assert seen["XDG_DATA_HOME"].startswith(v["RUN"] + "/")


@needs_jq
def test_the_shim_follows_the_active_file_and_install_stamps_the_plugin_version(world: World, tmp_path: Path) -> None:
    run = Path(world.vars()["RUN"])
    for version in ["1.0.0", "2.0.0"]:
        staged = _staged(tmp_path / f"s{version}", f"crook-{version}-test")
        r = world.run("install.sh", str(world.envf), version, extra={"E2E_STAGED": str(staged)})
        assert r.returncode == 0, r.stderr
    installed = run / "install" / "2.0.0" / "plugin/.claude-plugin/plugin.json"
    assert json.loads(installed.read_text())["version"] == "2.0.0"
    source = tmp_path / "s2.0.0" / "crook-2.0.0-test/plugin/.claude-plugin/plugin.json"
    assert json.loads(source.read_text())["version"] == "0.0.0-dev", "the staged source is not edited"
    again = world.run("install.sh", str(world.envf), "2.0.0", extra={"E2E_STAGED": str(staged)})
    assert again.returncode != 0 and "already installed" in again.stderr
    (run / "active").write_text("1.0.0\n")
    world.run("crook.sh", str(world.envf), "which")
    assert (tmp_path / "s1.0.0" / "crook.log").exists() and not (tmp_path / "s2.0.0" / "crook.log").exists()


def test_crook_refuses_when_nothing_is_installed(world: World) -> None:
    r = world.run("crook.sh", str(world.envf), "status")
    assert r.returncode != 0 and "install" in r.stderr


# ---------------------------------------------------------------------------------------------------------------
# load_run: every script refuses an env file that points outside its run


def test_every_script_refuses_an_env_file_that_points_outside_its_run(world: World, tmp_path: Path) -> None:
    v = world.vars()
    run = v["RUN"]
    cases = [("RUN", value) for value in ["", "/", str(tmp_path), str(world.tmp), f"{run}/../{Path(run).name}"]]
    cases += [("RT", value) for value in ["/tmp", str(tmp_path), "/tmp/other-dir", ""]]
    cases += [(key, str(tmp_path / "real")) for key in ["DATA", "CONFIG", "STATE", "CACHE", "APP", "RUN_HOME", "CLAUDE_DIR"]]
    cases += [("SHIM", "/usr/bin/true"), ("STUBS", "/usr/bin"), ("SESSION", "crook-e2e-other"), ("SESSION", "default")]
    cases += [("SLUG", "Bad Slug"), ("DATA", f"{run}/../escape"), ("REAL_PATH", "")]
    before = len(world.calls())
    for key, value in cases:
        bad = world.env_with(key, value)
        for script, extra in [("hc.sh", ["ls"]), ("crook.sh", ["ls"]), ("teardown.sh", []), ("trust.sh", ["t1"]), ("core_env.sh", [])]:
            r = world.run(script, str(bad), *extra)
            assert r.returncode != 0, (script, key, value)
            assert "e2e:" in r.stderr, (script, key, value, r.stderr)
    assert len(world.calls()) == before, world.calls()[before:]
    assert Path(run).is_dir()


def test_a_missing_env_file_is_refused(world: World) -> None:
    for script in ["hc.sh", "crook.sh", "teardown.sh", "trust.sh", "core_env.sh", "install.sh"]:
        r = world.run(script, str(world.root / "nope.env"))
        assert r.returncode != 0 and "no run env file" in r.stderr, script


# ---------------------------------------------------------------------------------------------------------------
# trust.sh


def _dialog(path: str, kind: str = "workspace") -> str:
    if kind == "workspace":
        return f" Accessing workspace:\n\n {path}\n\n Quick safety check\n"
    return f" {path}\n Claude hasn't worked here before. Add it?\n"


@needs_jq
@pytest.mark.parametrize("kind", ["workspace", "add-dir"])
def test_trust_answers_a_dialog_for_a_path_in_the_run_with_down_enter(world: World, kind: str) -> None:
    run = world.vars()["RUN"]
    world.get_json.write_text(f'{{"cwd":"{run}/data/x"}}')
    world.screen.write_text(_dialog(f"{run}/data/crook/flocks/f", kind), encoding="utf-8")
    r = world.run("trust.sh", str(world.envf), "t1", "30")
    assert r.returncode == 0, r.stderr
    assert (world.root / "keys.log").read_text().splitlines() == ["keys t1 down enter"]
    assert "dialogs answered: 1" in r.stdout


@needs_jq
@pytest.mark.parametrize("kind", ["workspace", "add-dir"])
def test_trust_refuses_a_dialog_that_names_a_path_outside_the_run(world: World, kind: str) -> None:
    run = world.vars()["RUN"]
    world.get_json.write_text(f'{{"cwd":"{run}/data/x"}}')
    world.screen.write_text(_dialog("/Users/someone/real-project", kind), encoding="utf-8")
    r = world.run("trust.sh", str(world.envf), "t1", "30")
    assert r.returncode != 0 and "REFUSED" in r.stderr
    assert not (world.root / "keys.log").exists(), "no key was sent"


@needs_jq
def test_trust_refuses_a_path_that_climbs_out_of_the_run(world: World) -> None:
    run = world.vars()["RUN"]
    world.get_json.write_text(f'{{"cwd":"{run}/data/x"}}')
    world.screen.write_text(_dialog(f"{run}/../../elsewhere"), encoding="utf-8")
    r = world.run("trust.sh", str(world.envf), "t1", "30")
    assert r.returncode != 0 and "REFUSED" in r.stderr
    assert not (world.root / "keys.log").exists()


@needs_jq
def test_trust_refuses_a_terminal_that_runs_outside_the_run(world: World) -> None:
    world.get_json.write_text('{"cwd":"/Users/someone/real-project"}')
    world.screen.write_text(_dialog(f"{world.vars()['RUN']}/x"), encoding="utf-8")
    r = world.run("trust.sh", str(world.envf), "t1", "30")
    assert r.returncode != 0 and "outside the run" in r.stderr
    assert not (world.root / "keys.log").exists()
    assert not any(c.startswith("hc read") for c in world.calls()), "the screen was not even read"


# ---------------------------------------------------------------------------------------------------------------
# teardown.sh


def _socket_in(rt: str) -> socket.socket:
    s = socket.socket(socket.AF_UNIX)
    s.bind(f"{rt}/core.sock")
    return s


@needs_lsof
def test_teardown_stops_the_runs_core_ends_leftovers_and_removes_both_folders(world: World) -> None:
    v = world.vars()
    s = _socket_in(v["RT"])
    proc = subprocess.Popen(["sleep", "300"], cwd=Path(v["RUN"]) / "data")  # noqa: S603, S607
    try:
        r = world.run("teardown.sh", str(world.envf))
        assert r.returncode == 0, r.stderr
        assert "hc server stop" in world.calls()
        assert not Path(v["RUN"]).exists() and not Path(v["RT"]).exists()
        assert proc.wait(timeout=10) == -signal.SIGTERM, "the process in the run was ended"
    finally:
        s.close()
        if proc.poll() is None:
            proc.kill()


@needs_lsof
def test_teardown_leaves_a_process_outside_the_run_alone(world: World, tmp_path: Path) -> None:
    outside = tmp_path / "outside"
    outside.mkdir()
    proc = subprocess.Popen(["sleep", "300"], cwd=outside)  # noqa: S603, S607
    try:
        r = world.run("teardown.sh", str(world.envf))
        assert r.returncode == 0, r.stderr
        assert proc.poll() is None, "a process whose folder is not in the run must not be touched"
    finally:
        proc.kill()
        proc.wait()


@needs_lsof
def test_teardown_keeps_the_run_and_exits_non_zero_when_the_core_will_not_stop(world: World) -> None:
    v = world.vars()
    s = _socket_in(v["RT"])
    try:
        r = world.run("teardown.sh", str(world.envf), extra={"STOP_RC": "1"})
        assert r.returncode != 0 and "kept" in r.stderr
        assert Path(v["RUN"]).is_dir() and Path(v["RT"]).is_dir()
    finally:
        s.close()


@needs_lsof
def test_teardown_keep_stops_everything_and_removes_nothing(world: World) -> None:
    v = world.vars()
    s = _socket_in(v["RT"])
    try:
        r = world.run("teardown.sh", str(world.envf), "--keep")
        assert r.returncode == 0, r.stderr
        assert "hc server stop" in world.calls()
        assert Path(v["RUN"]).is_dir() and Path(v["RT"]).is_dir()
    finally:
        s.close()


@needs_lsof
def test_teardown_does_not_call_hc_when_no_core_runs(world: World) -> None:
    r = world.run("teardown.sh", str(world.envf))
    assert r.returncode == 0, r.stderr
    assert world.calls() == [], "nothing to stop, so hc was not called"


@needs_lsof
def test_kill_in_run_refuses_a_pid_outside_the_run(world: World, tmp_path: Path) -> None:
    proc = subprocess.Popen(["sleep", "300"], cwd=tmp_path)  # noqa: S603, S607
    try:
        script = f'source {E2E / "lib.sh"}; load_run "$1"; kill_in_run "$2"'
        r = subprocess.run(["/bin/bash", "-c", script, "x", str(world.envf), str(proc.pid)], env=world.env, capture_output=True, text=True, check=False)  # noqa: S603, S607
        assert r.returncode != 0 and "outside the run" in r.stderr
        assert proc.poll() is None
        r = subprocess.run(["/bin/bash", "-c", script, "x", str(world.envf), "12; rm -rf /"], env=world.env, capture_output=True, text=True, check=False)  # noqa: S603, S607
        assert r.returncode != 0 and "not a pid" in r.stderr
    finally:
        proc.kill()
        proc.wait()


# ---------------------------------------------------------------------------------------------------------------
# The stubs


def test_the_stub_herdr_app_logs_and_never_reaches_an_app(tmp_path: Path) -> None:
    run = tmp_path / "run"
    run.mkdir()
    r = subprocess.run(  # noqa: S603
        ["/bin/bash", str(E2E / "stubs" / "herdr-app"), "do", "space.rename", '{"id":"a"}'],
        env={"E2E_RUN": str(run)},
        capture_output=True,
        text=True,
        check=False,
    )
    assert r.returncode == 0 and '"ok":true' in r.stdout
    assert "space.rename" in (run / "herdr-app.log").read_text()
    refused = subprocess.run(["/bin/bash", str(E2E / "stubs" / "herdr-app"), "do", "x"], env={}, capture_output=True, text=True, check=False)  # noqa: S603
    assert refused.returncode != 0, "without E2E_RUN it is not running in a run"


@pytest.mark.parametrize(("flag", "source"), [("--session-id", "startup"), ("--resume", "resume")])
def test_the_stub_claude_runs_the_session_start_hook_and_reports_idle(tmp_path: Path, flag: str, source: str) -> None:
    run = tmp_path / "run"
    cwd = tmp_path / "cwd"
    run.mkdir()
    cwd.mkdir()
    fake = tmp_path / "crook"
    _stub(fake, f'echo "$*" > {run}/hook.args; cat > {run}/hook.stdin; env | sort > {run}/hook.env')
    proc = subprocess.run(  # noqa: S603
        ["/bin/bash", str(E2E / "stubs" / "claude"), "--plugin-dir", "/p", "--agent", "crook:child", "--model", "haiku", flag, "sid-1", "the prompt"],
        input="hello\n/exit\n",
        env={"E2E_RUN": str(run), "CROOK_BIN": str(fake), "CLAUDE_CONFIG_DIR": str(tmp_path / "claude"), "PATH": "/usr/bin:/bin"},
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    assert proc.returncode == 0, proc.stderr
    assert "\x1b]7501;:state=idle:app=claude\x07" in proc.stdout
    assert (run / "hook.args").read_text().strip() == "hook session-start"
    hook_env = dict(line.split("=", 1) for line in (run / "hook.env").read_text().splitlines() if "=" in line)
    for name in ["CLAUDECODE", "CLAUDE_PID", "AI_AGENT", "CLAUDE_CODE_SESSION_ID"]:
        assert name in hook_env, f"a real session exports {name} to its hooks"
    assert hook_env["CLAUDE_PLUGIN_ROOT"] == "/p"
    assert "HC_TERMINAL_SOCKET" not in hook_env, "the harness never injects the core's socket"
    names = next((run / "claude-calls").glob("*.env")).read_text().splitlines()
    assert all("=" not in n for n in names), "the env log holds names, never values"
    hook = (run / "hook.stdin").read_text()
    assert '"session_id":"sid-1"' in hook and f'"source":"{source}"' in hook
    argv = next((run / "claude-calls").glob("*.argv")).read_text().split("\x1f")
    assert argv[:-1] == ["--plugin-dir", "/p", "--agent", "crook:child", "--model", "haiku", flag, "sid-1", "the prompt"]
    assert (run / "claude-calls" / "sid-1.input").read_text() == "hello\n/exit\n"
    assert list((tmp_path / "claude" / "projects").glob("*/sid-1.jsonl")), "a transcript where a session writes one"


def test_the_stub_claude_swallows_a_prompt_that_follows_add_dir_like_the_real_cli(tmp_path: Path) -> None:
    run = tmp_path / "run"
    run.mkdir()
    base = ["/bin/bash", str(E2E / "stubs" / "claude"), "--agent", "crook:coordinator", "--session-id", "s1"]
    env = {"E2E_RUN": str(run), "PATH": "/usr/bin:/bin"}
    bad = subprocess.run([*base, "--add-dir", "/a", "--add-dir", "/b", "the prompt"], input="", env=env, capture_output=True, text=True, check=False, timeout=30)  # noqa: S603
    assert "no prompt reached" in bad.stderr and list((run / "claude-calls").glob("*.noprompt"))
    shutil.rmtree(run / "claude-calls")
    good = subprocess.run([*base, "--add-dir", "/a", "--model", "haiku", "the prompt"], input="", env=env, capture_output=True, text=True, check=False, timeout=30)  # noqa: S603
    assert "no prompt" not in good.stderr and not list((run / "claude-calls").glob("*.noprompt"))


def test_the_stub_herdr_app_can_be_told_to_fail(tmp_path: Path) -> None:
    (tmp_path / "herdr-app.fail").write_text("")
    r = subprocess.run(["/bin/bash", str(E2E / "stubs" / "herdr-app"), "do", "x"], env={"E2E_RUN": str(tmp_path)}, capture_output=True, text=True, check=False)  # noqa: S603
    assert r.returncode == 1 and "failing on purpose" in r.stderr


def test_the_stub_claude_reports_the_state_a_harness_writes(tmp_path: Path) -> None:
    run = tmp_path / "run"
    (run / "claude-calls").mkdir(parents=True)
    (run / "claude-calls" / "sid-2.state").write_text("working")
    proc = subprocess.run(  # noqa: S603
        ["/bin/bash", str(E2E / "stubs" / "claude"), "--session-id", "sid-2"],
        input="",
        env={"E2E_RUN": str(run), "PATH": "/usr/bin:/bin"},
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    assert "state=idle" in proc.stdout
    # stdin closed at once, so the loop ends before it reads the file; a second line proves the file is read.
    proc = subprocess.run(  # noqa: S603
        ["/bin/bash", "-c", f'(sleep 2; echo /exit) | /bin/bash {E2E / "stubs" / "claude"} --session-id sid-2'],
        env={"E2E_RUN": str(run), "PATH": "/usr/bin:/bin"},
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    assert "state=working" in proc.stdout


# ---------------------------------------------------------------------------------------------------------------
# Static checks on the scripts themselves


def _scripts() -> list[Path]:
    return sorted([*E2E.glob("*.sh"), *(E2E / "stubs").iterdir()])


def _code(path: Path) -> list[str]:
    return [line for line in path.read_text(encoding="utf-8").splitlines() if not line.lstrip().startswith("#")]


def test_every_script_has_a_bash_shebang_and_no_script_runs_zsh() -> None:
    for path in _scripts():
        first = path.read_text(encoding="utf-8").splitlines()[0]
        assert first in {"#!/usr/bin/env bash", "#!/bin/bash"} or path.name == "lib.sh", (path, first)
        assert "zsh" not in "\n".join(_code(path)), path


def test_no_script_calls_hc_or_crook_except_through_the_wrappers() -> None:
    """`hc` and `crook` are shell functions that call hc.sh and crook.sh; nothing else may start either program."""
    direct = re.compile(r"(\b(command|exec|env|run_timeout)\s+(-\S+\s+)*|\bxargs\s+)(\S*/)?(hc|crook)\b")
    absolute = re.compile(r"(^|[\s;&|(`\"'=])/[\w./+-]*/(hc|crook)\s")
    for path in _scripts():
        if path.name in {"hc.sh", "crook.sh"}:
            continue
        text = "\n".join(_code(path))
        # install.sh only writes the shim's text, which runs the installed crook.
        assert path.name == "install.sh" or not direct.search(text), f"{path.name} starts hc or crook directly"
        assert not absolute.search(text), f"{path.name} starts hc or crook by an absolute path"
        for name, wrapper in [("hc", "hc.sh"), ("crook", "crook.sh")]:
            for line in _code(path):
                if re.match(rf"\s*{name}\(\)", line):
                    assert wrapper in line, f"{path.name}: {name}() must call {wrapper}"
        assert '"$SHIM"' not in text or path.name in {"install.sh"}, f"{path.name} runs the shim directly"


def test_the_wrappers_are_the_only_places_hc_and_crook_are_exec_ed() -> None:
    assert re.search(r'exec env .*\bhc "\$@"', (E2E / "hc.sh").read_text(encoding="utf-8"))
    assert re.search(r'exec env .*"\$SHIM" "\$@"', (E2E / "crook.sh").read_text(encoding="utf-8"))
    for name in ["hc.sh", "crook.sh"]:
        text = (E2E / name).read_text(encoding="utf-8")
        assert "e2e_verify_env" in text, f"{name} must check the environment before it runs"
        assert text.index("e2e_verify_env") < text.index("exec env")


def test_lib_removes_every_name_the_binarys_own_scrub_removes() -> None:
    """crook's `scrub` (src/hc/scrub.rs) and the harness's `removed_name` must name the same variables."""
    scrub = (REPO_ROOT / "src" / "hc" / "scrub.rs").read_text(encoding="utf-8")
    exact = re.search(r"const REMOVED: &\[&str\] = &\[(.*?)\];", scrub, re.S)
    prefixes = re.search(r"const REMOVED_PREFIXES: &\[&str\] = &\[(.*?)\];", scrub, re.S)
    assert exact and prefixes
    lib = "\n".join(_code(E2E / "lib.sh"))  # names in comments do not count
    for name in re.findall(r'"([A-Z_]+)"', exact.group(1)):
        assert re.search(rf"\b{name}\b", lib), f"lib.sh does not remove {name}"
    for prefix in re.findall(r'"([A-Z_]+_)"', prefixes.group(1)):
        assert f"{prefix}*" in lib, f"lib.sh does not remove {prefix}*"
