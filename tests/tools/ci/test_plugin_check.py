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
from tools.ci import release_tags
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


def test_a_missing_result_template_fails(plugin: Path) -> None:
    (plugin / "skills" / "crook-child" / "result-template.md").unlink()
    assert any("result-template.md" in e and "missing" in e for e in check_plugin(plugin))


def test_a_component_path_key_in_plugin_json_fails(plugin: Path) -> None:
    for key in ("agents", "skills", "hooks", "commands", "mcpServers", "outputStyles", "lspServers"):
        path = plugin / ".claude-plugin" / "plugin.json"
        data = json.loads(path.read_text(encoding="utf-8"))
        data[key] = "./elsewhere"
        path.write_text(json.dumps(data), encoding="utf-8")
        assert any("are not allowed" in e and key in e for e in check_plugin(plugin)), key


def test_a_matcher_on_a_hook_group_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["SessionStart"][0]["matcher"] = "startup"
    _write_hooks(plugin, data)
    assert any("SessionStart group keys" in e for e in check_plugin(plugin))


def test_an_extra_key_on_a_hook_fails(plugin: Path) -> None:
    data = _hooks(plugin)
    data["hooks"]["Stop"][0]["hooks"][0]["async"] = True
    _write_hooks(plugin, data)
    assert any("Stop hook keys" in e for e in check_plugin(plugin))


def test_a_changed_shim_path_in_skill_text_fails(plugin: Path) -> None:
    _replace(plugin / "skills" / "crook-coordinator" / "SKILL.md", "${XDG_DATA_HOME:-$HOME/.local/share}", "$HOME/.local/share")
    assert any("not the exact expression" in e for e in check_plugin(plugin))


def test_a_stray_shim_mention_in_a_supporting_file_fails(plugin: Path) -> None:
    with (plugin / "skills" / "crook-child" / "result-template.md").open("a", encoding="utf-8") as fh:
        fh.write("\nRun ~/.local/share/mise/shims/crook now.\n")
    assert any("not the exact expression" in e for e in check_plugin(plugin))


# plugin-check --since-last-release, against a throwaway git repo.


def _git(repo: Path, *args: str) -> str:
    ret = subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, text=True)  # noqa: S607
    return ret.stdout


def _commit(repo: Path, message: str = "change") -> None:
    _git(repo, "add", "plugin")
    _git(repo, "commit", "-q", "-m", message)


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
    _commit(root, "plugin")
    return root


def _release(repo: Path, tag: str) -> None:
    """Tag HEAD, then add a commit so the tag is an earlier release."""
    _git(repo, "tag", tag)
    (repo / "plugin" / "skills" / "crook-child" / f"after-{tag}.txt").write_text("x", encoding="utf-8")
    _commit(repo, f"after {tag}")


def _run(repo: Path, *, since: bool = True, plugin_dir: str = "plugin") -> tuple[int, str]:
    ctx = make_context(repo)
    try:
        plugin_check(ctx, plugin_dir=plugin_dir, since_last_release=since)
    except SystemExit as exc:
        return int(exc.code or 0), ctx.stdout + ctx.stderr
    return 0, ctx.stdout + ctx.stderr


def test_without_a_tag_the_compatibility_check_is_skipped_with_a_notice(repo: Path) -> None:
    code, output = _run(repo)
    assert code == 0
    assert "no earlier v* release tag" in output


def test_an_unchanged_plugin_passes_against_an_earlier_tag(repo: Path) -> None:
    _release(repo, "v0.1.0")
    code, output = _run(repo)
    assert code == 0, output
    assert "compared with v0.1.0" in output


def test_a_tag_on_head_is_not_compared_with_itself(repo: Path) -> None:
    _git(repo, "tag", "v0.1.0")
    assert release_tags(repo) == []
    code, output = _run(repo)
    assert code == 0
    assert "no earlier v* release tag" in output


def test_a_tag_on_an_unmerged_branch_is_ignored(repo: Path) -> None:
    _git(repo, "checkout", "-q", "-b", "side")
    (repo / "plugin" / "agents" / "child.md").unlink()
    _commit(repo, "side")
    _git(repo, "tag", "v0.2.0")
    _git(repo, "checkout", "-q", "-")
    assert release_tags(repo) == []


def test_a_tag_that_is_not_a_semver_release_is_ignored(repo: Path) -> None:
    _release(repo, "v1")
    _release(repo, "v0.1.0-rc.1")
    _release(repo, "v0.1.0.extra")
    assert release_tags(repo) == ["v0.1.0-rc.1"]


def test_a_skill_renamed_since_the_tag_fails(repo: Path) -> None:
    _release(repo, "v0.1.0")
    (repo / "plugin" / "skills" / "crook-child").rename(repo / "plugin" / "skills" / "crook-kid")
    _commit(repo)
    code, output = _run(repo)
    assert code == 1
    assert "plugin/skills/crook-child/SKILL.md" in output


def test_an_agent_removed_since_the_tag_fails(repo: Path) -> None:
    _release(repo, "v0.1.0")
    (repo / "plugin" / "agents" / "child.md").unlink()
    _commit(repo)
    code, output = _run(repo)
    assert code == 1
    assert "plugin/agents/child.md" in output


def test_a_supporting_file_named_by_a_skill_and_then_removed_fails(repo: Path) -> None:
    _release(repo, "v0.1.0")
    (repo / "plugin" / "skills" / "crook-coordinator" / "models.md").unlink()
    _commit(repo)
    code, output = _run(repo)
    assert code == 1
    assert "models.md" in output


def test_a_file_named_only_by_another_supporting_file_is_protected(repo: Path) -> None:
    skill = repo / "plugin" / "skills" / "crook-coordinator"
    (skill / "models.md").write_text("See deep-notes.md for more.\n", encoding="utf-8")
    (skill / "deep-notes.md").write_text("notes\n", encoding="utf-8")
    _commit(repo)
    _release(repo, "v0.1.0")
    (skill / "deep-notes.md").unlink()
    _commit(repo)
    code, output = _run(repo)
    assert code == 1
    assert "deep-notes.md" in output


def test_a_file_whose_name_is_only_a_substring_of_a_mention_is_not_protected(repo: Path) -> None:
    skill = repo / "plugin" / "skills" / "crook-coordinator"
    (skill / "notes.md").write_text("unreferenced\n", encoding="utf-8")
    (skill / "SKILL.md").write_text(
        (skill / "SKILL.md").read_text(encoding="utf-8") + "\nSee my-notes.md and notes.md.bak.\n", encoding="utf-8"
    )
    _commit(repo)
    _release(repo, "v0.1.0")
    (skill / "notes.md").unlink()
    _commit(repo)
    code, output = _run(repo)
    assert code == 0, output


def test_every_earlier_tag_counts_not_only_the_newest(repo: Path) -> None:
    skill = repo / "plugin" / "skills" / "crook-coordinator"
    _release(repo, "v0.1.0")
    # v0.2.0 already lacks models.md and the SKILL.md that named it, so only v0.1.0 protects it.
    (skill / "models.md").unlink()
    text = (skill / "SKILL.md").read_text(encoding="utf-8").replace("models.md", "the models file")
    (skill / "SKILL.md").write_text(text, encoding="utf-8")
    _commit(repo)
    _release(repo, "v0.2.0")
    code, output = _run(repo)
    assert code == 1
    assert "models.md" in output
    assert "v0.1.0" in output


def test_a_release_candidate_sorts_below_its_release(repo: Path) -> None:
    _release(repo, "v0.1.0-rc.1")
    _release(repo, "v0.1.0-rc.10")
    _release(repo, "v0.1.0-rc.2")
    _release(repo, "v0.1.0")
    assert newest_release_tag(repo) == "v0.1.0"
    assert release_tags(repo) == ["v0.1.0", "v0.1.0-rc.10", "v0.1.0-rc.2", "v0.1.0-rc.1"]


def test_a_dot_slash_plugin_dir_is_the_same_as_plugin(repo: Path) -> None:
    _release(repo, "v0.1.0")
    (repo / "plugin" / "agents" / "child.md").unlink()
    _commit(repo)
    code, _ = _run(repo, plugin_dir="./plugin")
    assert code == 1


def test_a_shallow_clone_fails_closed(repo: Path, tmp_path: Path) -> None:
    _release(repo, "v0.1.0")
    shallow = tmp_path / "shallow"
    subprocess.run(["git", "clone", "-q", "--depth", "1", f"file://{repo}", str(shallow)], check=True)  # noqa: S607
    code, output = _run(shallow)
    assert code == 1
    assert "shallow" in output


def test_git_failing_fails_closed(tmp_path: Path) -> None:
    code, output = _run(tmp_path)  # not a git repository
    assert code == 1
    assert "cannot read release tags" in output


def test_without_the_flag_no_tag_is_read(repo: Path) -> None:
    _release(repo, "v0.1.0")
    (repo / "plugin" / "agents" / "child.md").unlink()
    _commit(repo)
    _, output = _run(repo, since=False)
    assert "compared with" not in output
