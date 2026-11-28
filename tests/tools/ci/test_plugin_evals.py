"""Checks on the plugin's eval fixtures in ``plugin/evals/``. They run no model."""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import pytest

ROOT = Path(__file__).resolve().parents[3]
EVALS = ROOT / "plugin" / "evals"
STATUS_SCHEMA = json.loads((ROOT / "schema" / "status.schema.json").read_text(encoding="utf-8"))
CASES = sorted(p.parent.name for p in EVALS.glob("*/case.yaml"))
SAFETY_WEIGHT = 20
RUNS = 3
# Graders that guard a rule: one run failing any of them must fail the case.
SAFETY = {
    "never-hc-send",
    "never-git-worktree",
    "never-push",
    "no-resume",
    "no-hc-call",
    "no-ask-user-question",
    "no-bare-crook",
    "uses-literal-path",
    "no-shim-form",
    "no-compound-crook",
    "no-bash-file-io",
    "skill-invoked",
    "spawn-call-with-model",
    "spawn-call-with-repo-and-model",
    "log-no-duplicate-step",
    "committed",
    "remote-untouched",
    "status-failed",
    "no-invented-total",
}


def _value(text: str) -> Any:
    text = text.strip()
    if text.startswith("{"):
        return {k: _value(v) for k, v in re.findall(r"(\w+): ('(?:[^']|'')*'|\S+?)(?:,| \})", text)}
    if text.startswith("'"):
        return text[1:-1].replace("''", "'")
    if text in ("True", "False"):
        return text == "True"
    return int(text) if re.fullmatch(r"-?\d+", text) else text


def _graders(case: str) -> list[dict[str, Any]]:
    """The graders of a generated case.yaml: `  - name: x` followed by `    key: value` lines."""
    text = (EVALS / case / "case.yaml").read_text(encoding="utf-8")
    graders: list[dict[str, Any]] = []
    for line in text.split("\ngraders:\n", 1)[1].splitlines():
        if line.startswith("  - name: "):
            graders.append({"name": line.removeprefix("  - name: ")})
        elif line.startswith("    "):
            key, _, value = line.strip().partition(": ")
            graders[-1][key] = _value(value)
    return graders


def _install_env(home: Path) -> dict[str, str]:
    return {"PATH": os.environ["PATH"], "HOME": str(home), "TMPDIR": tempfile.gettempdir()}


def _install(tmp_path: Path) -> tuple[Path, Path]:
    """Run the shared scaffold step in a scratch workspace and home, as the eval harness does."""
    home = tmp_path / "home"
    workspace = home / "cwd"
    workspace.mkdir(parents=True)
    subprocess.run(  # noqa: S603
        ["bash", str(EVALS / "_lib" / "install.sh")], cwd=workspace, env=_install_env(home), check=True
    )
    return home, workspace


def _call(home: Path, workspace: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    shim = home / ".local/share/mise/shims/crook"
    return subprocess.run(  # noqa: S603
        [str(shim), *args], cwd=workspace, env=_install_env(home), check=check, capture_output=True, text=True
    )


def test_there_are_cases_for_both_skills() -> None:
    assert [c for c in CASES if c.startswith("child-")]
    assert [c for c in CASES if c.startswith("coord-")]


@pytest.mark.parametrize("case", CASES)
def test_each_case_has_an_executable_scaffold_and_graders(case: str) -> None:
    folder = EVALS / case
    assert os.access(folder / "scaffold.sh", os.X_OK)
    text = (folder / "case.yaml").read_text(encoding="utf-8")
    assert f"name: {case}\n" in text
    assert "scaffold_script: scaffold.sh" in text
    assert _graders(case)


@pytest.mark.parametrize("case", CASES)
def test_every_grader_regex_compiles(case: str) -> None:
    for grader in _graders(case):
        for key in ("pattern", "input_match"):
            if key in grader:
                # JavaScript syntax; the patterns used here are in the common subset with Python's.
                re.compile(grader[key])


@pytest.mark.parametrize("case", CASES)
def test_every_graded_file_is_made_by_the_scaffold_or_the_model(case: str) -> None:
    folder = EVALS / case
    made = (
        (folder / "scaffold.sh").read_text(encoding="utf-8")
        + (EVALS / "_lib" / "install.sh").read_text(encoding="utf-8")
        + (folder / "case.yaml").read_text(encoding="utf-8").split("\ngraders:\n", 1)[0]
    )
    for grader in _graders(case):
        target = grader.get("target")
        path = target["path"] if isinstance(target, dict) else grader.get("path") if grader["type"] == "file_exists" else None
        if path is None or grader.get("exists") is False:
            continue
        if path.startswith("briefs/") and case.startswith("coord-"):
            continue  # the coordinator writes its briefs
        if path == ".git/logs/HEAD":
            assert "git init" in made
        elif path == ".crook-calls.log":
            assert path in (EVALS / "_lib" / "install.sh").read_text(encoding="utf-8")
        else:
            assert path in made, f"{case}: {grader['name']} reads {path}, which nothing creates"


def test_safety_graders_gate_every_case() -> None:
    """One run failing a safety grader fails the case; one run losing every other grader does not.

    A run scores the weighted share of graders that passed and a case scores the mean of its runs
    (the docs page `plugin-evals.md`), so with threshold t the case passes only if the score is at least t.
    """
    threshold = float(re.search(r"--threshold ([\d.]+)", (EVALS / "run.sh").read_text(encoding="utf-8"))[1])
    for case in CASES:
        graders = _graders(case)
        for grader in graders:
            expected = SAFETY_WEIGHT if grader["name"] in SAFETY else 1
            assert grader.get("weight", 1) == expected, f"{case}: {grader['name']} has the wrong weight"
        total = sum(g.get("weight", 1) for g in graders)
        others = sum(g.get("weight", 1) for g in graders if g["name"] not in SAFETY)
        one_safety_failure = 1 - SAFETY_WEIGHT / (RUNS * total)
        every_other_failing_once = 1 - others / (RUNS * total)
        assert one_safety_failure < threshold, f"{case}: a safety failure scores {one_safety_failure:.4f}"
        assert every_other_failing_once >= threshold, f"{case}: a flake scores {every_other_failing_once:.4f}"


def test_the_stub_logs_arguments_and_prints_a_valid_status(tmp_path: Path) -> None:
    home, workspace = _install(tmp_path)
    status = json.loads(_call(home, workspace, "status", "demo", "--json").stdout)
    assert status["version"] == STATUS_SCHEMA["properties"]["version"]["const"]
    assert set(STATUS_SCHEMA["required"]) <= set(status)
    (flock,) = status["flocks"]
    assert set(STATUS_SCHEMA["$defs"]["flock"]["required"]) <= set(flock)
    assert flock["folder"] == str(workspace.resolve())
    assert flock["state"] in STATUS_SCHEMA["$defs"]["state"]["enum"]
    assert set(STATUS_SCHEMA["$defs"]["flock"]["properties"]["coordinator"]["required"]) <= set(flock["coordinator"])
    assert flock["children"] == []

    (workspace / "briefs" / "survey.md").write_text("## Goal\n", encoding="utf-8")
    brief = str(workspace.resolve() / "briefs" / "survey.md")
    out = _call(home, workspace, "spawn", "child", "survey", brief, "--model", "haiku").stdout
    assert out.startswith("spawned demo.survey (terminal ")
    log = (workspace / ".crook-calls.log").read_text(encoding="utf-8").splitlines()
    assert log == ["status demo --json", f"spawn child survey {brief} --model haiku"]


def test_the_stub_refuses_a_brief_outside_briefs_or_missing(tmp_path: Path) -> None:
    home, workspace = _install(tmp_path)
    (workspace / "elsewhere.md").write_text("x", encoding="utf-8")
    for brief in (str(workspace.resolve() / "elsewhere.md"), str(workspace.resolve() / "briefs" / "none.md"), "x.md"):
        done = _call(home, workspace, "spawn", "child", "a", brief, check=False)
        assert done.returncode == 1


def test_the_stub_reports_the_children_a_case_scaffolds(tmp_path: Path) -> None:
    home, workspace = _install(tmp_path)
    (workspace / ".stub-children.json").write_text('[{"name":"survey"}]', encoding="utf-8")
    status = json.loads(_call(home, workspace, "status", "demo", "--json").stdout)
    assert status["flocks"][0]["children"] == [{"name": "survey"}]


def test_the_stub_tells_only_a_coordinator_its_literal_path(tmp_path: Path) -> None:
    home, workspace = _install(tmp_path)
    assert _call(home, workspace, "hook", "session-start").stdout == ""
    (workspace / ".stub-coordinator").write_text("", encoding="utf-8")
    out = json.loads(_call(home, workspace, "hook", "session-start").stdout)["hookSpecificOutput"]
    shim = home / ".local/share/mise/shims/crook"
    assert out["hookEventName"] == "SessionStart"
    assert out["additionalContext"].startswith(f'crook: Run every crook command as "{shim}" <subcommand>')
    assert _call(home, workspace, "hook", "stop").stdout == ""


@pytest.mark.parametrize("case", CASES)
def test_only_coordinator_cases_mark_a_coordinator(case: str) -> None:
    marked = ".stub-coordinator" in (EVALS / case / "scaffold.sh").read_text(encoding="utf-8")
    assert marked == case.startswith("coord-")


@pytest.mark.parametrize("case", [c for c in CASES if (EVALS / c / "scaffold.sh").read_text().count("stub-children")])
def test_a_scaffolded_child_has_every_key_the_schema_requires(case: str, tmp_path: Path) -> None:
    home = tmp_path / "home"
    workspace = home / "cwd"
    workspace.mkdir(parents=True)
    subprocess.run(["bash", str(EVALS / case / "scaffold.sh")], cwd=workspace, env=_install_env(home), check=True)  # noqa: S603, S607
    status = json.loads(_call(home, workspace, "status", "demo", "--json").stdout)
    child_schema = STATUS_SCHEMA["$defs"]["child"]
    for child in status["flocks"][0]["children"]:
        assert set(child_schema["required"]) <= set(child)
        assert set(child_schema["properties"]["result"]["required"]) <= set(child["result"])


def test_the_stub_never_calls_hc() -> None:
    lines = [
        line
        for line in (EVALS / "_lib" / "crook-stub.sh").read_text(encoding="utf-8").splitlines()
        if not line.lstrip().startswith("#")
    ]
    assert not [line for line in lines if " hc " in f" {line} " or "herdr" in line]


def test_install_refuses_a_home_outside_a_temporary_folder() -> None:
    # A stand-in "real" home inside the repository, which is not under a temporary folder.
    fake = Path(tempfile.mkdtemp(dir=ROOT, prefix=".fake-home-"))
    try:
        target = fake / "mise-real-crook"
        target.write_text("the real shim", encoding="utf-8")
        shim = fake / ".local/share/mise/shims/crook"
        shim.parent.mkdir(parents=True)
        shim.symlink_to(target)
        workspace = Path(tempfile.mkdtemp())
        try:
            done = subprocess.run(  # noqa: S603
                ["bash", str(EVALS / "_lib" / "install.sh")],  # noqa: S607
                cwd=workspace,
                env=_install_env(fake),
                capture_output=True,
                text=True,
                check=False,
            )
        finally:
            shutil.rmtree(workspace)
        assert done.returncode != 0
        assert "refusing" in done.stderr
        assert target.read_text(encoding="utf-8") == "the real shim"
        assert shim.is_symlink()
    finally:
        shutil.rmtree(fake)


def test_install_refuses_a_workspace_outside_a_temporary_folder(tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir()
    workspace = Path(tempfile.mkdtemp(dir=ROOT, prefix=".fake-cwd-"))
    try:
        done = subprocess.run(  # noqa: S603
            ["bash", str(EVALS / "_lib" / "install.sh")],  # noqa: S607
            cwd=workspace,
            env=_install_env(home),
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        shutil.rmtree(workspace)
    assert done.returncode != 0
    assert not (home / ".local").exists()


def test_install_never_writes_through_a_symlink(tmp_path: Path) -> None:
    home = tmp_path / "home"
    workspace = home / "cwd"
    workspace.mkdir(parents=True)
    target = tmp_path / "real-shim"
    target.write_text("the real shim", encoding="utf-8")
    shim = home / ".local/share/mise/shims/crook"
    shim.parent.mkdir(parents=True)
    shim.symlink_to(target)
    subprocess.run(  # noqa: S603
        ["bash", str(EVALS / "_lib" / "install.sh")], cwd=workspace, env=_install_env(home), check=True
    )
    assert target.read_text(encoding="utf-8") == "the real shim"
    assert not shim.is_symlink()
    assert "crook-calls.log" in shim.read_text(encoding="utf-8")


def test_install_refuses_unusual_path_characters(tmp_path: Path) -> None:
    home = tmp_path / "home"
    workspace = home / "c&w d"
    workspace.mkdir(parents=True)
    done = subprocess.run(  # noqa: S603
        ["bash", str(EVALS / "_lib" / "install.sh")],  # noqa: S607
        cwd=workspace,
        env=_install_env(home),
        capture_output=True,
        text=True,
        check=False,
    )
    assert done.returncode != 0
    assert not (home / ".local").exists()
