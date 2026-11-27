"""
CI checks for the repository.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

from toolr import Context
from toolr import command_group

group = command_group("ci", "CI checks for the repository", docstring=__doc__)

# 0019 and Q22: the shim expression, which honours MISE_DATA_DIR and XDG_DATA_HOME (build log D22).
SHIM = '"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}"'

# Hook event -> (crook subcommand argument, timeout in seconds). Plugin plan P1.3, Q25.
EXPECTED_HOOKS = {
    "SessionStart": ("session-start", 120),
    "UserPromptSubmit": ("user-prompt-submit", 30),
    "Stop": ("stop", 30),
}

PLUGIN_NAME = "crook"
# Names that never change (plugin plan, "Names that never change"; 0019).
PERMANENT_SKILLS = {"coordinator": "crook-coordinator", "child": "crook-child"}

# An agent file only says who the session is and names one skill (0019, 0025).
AGENT_BODY_MAX_LINES = 30

BRIEF_HEADINGS = ["Goal", "Why", "Scope", "References", "Done when"]
RESULT_HEADINGS = ["Progress", "Summary", "Links"]

_FRONTMATTER = re.compile(r"\A---\n(?P<body>.*?)\n---\n(?P<rest>.*)\Z", re.DOTALL)
_KEY = re.compile(r"^(?P<key>[A-Za-z_][\w-]*):[ \t]*(?P<value>.*)$")
_SKILL_REF = re.compile(r"`(crook-[a-z][a-z0-9-]*)`")
_HEADING = re.compile(r"^## (?P<title>.+?)\s*$", re.MULTILINE)


def _frontmatter(text: str) -> tuple[dict[str, str], str] | None:
    """Split a markdown file into its top-level frontmatter keys and its body.

    Only `key: value` lines are read; this is enough for `name`, `description`, `model` and `status`.
    """
    match = _FRONTMATTER.match(text)
    if match is None:
        return None
    keys: dict[str, str] = {}
    for line in match["body"].splitlines():
        if key := _KEY.match(line):
            keys[key["key"]] = key["value"].strip()
    return keys, match["rest"]


def _load_json(path: Path, errors: list[str]) -> object | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        errors.append(f"{path}: missing")
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        errors.append(f"{path}: not valid JSON ({exc})")
    return None


def _check_manifest(plugin: Path, errors: list[str]) -> None:
    path = plugin / ".claude-plugin" / "plugin.json"
    manifest = _load_json(path, errors)
    if manifest is None:
        return
    if not isinstance(manifest, dict):
        errors.append(f"{path}: must be a JSON object")
        return
    if manifest.get("name") != PLUGIN_NAME:
        errors.append(f"{path}: name must be {PLUGIN_NAME!r}, got {manifest.get('name')!r}")
    if not isinstance(manifest.get("version"), str) or not manifest["version"]:
        errors.append(f"{path}: version must be a non-empty string")


def _check_hooks(plugin: Path, errors: list[str]) -> None:
    path = plugin / "hooks" / "hooks.json"
    data = _load_json(path, errors)
    if data is None:
        return
    events = data.get("hooks") if isinstance(data, dict) else None
    if not isinstance(events, dict):
        errors.append(f"{path}: needs a top-level 'hooks' object")
        return
    if set(events) != set(EXPECTED_HOOKS):
        errors.append(f"{path}: events must be exactly {sorted(EXPECTED_HOOKS)}, got {sorted(events)}")
    for event, (argument, timeout) in EXPECTED_HOOKS.items():
        groups = events.get(event)
        if not isinstance(groups, list) or len(groups) != 1:
            if event in events:
                errors.append(f"{path}: {event} must hold exactly one matcher group")
            continue
        entries = groups[0].get("hooks") if isinstance(groups[0], dict) else None
        if not isinstance(entries, list) or len(entries) != 1 or not isinstance(entries[0], dict):
            errors.append(f"{path}: {event} must hold exactly one hook")
            continue
        hook = entries[0]
        if hook.get("type") != "command":
            errors.append(f"{path}: {event} hook type must be 'command'")
        expected = f"{SHIM} hook {argument}"
        if hook.get("command") != expected:
            errors.append(f"{path}: {event} command must be exactly {expected!r}, got {hook.get('command')!r}")
        if hook.get("timeout") != timeout or isinstance(hook.get("timeout"), bool):
            errors.append(f"{path}: {event} timeout must be {timeout}, got {hook.get('timeout')!r}")


def _skills(plugin: Path) -> dict[str, Path]:
    return {p.parent.name: p for p in sorted((plugin / "skills").glob("*/SKILL.md"))}


def _check_skills(plugin: Path, errors: list[str]) -> dict[str, Path]:
    skills = _skills(plugin)
    for name, path in skills.items():
        parsed = _frontmatter(path.read_text(encoding="utf-8"))
        if parsed is None:
            errors.append(f"{path}: no frontmatter")
            continue
        keys, _ = parsed
        if keys.get("name") != name:
            errors.append(f"{path}: name must be {name!r} (its folder), got {keys.get('name')!r}")
        if not keys.get("description"):
            errors.append(f"{path}: description is missing or empty")
    for wanted in PERMANENT_SKILLS.values():
        if wanted not in skills:
            errors.append(f"{plugin / 'skills' / wanted}: skill missing (its name never changes)")
    return skills


def _check_agents(plugin: Path, skills: dict[str, Path], errors: list[str]) -> None:
    agents = {p.stem: p for p in sorted((plugin / "agents").glob("*.md"))}
    for role in PERMANENT_SKILLS:
        if role not in agents:
            errors.append(f"{plugin / 'agents' / (role + '.md')}: agent missing (its name never changes)")
    for role, path in agents.items():
        parsed = _frontmatter(path.read_text(encoding="utf-8"))
        if parsed is None:
            errors.append(f"{path}: no frontmatter")
            continue
        keys, body = parsed
        if keys.get("name") != role:
            errors.append(f"{path}: name must be {role!r} (its file), got {keys.get('name')!r}")
        if not keys.get("description"):
            errors.append(f"{path}: description is missing or empty")
        # No `skills:` list: the body asks the model to invoke the skill, so it follows an upgrade (plan, agent files).
        if "skills" in keys:
            errors.append(f"{path}: must not list `skills:` in the frontmatter")
        named = sorted(set(_SKILL_REF.findall(body)))
        if len(named) != 1:
            errors.append(f"{path}: must name exactly one `crook-<role>` skill, found {named or 'none'}")
        else:
            if named[0] not in skills:
                errors.append(f"{path}: names skill {named[0]!r}, which is not in plugin/skills/")
            if role in PERMANENT_SKILLS and named[0] != PERMANENT_SKILLS[role]:
                errors.append(f"{path}: must name {PERMANENT_SKILLS[role]!r}, got {named[0]!r}")
        lines = len(body.strip().splitlines())
        if lines > AGENT_BODY_MAX_LINES:
            errors.append(f"{path}: body is {lines} lines, over the cap of {AGENT_BODY_MAX_LINES}")


def _headings(text: str) -> list[str]:
    return [m["title"] for m in _HEADING.finditer(text)]


def _check_templates(plugin: Path, errors: list[str]) -> None:
    brief = plugin / "skills" / "crook-coordinator" / "brief-template.md"
    if not brief.is_file():
        errors.append(f"{brief}: missing")
    elif _headings(brief.read_text(encoding="utf-8")) != BRIEF_HEADINGS:
        found = _headings(brief.read_text(encoding="utf-8"))
        errors.append(f"{brief}: headings must be {BRIEF_HEADINGS} in order (0024), got {found}")
    result = plugin / "skills" / "crook-child" / "result-template.md"
    if not result.is_file():
        errors.append(f"{result}: missing")
        return
    text = result.read_text(encoding="utf-8")
    parsed = _frontmatter(text)
    if parsed is None or parsed[0].get("status") != "working":
        errors.append(f"{result}: frontmatter must parse and give status: working (0024)")
    if _headings(text) != RESULT_HEADINGS:
        errors.append(f"{result}: headings must be {RESULT_HEADINGS} in order (0024), got {_headings(text)}")


def check_plugin(plugin: Path) -> list[str]:
    """Run every static check on a plugin folder and return the problems found."""
    errors: list[str] = []
    if not plugin.is_dir():
        return [f"{plugin}: not a folder"]
    _check_manifest(plugin, errors)
    _check_hooks(plugin, errors)
    skills = _check_skills(plugin, errors)
    _check_agents(plugin, skills, errors)
    _check_templates(plugin, errors)
    return errors


def _git(repo: Path, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(  # noqa: S603
        ["git", "-C", str(repo), *args],  # noqa: S607
        capture_output=True,
        text=True,
        check=False,
    )


def newest_release_tag(repo: Path) -> str | None:
    """The newest `v*` tag by version order, with pre-releases below their release (`-` suffix sorts first)."""
    ret = _git(repo, "-c", "versionsort.suffix=-", "tag", "--list", "v*", "--sort=-version:refname")
    tags = ret.stdout.split() if ret.returncode == 0 else []
    return tags[0] if tags else None


def check_compatibility(repo: Path, plugin_rel: str, tag: str) -> list[str]:
    """Names the tag's plugin used must still exist now (0019, plugin plan P4.3).

    Covers agent files, skill folders and the supporting files a tagged `SKILL.md` names.
    """
    errors: list[str] = []
    listing = _git(repo, "ls-tree", "-r", "--name-only", tag, "--", plugin_rel)
    if listing.returncode != 0:
        return [f"git ls-tree {tag} failed: {listing.stderr.strip()}"]
    old_files = [f for f in listing.stdout.splitlines() if f]
    prefix = f"{plugin_rel.rstrip('/')}/"

    def gone(old: str, what: str) -> None:
        if not (repo / old).is_file():
            errors.append(f"{old}: {what} existed at {tag} and is gone; a version never removes or renames it (0019)")

    for old in old_files:
        rel = old.removeprefix(prefix)
        if re.fullmatch(r"agents/[^/]+\.md", rel):
            gone(old, "agent file")
        elif re.fullmatch(r"skills/[^/]+/SKILL\.md", rel):
            gone(old, "skill")
            skill_dir = rel.rpartition("/")[0]
            text = _git(repo, "show", f"{tag}:{old}").stdout
            for other in old_files:
                other_rel = other.removeprefix(prefix)
                support = other_rel.removeprefix(f"{skill_dir}/")
                if other_rel.startswith(f"{skill_dir}/") and support != "SKILL.md" and support in text:
                    gone(other, "supporting file named by SKILL.md")
    return errors


@group.command
def plugin_check(ctx: Context, *, plugin_dir: str = "plugin", since_last_release: bool = False) -> None:
    """Check the plugin's files without Claude: JSON, frontmatter, hooks, skill names and templates.

    Checks that `plugin.json` and `hooks/hooks.json` parse; that the hooks are exactly the three events with
    the shim command form and the timeouts 120, 30 and 30; that each agent file has a name, names exactly one
    existing `crook-<role>` skill and stays under a line cap; that each `SKILL.md` has a name and a
    description; and that the brief and result templates keep their 0024 headings.

    With `--since-last-release`, also fails when an agent file, a skill or a supporting file that a skill
    names at the newest `v*` tag is gone now. The compatibility check is skipped with a notice when no tag
    exists.

    Args:
        plugin_dir: The plugin folder, relative to the repository root.
        since_last_release: Compare with the plugin at the newest `v*` tag.
    """
    root = Path(ctx.repo_root)
    plugin = root / plugin_dir
    errors = check_plugin(plugin)
    if since_last_release:
        tag = newest_release_tag(root)
        if tag is None:
            ctx.info("plugin-check: no v* tag yet, skipping the compatibility check")
        else:
            errors += check_compatibility(root, plugin_dir, tag)
            ctx.info(f"plugin-check: compared with {tag}")
    for error in errors:
        ctx.error(f"plugin-check: {error}", markup=False)
    if errors:
        ctx.exit(1)
    ctx.info("plugin-check: ok")
