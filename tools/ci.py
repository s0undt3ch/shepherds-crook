"""
CI checks for the repository.
"""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path
from pathlib import PurePosixPath

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
# The manifest carries metadata only: the folder layout is the one source of the plugin's agents, skills
# and hooks, so any key that points at components (`agents`, `skills`, `hooks`, `commands`, `mcpServers`,
# `outputStyles`, `lspServers`, ...) is refused by allow-listing instead of by a list that could go stale.
MANIFEST_KEYS = {"$schema", "name", "version", "description", "author", "license", "homepage", "repository", "keywords"}

# A release tag: `v` and a semantic version (https://semver.org), pre-release allowed.
RELEASE_TAG = re.compile(r"^v(?P<core>\d+\.\d+\.\d+)(?:-(?P<pre>[0-9A-Za-z.-]+))?$")
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
        # The first occurrence wins, as in the binary's result parser.
        if (key := _KEY.match(line)) and key["key"] not in keys:
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
    extra = sorted(set(manifest) - MANIFEST_KEYS)
    if extra:
        errors.append(f"{path}: keys {extra} are not allowed; the folder layout is the only source of components")
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
        # No `matcher`: SessionStart must also fire on resume, clear and compact.
        if not isinstance(groups[0], dict) or set(groups[0]) != {"hooks"}:
            errors.append(f"{path}: {event} group keys must be exactly ['hooks'] (no matcher)")
            continue
        entries = groups[0].get("hooks")
        if not isinstance(entries, list) or len(entries) != 1 or not isinstance(entries[0], dict):
            errors.append(f"{path}: {event} must hold exactly one hook")
            continue
        hook = entries[0]
        if set(hook) != {"type", "command", "timeout"}:
            errors.append(f"{path}: {event} hook keys must be exactly ['command', 'timeout', 'type'], got {sorted(hook)}")
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
    # Its frontmatter status is checked by the binary's own parser in tests/plugin_templates.rs.
    text = result.read_text(encoding="utf-8")
    if _headings(text) != RESULT_HEADINGS:
        errors.append(f"{result}: headings must be {RESULT_HEADINGS} in order (0024), got {_headings(text)}")


def _check_shim_text(plugin: Path, errors: list[str]) -> None:
    """Every mention of the shim path in the agents and skills must be the exact expression (D22)."""
    for path in sorted([*(plugin / "agents").glob("*.md"), *(plugin / "skills").rglob("*.md")]):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            rest = line.replace(SHIM, "")
            if "MISE_DATA_DIR" in rest or "shims/crook" in rest:
                errors.append(f"{path}:{number}: shim path is not the exact expression {SHIM}")


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
    _check_shim_text(plugin, errors)
    return errors


class GitError(Exception):
    """A git call failed, so the compatibility check cannot say anything and must fail."""


def _git(repo: Path, *args: str) -> str:
    ret = subprocess.run(  # noqa: S603
        ["git", "-C", str(repo), *args],  # noqa: S607
        capture_output=True,
        text=True,
        check=False,
    )
    if ret.returncode != 0:
        raise GitError(f"git {' '.join(args)} failed: {ret.stderr.strip() or ret.returncode}")
    return ret.stdout


def _precedence(tag: str) -> tuple:
    """Sort key by semver precedence: a pre-release is below its release; numeric identifiers sort below text."""
    match = RELEASE_TAG.match(tag)
    assert match is not None
    core = tuple(int(n) for n in match["core"].split("."))
    if match["pre"] is None:
        return core, (1,)
    ids = tuple((0, int(i), "") if i.isdigit() else (1, 0, i) for i in match["pre"].split("."))
    return core, (0, ids)


def release_tags(repo: Path) -> list[str]:
    """Release tags that are ancestors of HEAD and do not point at it, newest first by semver precedence.

    A tag on HEAD is the release being built, and comparing it with itself proves nothing.
    """
    out = _git(repo, "tag", "--list", "v*", "--merged", "HEAD", "--no-contains", "HEAD")
    return sorted((t for t in out.split() if RELEASE_TAG.match(t)), key=_precedence, reverse=True)


def newest_release_tag(repo: Path) -> str | None:
    tags = release_tags(repo)
    return tags[0] if tags else None


_TOKEN = re.compile(r"[\w./-]+")


def _named_files(text: str, skill_dir: str, candidates: set[str]) -> set[str]:
    """The candidate files (skill-relative paths) that `text` names as a whole file name or path.

    Whole tokens only, so `my-models.md` does not name `models.md`.
    """
    named: set[str] = set()
    for raw in _TOKEN.findall(text):
        token = raw.removeprefix("./").rstrip(".")
        for rel in candidates:
            if token in (rel, PurePosixPath(rel).name) or token.endswith(f"skills/{skill_dir}/{rel}"):
                named.add(rel)
    return named


def check_compatibility(repo: Path, plugin_rel: str, tags: list[str]) -> list[str]:
    """Names any earlier release used must still exist now (0019, plugin plan P4.3).

    Covers agent files, skill folders, and the supporting files a tagged `SKILL.md` names, directly or
    through another supporting file. The union over every tag in `tags`: a live conversation can still
    carry the text of any earlier release.
    """
    errors: list[str] = []
    prefix = f"{plugin_rel.rstrip('/')}/"

    def gone(old: str, tag: str, what: str) -> None:
        if not (repo / old).is_file():
            errors.append(f"{old}: {what} existed at {tag} and is gone; a version never removes or renames it (0019)")

    for tag in tags:
        old_files = [f for f in _git(repo, "ls-tree", "-r", "--name-only", tag, "--", plugin_rel).splitlines() if f]
        rels = {f.removeprefix(prefix) for f in old_files}
        for rel in sorted(rels):
            if re.fullmatch(r"agents/[^/]+\.md", rel):
                gone(prefix + rel, tag, "agent file")
            elif re.fullmatch(r"skills/[^/]+/SKILL\.md", rel):
                gone(prefix + rel, tag, "skill")
                skill_dir = rel.split("/")[1]
                base = f"skills/{skill_dir}/"
                support = {r.removeprefix(base) for r in rels if r.startswith(base) and r != rel}
                reached: set[str] = set()
                queue = [rel]
                while queue:
                    current = queue.pop()
                    try:
                        text = _git(repo, "show", f"{tag}:{prefix}{current}")
                    except GitError:
                        continue
                    for found in sorted(_named_files(text, skill_dir, support) - reached):
                        reached.add(found)
                        queue.append(base + found)
                for found in sorted(reached):
                    gone(f"{prefix}{base}{found}", tag, "supporting file named by a skill")
    return list(dict.fromkeys(errors))


def _plugin_rel(root: Path, plugin_dir: str) -> str | None:
    """The plugin folder as a clean repository-relative path (`./plugin` is `plugin`), or None outside it."""
    try:
        return (root / plugin_dir).resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return None


def _compatibility(root: Path, plugin_rel: str | None) -> tuple[list[str], str]:
    """The compatibility errors and a one-line status. Fails closed when git cannot answer."""
    if plugin_rel is None:
        return ["--plugin-dir is outside the repository, so it cannot be compared with a release tag"], ""
    try:
        if _git(root, "rev-parse", "--is-shallow-repository").strip() != "false":
            return ["the repository is a shallow clone: fetch the full history and tags (fetch-depth: 0)"], ""
        tags = release_tags(root)
        if not tags:
            return [], "no earlier v* release tag, skipping the compatibility check"
        return check_compatibility(root, plugin_rel, tags), f"compared with {', '.join(tags)}"
    except GitError as exc:
        return [f"cannot read release tags: {exc}"], ""


@group.command
def plugin_check(ctx: Context, *, plugin_dir: str = "plugin", since_last_release: bool = False) -> None:
    """Check the plugin's files without Claude: JSON, frontmatter, hooks, skill names and templates.

    Checks that `plugin.json` holds only metadata keys and `hooks/hooks.json` parses; that the hooks are
    exactly the three events, each one group of `hooks` (no matcher) with one `type`, `command` and
    `timeout`, the shim command form and the timeouts 120, 30 and 30; that each agent file has a name,
    names exactly one existing `crook-<role>` skill and stays under a line cap; that each `SKILL.md` has a
    name and a description; that every shim path in the agent and skill text is the exact expression; and
    that the brief and result templates keep their 0024 headings.

    With `--since-last-release`, also fails when an agent file, a skill, or a supporting file that a skill
    names was in any earlier release tag (`v*`, reachable from HEAD, not on HEAD) and is gone now. It skips
    with a notice when no such tag exists, and fails when git cannot answer or the clone is shallow.

    Args:
        plugin_dir: The plugin folder, relative to the repository root.
        since_last_release: Compare with the plugin at the earlier release tags.
    """
    root = Path(ctx.repo_root)
    errors = check_plugin(root / plugin_dir)
    if since_last_release:
        more, status = _compatibility(root, _plugin_rel(root, plugin_dir))
        errors += more
        if status:
            ctx.info(f"plugin-check: {status}")
    for error in errors:
        ctx.error(f"plugin-check: {error}", markup=False)
    if errors:
        ctx.exit(1)
    ctx.info("plugin-check: ok")
