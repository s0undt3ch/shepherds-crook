"""Tests for ``toolr ci plugin-check``: good and deliberately broken fixture plugins."""

from __future__ import annotations

import json
import shutil
import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.ci import SHIM
from tools.ci import check_plugin
from tools.ci import newest_release_tag
from tools.ci import plugin_check

REPO_ROOT = Path(__file__).resolve().parents[3]

Edit = Callable[[Path], None]


@pytest.fixture
def plugin(tmp_path: Path) -> Path:
    """A copy of the real plugin tree, as `<tmp>/plugin`, for a test to break."""
    dest = tmp_path / "plugin"
    shutil.copytree(REPO_ROOT / "plugin", dest)
    return dest


def _hooks(plugin: Path) -> dict:
    return json.loads((plugin / "hooks" / "hooks.json").read_text(encoding="utf-8"))


def _write_hooks(plugin: Path, data: dict) -> None:
    (plugin / "hooks" / "hooks.json").write_text(json.dumps(data), encoding="utf-8")


def _replace(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    assert old in text
    path.write_text(text.replace(old, new), encoding="utf-8")


def test_the_real_plugin_passes() -> None:
    assert check_plugin(REPO_ROOT / "plugin") == []


def test_a_missing_plugin_folder_fails(tmp_path: Path) -> None:
    assert check_plugin(tmp_path / "nope")


def test_plugin_json_that_does_not_parse_fails(plugin: Path) -> None:
    (plugin / ".claude-plugin" / "plugin.json").write_text("{", encoding="utf-8")
    assert any("not valid JSON" in e for e in check_plugin(plugin))


def test_a_renamed_plugin_fails(plugin: Path) -> None:
    _replace(plugin / ".claude-plugin" / "plugin.json", '"name": "crook"', '"name": "other"')
    assert any("name must be 'crook'" in e for e in check_plugin(plugin))


def test_hooks_json_that_does_not_parse_fails(plugin: Path) -> None:
    (plugin / "hooks" / "hooks.json").write_text("[", encoding="utf-8")
    assert any("not valid JSON" in e for e in check_plugin(plugin))


def test_a_bare_crook_in_a_hook_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["Stop"][0]["hooks"][0]["command"] = "crook hook stop"
    _write_hooks(plugin, data)
    assert any("Stop command must be exactly" in e for e in check_plugin(plugin))


def test_a_shim_without_the_crook_bin_override_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["Stop"][0]["hooks"][0]["command"] = f"{SHIM.replace('CROOK_BIN:-', 'X:-')} hook stop"
    _write_hooks(plugin, data)
    assert any("Stop command must be exactly" in e for e in check_plugin(plugin))


def test_the_wrong_subcommand_for_an_event_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["Stop"][0]["hooks"][0]["command"] = f"{SHIM} hook session-start"
    _write_hooks(plugin, data)
    assert any("Stop command must be exactly" in e for e in check_plugin(plugin))


@pytest.mark.parametrize(("event", "timeout"), [("SessionStart", 30), ("UserPromptSubmit", 120), ("Stop", None)])
def test_a_wrong_or_missing_timeout_fails(plugin: Path, event: str, timeout: int | None) -> None:
    data = _hooks(plugin)
    hook = data["hooks"][event][0]["hooks"][0]
    if timeout is None:
        del hook["timeout"]
    else:
        hook["timeout"] = timeout
    _write_hooks(plugin, data)
    assert any(f"{event} timeout must be" in e for e in check_plugin(plugin))


def test_an_extra_hook_event_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["PreToolUse"] = data["hooks"]["Stop"]
    _write_hooks(plugin, data)
    assert any("events must be exactly" in e for e in check_plugin(plugin))


def test_a_missing_hook_event_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    del data["hooks"]["Stop"]
    _write_hooks(plugin, data)
    assert any("events must be exactly" in e for e in check_plugin(plugin))


def test_an_agent_without_frontmatter_fails(plugin: Path) -> None:
    (plugin / "agents" / "child.md").write_text("Invoke the `crook-child` skill.\n", encoding="utf-8")
    assert any("no frontmatter" in e for e in check_plugin(plugin))


def test_an_agent_without_a_name_fails(plugin: Path) -> None:
    _replace(plugin / "agents" / "child.md", "name: child\n", "")
    assert any("name must be 'child'" in e for e in check_plugin(plugin))


def test_an_agent_naming_a_missing_skill_fails(plugin: Path) -> None:
    _replace(plugin / "agents" / "child.md", "`crook-child`", "`crook-ghost`")
    errors = check_plugin(plugin)
    assert any("'crook-ghost', which is not in plugin/skills/" in e for e in errors)


def test_an_agent_naming_no_skill_fails(plugin: Path) -> None:
    _replace(plugin / "agents" / "child.md", "`crook-child`", "the skill")
    assert any("exactly one" in e for e in check_plugin(plugin))


def test_an_agent_naming_two_skills_fails(plugin: Path) -> None:
    with (plugin / "agents" / "child.md").open("a", encoding="utf-8") as fh:
        fh.write("Also use `crook-coordinator`.\n")
    assert any("exactly one" in e for e in check_plugin(plugin))


def test_an_agent_listing_skills_in_frontmatter_fails(plugin: Path) -> None:
    _replace(plugin / "agents" / "child.md", "model: sonnet\n", "model: sonnet\nskills: crook-child\n")
    assert any("must not list `skills:`" in e for e in check_plugin(plugin))


def test_an_agent_body_over_the_cap_fails(plugin: Path) -> None:
    with (plugin / "agents" / "child.md").open("a", encoding="utf-8") as fh:
        fh.write("More rules.\n" * 40)
    assert any("over the cap" in e for e in check_plugin(plugin))


def test_a_missing_agent_fails(plugin: Path) -> None:
    (plugin / "agents" / "coordinator.md").unlink()
    assert any("agent missing" in e for e in check_plugin(plugin))


def test_a_skill_without_a_description_fails(plugin: Path) -> None:
    path = plugin / "skills" / "crook-child" / "SKILL.md"
    lines = [ln for ln in path.read_text(encoding="utf-8").splitlines(keepends=True) if not ln.startswith("description:")]
    path.write_text("".join(lines), encoding="utf-8")
    assert any("description is missing" in e for e in check_plugin(plugin))


def test_a_skill_whose_name_differs_from_its_folder_fails(plugin: Path) -> None:
    _replace(plugin / "skills" / "crook-child" / "SKILL.md", "name: crook-child", "name: crook-kid")
    assert any("name must be 'crook-child'" in e for e in check_plugin(plugin))


def test_a_renamed_skill_folder_fails(plugin: Path) -> None:
    (plugin / "skills" / "crook-child").rename(plugin / "skills" / "crook-kid")
    errors = check_plugin(plugin)
    assert any("skill missing" in e for e in errors)


def test_a_brief_template_with_reordered_headings_fails(plugin: Path) -> None:
    path = plugin / "skills" / "crook-coordinator" / "brief-template.md"
    _replace(path, "## Goal", "## Zzz")
    assert any("headings must be" in e for e in check_plugin(plugin))


def test_a_result_template_with_the_wrong_status_fails(plugin: Path) -> None:
    _replace(plugin / "skills" / "crook-child" / "result-template.md", "status: working", "status: done")
    assert any("status: working" in e for e in check_plugin(plugin))


def test_a_missing_result_template_fails(plugin: Path) -> None:
    (plugin / "skills" / "crook-child" / "result-template.md").unlink()
    assert any("result-template.md" in e and "missing" in e for e in check_plugin(plugin))


# plugin-check --since-last-release, against a throwaway git repo.


def _git(repo: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)  # noqa: S607


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    root = tmp_path / "repo"
    root.mkdir()
    _git(root, "init", "-q")
    _git(root, "config", "user.email", "t@example.invalid")
    _git(root, "config", "user.name", "t")
    _git(root, "config", "commit.gpgsign", "false")
    _git(root, "config", "tag.gpgsign", "false")
    shutil.copytree(REPO_ROOT / "plugin", root / "plugin")
    _git(root, "add", "plugin")
    _git(root, "commit", "-q", "-m", "plugin")
    return root


def _run(repo: Path, *, since: bool = True) -> tuple[int, str]:
    ctx = make_context(repo)
    try:
        plugin_check(ctx, since_last_release=since)
    except SystemExit as exc:
        return int(exc.code or 0), ctx.stdout + ctx.stderr
    return 0, ctx.stdout + ctx.stderr


def test_without_a_tag_the_compatibility_check_is_skipped_with_a_notice(repo: Path) -> None:
    code, output = _run(repo)
    assert code == 0
    assert "no v* tag yet" in output


def test_an_unchanged_plugin_passes_against_its_tag(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0")
    code, output = _run(repo)
    assert code == 0, output
    assert "compared with v0.1.0" in output


def test_a_skill_renamed_since_the_tag_fails(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0")
    (repo / "plugin" / "skills" / "crook-child").rename(repo / "plugin" / "skills" / "crook-kid")
    code, output = _run(repo)
    assert code == 1
    assert "plugin/skills/crook-child/SKILL.md" in output


def test_an_agent_removed_since_the_tag_fails(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0")
    (repo / "plugin" / "agents" / "child.md").unlink()
    code, output = _run(repo)
    assert code == 1
    assert "plugin/agents/child.md" in output


def test_a_supporting_file_named_by_a_skill_and_then_removed_fails(repo: Path) -> None:
    skill = (repo / "plugin" / "skills" / "crook-coordinator" / "SKILL.md").read_text(encoding="utf-8")
    assert "models.md" in skill
    _git(repo, "tag", "v0.1.0")
    (repo / "plugin" / "skills" / "crook-coordinator" / "models.md").unlink()
    code, output = _run(repo)
    assert code == 1
    assert "models.md" in output


def test_the_newest_tag_wins_and_a_release_candidate_sorts_below_its_release(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0-rc.1")
    (repo / "plugin" / "skills" / "crook-child").rename(repo / "plugin" / "skills" / "crook-kid")
    _git(repo, "add", "plugin")
    _git(repo, "commit", "-q", "-m", "rename")
    _git(repo, "tag", "v0.1.0-rc.2")
    _git(repo, "tag", "v0.1.0")
    assert newest_release_tag(repo) == "v0.1.0"
    _git(repo, "tag", "-d", "v0.1.0")
    assert newest_release_tag(repo) == "v0.1.0-rc.2"


def test_without_the_flag_no_tag_is_read(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0")
    (repo / "plugin" / "agents" / "child.md").unlink()
    _, output = _run(repo, since=False)
    assert "compared with" not in output
