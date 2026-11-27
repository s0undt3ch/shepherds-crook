"""
CI checks for the repository.
"""

from __future__ import annotations

import fnmatch
import gzip
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path
from pathlib import PurePosixPath
from typing import NamedTuple

from toolr import Context
from toolr import command_group
from toolr.types import ResolvedPath

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

# The mod (mod plan M1.3, Q34). `mod.json` fields the mod plan names; `overlay` is a dialog with a size.
MOD_FIELDS = {"place": str, "title": str, "overlay": dict, "palette": list}
MOD_OVERLAY_FIELDS = {"kind": str, "width": int, "height": int}
MOD_PALETTE_FIELDS = {"title": str, "command": str, "args": dict, "words": str}
# Element ids the first view must render, read from `herdr-app check --print-spec`.
MOD_SPEC_IDS = ("crook-root", "crook-new")
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


def _is_number(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _check_mod_json(mod: Path, errors: list[str]) -> None:
    """`mod.json` parses and carries the fields the mod plan names, with the right types."""
    path = mod / "mod.json"
    data = _load_json(path, errors)
    if data is None:
        return
    if not isinstance(data, dict):
        errors.append(f"{path}: must be a JSON object")
        return

    def fields(where: str, obj: dict, spec: dict[str, type]) -> None:
        for key, kind in spec.items():
            if key not in obj:
                errors.append(f"{path}: {where} is missing `{key}`")
            elif kind is int and not _is_number(obj[key]) or kind is not int and not isinstance(obj[key], kind):
                errors.append(f"{path}: {where}.{key} must be a {kind.__name__}")

    fields("the file", data, MOD_FIELDS)
    if isinstance(data.get("overlay"), dict):
        fields("overlay", data["overlay"], MOD_OVERLAY_FIELDS)
    palette = data.get("palette")
    if isinstance(palette, list):
        if not palette:
            errors.append(f"{path}: palette must not be empty")
        for index, entry in enumerate(palette):
            if isinstance(entry, dict):
                fields(f"palette[{index}]", entry, MOD_PALETTE_FIELDS)
            else:
                errors.append(f"{path}: palette[{index}] must be an object")


def _mod_scripts(mod: Path) -> list[Path]:
    """Every `.js` and `.mjs` file under the mod folder."""
    return sorted(p for pattern in ("*.js", "*.mjs") for p in mod.rglob(pattern) if "node_modules" not in p.parts)


def _check_mod_syntax(mod: Path, errors: list[str]) -> None:
    """`node --check` on every script, read as an ES module whatever the nearest package.json says."""
    scripts = _mod_scripts(mod)
    if not scripts:
        errors.append(f"{mod}: no .js files")
    if not scripts or shutil.which("node") is None:
        if scripts:
            errors.append("node is not on PATH, so the scripts cannot be syntax-checked")
        return
    for script in scripts:
        # `--check` reads the file as CommonJS or ESM by its syntax; stdin with `--input-type` makes it ESM.
        result = subprocess.run(  # noqa: S603
            ["node", "--input-type=module", "--check"],  # noqa: S607
            input=script.read_text(encoding="utf-8"),
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0:
            detail = (result.stderr.strip().splitlines() or ["syntax error"])[0:4]
            errors.append(f"{script}: node --check failed: {' | '.join(detail)}")


Token = tuple[str, str, int]  # kind (ident, str, tpl, num, punct), text, line

# After these a `/` starts a regular expression; after any other identifier, number, `)`, `]` or `}` it divides.
_REGEX_AFTER_WORDS = {"return", "typeof", "case", "of", "in", "instanceof", "delete", "void", "throw", "new", "else", "do"}
_IDENT_START = re.compile(r"[A-Za-z_$\u0080-\uffff]")
_IDENT = re.compile(r"[\w$\u0080-\uffff]+")


class _Tokenizer:
    """Just enough JavaScript lexing to find calls: comments are dropped; strings, template literals and
    regular expressions are single tokens, so a `//` or a quote inside one hides nothing and a call inside
    a comment or a string is not seen. Calls inside a template's `${}` are tokenised as code."""

    def __init__(self, text: str) -> None:
        self.text = text
        self.tokens: list[Token] = []
        self.line = 1
        self._scan(0, nested=False)

    def _regex_allowed(self) -> bool:
        if not self.tokens:
            return True
        kind, text, _ = self.tokens[-1]
        if kind == "punct":
            return text not in ")]}"
        return kind == "ident" and text in _REGEX_AFTER_WORDS

    def _quoted(self, i: int, quote: str) -> int:
        start, j = i, i + 1
        while j < len(self.text) and self.text[j] != quote:
            if self.text[j] == "\\":
                j += 1
            if j < len(self.text) and self.text[j] == "\n":
                self.line += 1
            j += 1
        self.tokens.append(("str", self.text[start : j + 1], self.line))
        return j + 1

    def _template(self, i: int) -> int:
        j = i + 1
        self.tokens.append(("tpl", "`", self.line))
        while j < len(self.text) and self.text[j] != "`":
            if self.text[j] == "\\":
                j += 1
            elif self.text.startswith("${", j):
                j = self._scan(j + 2, nested=True)
                continue
            if j < len(self.text) and self.text[j] == "\n":
                self.line += 1
            j += 1
        return j + 1

    def _regex(self, i: int) -> int:
        j, in_class = i + 1, False
        while j < len(self.text) and self.text[j] != "\n":
            c = self.text[j]
            if c == "\\":
                j += 1
            elif c == "[":
                in_class = True
            elif c == "]":
                in_class = False
            elif c == "/" and not in_class:
                break
            j += 1
        j += 1
        while j < len(self.text) and self.text[j].isalpha():
            j += 1
        self.tokens.append(("str", "/regex/", self.line))
        return j

    def _scan(self, i: int, *, nested: bool) -> int:
        text, depth = self.text, 0
        while i < len(text):
            c = text[i]
            if c == "\n":
                self.line += 1
                i += 1
            elif c.isspace():
                i += 1
            elif text.startswith("//", i):
                while i < len(text) and text[i] != "\n":
                    i += 1
            elif text.startswith("/*", i):
                end = text.find("*/", i + 2)
                end = len(text) if end < 0 else end + 2
                self.line += text.count("\n", i, end)
                i = end
            elif c in "\"'":
                i = self._quoted(i, c)
            elif c == "`":
                i = self._template(i)
            elif _IDENT_START.match(c):
                m = _IDENT.match(text, i)
                self.tokens.append(("ident", m.group(), self.line))
                i = m.end()
            elif c.isdigit():
                m = re.compile(r"[\w.]+").match(text, i)
                self.tokens.append(("num", m.group(), self.line))
                i = m.end()
            elif c == "/" and self._regex_allowed():
                i = self._regex(i)
            else:
                if nested and c == "}" and depth == 0:
                    return i + 1
                depth += (c == "{") - (c == "}")
                if text.startswith("?.", i) and not text[i + 2 : i + 3].isdigit():
                    self.tokens.append(("punct", "?.", self.line))
                    i += 2
                else:
                    self.tokens.append(("punct", c, self.line))
                    i += 1
        return i


def _string_value(token: str) -> str:
    """The value of a quoted string token, with the common escapes decoded."""
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), token[1:-1])


def _herdr_names(tokens: list[Token]) -> tuple[set[str], set[str], set[int]]:
    """Local names bound to `exec` by an import from "herdr", the namespace names, and the import's token indices."""
    funcs, spaces, in_import = {"exec"}, set(), set()
    for i, (kind, text, _) in enumerate(tokens):
        if kind != "ident" or text != "import":
            continue
        end = i + 1
        while end < len(tokens) and not (tokens[end][0] == "ident" and tokens[end][1] == "from"):
            if tokens[end][1] == ";" or tokens[end][1] == "import":
                break
            end += 1
        if end + 1 >= len(tokens) or tokens[end][1] != "from" or tokens[end + 1][0] != "str":
            continue
        if _string_value(tokens[end + 1][1]) != "herdr":
            continue
        in_import.update(range(i, end + 2))
        clause = [t[1] for t in tokens[i + 1 : end]]
        for j, word in enumerate(clause):
            if word == "as" and j + 1 < len(clause):
                (funcs if j > 0 and clause[j - 1] == "exec" else spaces if clause[j - 1] == "*" else set()).add(clause[j + 1])
        if clause and clause[0] not in {"{", "*"}:
            spaces.add(clause[0])  # a default import: `h.exec(...)`
    return funcs, spaces, in_import


def _exec_problems(path: Path, text: str) -> list[str]:
    """Every use of herdr's `exec` in a script that is not `exec("sh", crookArgv(...))`."""
    tokens = _Tokenizer(text).tokens
    funcs, spaces, in_import = _herdr_names(tokens)
    problems: list[str] = []

    def at(i: int) -> Token:
        return tokens[i] if i < len(tokens) else ("", "", 0)

    for i, (kind, word, line) in enumerate(tokens):
        if kind != "ident" or i in in_import:
            continue
        after_dot = at(i - 1)[1] in {".", "?."} if i else False
        j = None  # index of the token after the callee
        if word in funcs and not after_dot:
            j = i + 1
        elif word in spaces and not after_dot and at(i + 1)[1] in {".", "?."} and at(i + 2)[1] == "exec":
            j = i + 3
        elif word in spaces and not after_dot and at(i + 1)[1] == "[":
            problems.append(f"{path}:{line}: `{word}[...]` hides which function is called")
            continue
        if j is None:
            continue
        if at(j)[1] == "?.":
            j += 1
        if at(j)[1] != "(":
            if at(j)[1] != ":":  # `{ exec: 1 }` is a property name, not a use
                problems.append(f"{path}:{line}: `{word}` is used without being called directly as exec(...)")
            continue
        first, comma, second, paren = at(j + 1), at(j + 2), at(j + 3), at(j + 4)
        if not (
            first[0] == "str"
            and first[1][0] in "\"'"
            and _string_value(first[1]) == "sh"
            and comma[1] == ","
            and second[1] == "crookArgv"
            and paren[1] == "("
        ):
            problems.append(f'{path}:{line}: exec must run "sh" with crookArgv(...), the shim script')
    return problems


def _shim_script_problems(lib: Path) -> list[str]:
    """`SHIM_SCRIPT` in lib.js is assigned exactly the shim expression (string pieces joined by `+`)."""
    tokens = _Tokenizer(lib.read_text(encoding="utf-8")).tokens
    expected = f'exec {SHIM} "$@"'
    for i, (kind, text, _) in enumerate(tokens):
        if kind == "ident" and text == "SHIM_SCRIPT" and tokens[i + 1][1:2] == ("=",):
            j, value = i + 2, ""
            while j < len(tokens) and tokens[j][0] == "str" and tokens[j][1][0] in "\"'":
                value += _string_value(tokens[j][1])
                j += 1
                if tokens[j][1] == "+":
                    j += 1
                else:
                    break
            if value == expected and tokens[j][1] in {";", ""}:
                return []
            return [f"{lib}: SHIM_SCRIPT is not assigned exactly `{expected}`"]
    return [f"{lib}: SHIM_SCRIPT is not defined"]


def _check_mod_exec(mod: Path, errors: list[str]) -> None:
    """Every use of `exec` runs `sh` with `crookArgv(...)`, and the script in lib.js is the exact shim."""
    for script in _mod_scripts(mod):
        errors.extend(_exec_problems(script, script.read_text(encoding="utf-8")))
    lib = mod / "lib.js"
    if lib.is_file():
        errors.extend(_shim_script_problems(lib))
    else:
        errors.append(f"{lib}: missing, so the shim script cannot be checked")


def check_mod(mod: Path) -> list[str]:
    """Run the static checks on a mod folder and return the problems found."""
    errors: list[str] = []
    if not mod.is_dir():
        return [f"{mod}: not a folder"]
    _check_mod_json(mod, errors)
    _check_mod_syntax(mod, errors)
    _check_mod_exec(mod, errors)
    return errors


@group.command
def mod_check(ctx: Context, *, mod_dir: str = "mod") -> None:
    """Check the herdr mod's files without herdr-app: syntax, `mod.json` shape and the crook shim.

    Runs `node --check` on each `.js` file; checks that `mod.json` holds `place`, `title`, an `overlay`
    with `kind`, `width` and `height`, and a non-empty `palette` whose entries hold `title`, `command`,
    `args` and `words`; and that every `exec(` in the mod runs `sh` with `crookArgv(...)`, whose script
    in `lib.js` is the exact shim expression (so `CROOK_BIN` and the shim path are the only ways in).

    Args:
        mod_dir: The mod folder, relative to the repository root.
    """
    errors = check_mod(Path(ctx.repo_root) / mod_dir)
    for error in errors:
        ctx.error(f"mod-check: {error}", markup=False)
    if errors:
        ctx.exit(1)
    ctx.info("mod-check: ok")


def herdr_app_check(mod: Path, herdr_app: str) -> list[str]:
    """Run `herdr-app check` on a temporary copy of the mod and look for the required element ids."""
    errors: list[str] = []
    with tempfile.TemporaryDirectory(prefix="crook-mod-check-") as tmp:
        copy = Path(tmp) / "mod"
        shutil.copytree(mod, copy, ignore=shutil.ignore_patterns("gpui-kit.d.ts", "jsconfig.json"))
        # A throwaway app home, so the check never reads or writes the real one.
        # herdr-app also keeps per-folder state under the home and XDG folders, so all of them move.
        home = Path(tmp) / "home"
        env = {
            **os.environ,
            "HERDR_APP_HOME": str(home / "app"),
            "HOME": str(home),
            "XDG_DATA_HOME": str(home / "data"),
            "XDG_CONFIG_HOME": str(home / "config"),
            "XDG_CACHE_HOME": str(home / "cache"),
        }
        home.mkdir()
        for args in (["check", str(copy)], ["check", str(copy), "--print-spec"]):
            try:
                result = subprocess.run(  # noqa: S603
                    [herdr_app, *args], capture_output=True, text=True, check=False, env=env, timeout=30
                )
            except subprocess.TimeoutExpired:
                return [f"herdr-app {args[0]} timed out"]
            if result.returncode != 0:
                tail = (result.stdout + result.stderr).strip()
                return [f"herdr-app {args[0]} failed ({result.returncode}): {tail}"]
        missing = [i for i in MOD_SPEC_IDS if re.search(rf':id\[Str\("{re.escape(i)}"\)\]', result.stdout) is None]
        errors += [f"--print-spec does not show the element id {i}" for i in missing]
    return errors


@group.command
def mod_herdr_check(ctx: Context, *, mod_dir: str = "mod") -> None:
    """Load the mod once with `herdr-app check`, on a temporary copy, and look for its element ids.

    `herdr-app check` writes `gpui-kit.d.ts` and `jsconfig.json` into the folder it checks, so the copy is
    checked and `mod/` itself is never touched. Skips with a message, and exit 0, when `herdr-app` is not on
    `PATH` (CI, which cannot install it: Q34).

    Args:
        mod_dir: The mod folder, relative to the repository root.
    """
    herdr_app = shutil.which("herdr-app")
    if herdr_app is None:
        ctx.info("mod-herdr-check: herdr-app is not installed, skipping")
        return
    errors = herdr_app_check(Path(ctx.repo_root) / mod_dir, herdr_app)
    for error in errors:
        ctx.error(f"mod-herdr-check: {error}", markup=False)
    if errors:
        ctx.exit(1)
    ctx.info("mod-herdr-check: ok")


PACKSLIP_PROJECT = "github.com/s0undt3ch/shepherds-crook"
PACKSLIP_SOURCE_REPO = "https://github.com/s0undt3ch/shepherds-crook"
PACKSLIP_COMPLETION_SHELLS = ["bash", "zsh", "fish"]

# `X.Y.Z` or `X.Y.Z-<pre>`; CI archives use `0.0.0-ci`.
ARCHIVE_VERSION = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")
ARCHIVE_TRIPLE = re.compile(r"^[A-Za-z0-9_.]+(?:-[A-Za-z0-9_.]+)+$")
# What an archive must hold below its one top folder (0019).
ARCHIVE_FILES = ("crook", "plugin/.claude-plugin/plugin.json", "mod/mod.json")
# Files that a build, herdr-app or an editor leaves behind. An archive carries only what git tracks under
# `plugin/` and `mod/`, and `archive-check` rejects these names even when someone tracked one.
ARCHIVE_JUNK = (
    ".DS_Store",
    "__pycache__",
    "node_modules",
    "gpui-kit.d.ts",
    "jsconfig.json",
    "*.pyc",
    "*~",
    "*.orig",
    "*.rej",
    "*.swp",
    "*.swo",
    "*.bak",
)


class Target(NamedTuple):
    """One release archive: the Rust target triple and the runner that builds it natively."""

    triple: str
    runner: str
    # False keeps a slow-queue runner off pull requests. Pushes to main and releases always build
    # and smoke every target.
    on_prs: bool = True


# Q31: Linux (musl) and macOS, each on x86_64 and aarch64.
TARGETS = (
    Target("x86_64-unknown-linux-musl", "ubuntu-latest"),
    Target("aarch64-unknown-linux-musl", "ubuntu-24.04-arm"),
    Target("aarch64-apple-darwin", "macos-15"),
    Target("x86_64-apple-darwin", "macos-15-intel", on_prs=False),
)


@group.command
def targets(ctx: Context) -> None:
    """Print the build targets as JSON, and set them as the `targets` step output.

    The one list the CI and release builds, and their smoke jobs, all read. On a pull request
    (`GITHUB_EVENT_NAME=pull_request`) it leaves out the targets marked `on_prs=False`. The JSON goes
    to stdout as one plain line, so it can be piped.
    """
    pull_request = os.environ.get("GITHUB_EVENT_NAME") == "pull_request"
    payload = json.dumps(
        [{"triple": t.triple, "runner": t.runner} for t in TARGETS if t.on_prs or not pull_request],
        separators=(",", ":"),
    )
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with Path(output).open("a", encoding="utf-8") as fh:
            fh.write(f"targets={payload}\n")
    ctx.print(payload, markup=False, highlight=False, emoji=False, soft_wrap=True)


def stage_archive_folder(root: Path, binary: Path, out: Path, *, version: str, triple: str) -> Path:
    """Write `crook-<version>-<triple>/` under `out`: the binary, `plugin/`, `mod/` and the top-level docs.

    Raises:
        ValueError: A bad version or triple, or a source file or folder that is missing.
    """
    if not ARCHIVE_VERSION.match(version):
        raise ValueError(f"version {version!r} is not X.Y.Z or X.Y.Z-<pre>")
    if not ARCHIVE_TRIPLE.match(triple):
        raise ValueError(f"target triple {triple!r} is not a Rust target triple")
    if not binary.is_file():
        raise ValueError(f"the binary {binary} does not exist")
    for rel in ("plugin/.claude-plugin/plugin.json", "mod/mod.json", "LICENSE", "README.md"):
        if not (root / rel).is_file():
            raise ValueError(f"{root / rel} does not exist")
    tracked = _tracked_files(root)
    folder = out / f"crook-{version}-{triple}"
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    shutil.copy2(binary, folder / "crook")
    (folder / "crook").chmod(0o755)
    for rel in tracked:
        source = root / rel
        if source.is_symlink():
            raise ValueError(f"{rel} is a symbolic link; an archive never carries one")
        if not source.is_file():
            raise ValueError(f"{rel} is tracked by git but is not a file in the working tree")
        target = folder / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
    for rel in ("plugin/.claude-plugin/plugin.json", "mod/mod.json"):
        if not (folder / rel).is_file():
            raise ValueError(f"{rel} is not tracked by git, so it would not ship")
    for name in ("LICENSE", "README.md", "CHANGELOG.md"):
        if (root / name).is_file():
            shutil.copy2(root / name, folder / name)
    return folder


def _tracked_files(root: Path) -> list[str]:
    """The files git tracks (index included) under `plugin/` and `mod/`."""
    try:
        done = subprocess.run(  # noqa: S603
            ["git", "ls-files", "-z", "--cached", "--", "plugin", "mod"],  # noqa: S607
            cwd=root,
            capture_output=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise ValueError(f"cannot list the files git tracks in {root}: {exc}") from exc
    return sorted(name for name in done.stdout.decode().split("\0") if name)


def _archive_mtime(root: Path) -> int:
    """`SOURCE_DATE_EPOCH` when set, else the time of the last commit, else 0."""
    given = os.environ.get("SOURCE_DATE_EPOCH", "")
    if given.isdigit():
        return int(given)
    try:
        done = subprocess.run(  # noqa: S603
            ["git", "log", "-1", "--format=%ct"],  # noqa: S607
            cwd=root,
            capture_output=True,
            check=True,
            text=True,
        )
        return int(done.stdout.strip())
    except (OSError, subprocess.CalledProcessError, ValueError):
        return 0


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def pack_archive(folder: Path, dist: Path, *, mtime: int = 0) -> Path:
    """Write `<folder name>.tar.gz` and its `.sha256` (`<hex>  <name>`) into `dist`; return the tarball.

    The bytes depend only on the folder's contents and `mtime`: members are sorted, owners are cleared and
    every member and the gzip header carry `mtime`.
    """
    dist.mkdir(parents=True, exist_ok=True)
    tarball = dist / f"{folder.name}.tar.gz"

    def _owner(info: tarfile.TarInfo) -> tarfile.TarInfo:
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        info.mtime = mtime
        return info

    with (
        tarball.open("wb") as raw,
        gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=mtime) as gz,
        tarfile.open(fileobj=gz, mode="w") as tf,
    ):
        tf.add(folder, arcname=folder.name, filter=_owner)
    tarball.with_name(f"{tarball.name}.sha256").write_text(f"{_sha256(tarball)}  {tarball.name}\n", encoding="utf-8")
    return tarball


@group.command
def stage_archive(
    ctx: Context,
    triple: str,
    version: str,
    *,
    binary: ResolvedPath | None = None,
    out: str | None = None,
    dist: str | None = None,
) -> None:
    """Stage one release archive: `crook-<version>-<triple>/` holding `crook`, `plugin/` and `mod/`.

    `plugin/` and `mod/` hold only the files git tracks there, so a working tree's stray files and links
    never ship. The tarball is reproducible: its times come from `SOURCE_DATE_EPOCH`, else the last commit.
    The folder also gets `LICENSE`, `README.md` and `CHANGELOG.md` when present. The CI build and the
    e2e harness both stage through this command, so the layout (0019) has one source. mise strips a lone
    top folder, so `plugin/` and `mod/` land next to `crook` once installed.

    Args:
        triple: The Rust target triple, as in `toolr ci targets`.
        version: The version named in the folder and the archive. It does not change the binary.
        binary: The built `crook`. Defaults to `target/<triple>/release/crook` in the repository.
        out: The folder that receives `crook-<version>-<triple>/`. Defaults to `stage` in the repository.
        dist: When given, also write `crook-<version>-<triple>.tar.gz` and its `.sha256` here.
    """
    root = Path(ctx.repo_root)
    binary_path = Path(binary) if binary is not None else root / "target" / triple / "release" / "crook"
    out_path = Path(out) if out is not None else root / "stage"
    try:
        folder = stage_archive_folder(root, binary_path, out_path, version=version, triple=triple)
    except ValueError as exc:
        ctx.error(f"stage-archive: {exc}", markup=False)
        ctx.exit(1)
    ctx.info(f"stage-archive: staged {folder}")
    if dist is not None:
        tarball = pack_archive(folder, Path(dist), mtime=_archive_mtime(root))
        ctx.info(f"stage-archive: wrote {tarball}")


def check_archive(tarball: Path) -> list[str]:
    """The layout problems of one release tarball (0019); empty means it is good.

    The tarball must have exactly one top folder, named as the tarball is, holding an executable `crook`,
    `plugin/.claude-plugin/plugin.json` and `mod/mod.json`; no member may leave that folder, be a link or
    be a file git would not ship.
    """
    stem = tarball.name.removesuffix(".tar.gz")
    try:
        with tarfile.open(tarball, "r:gz") as tf:
            members = tf.getmembers()
    except (tarfile.TarError, OSError) as exc:
        return [f"{tarball.name}: cannot read the tarball: {exc}"]
    errors: list[str] = []
    files: dict[str, tarfile.TarInfo] = {}
    tops: set[str] = set()
    for member in members:
        path = PurePosixPath(member.name)
        parts = [part for part in path.parts if part != "."]
        if path.is_absolute() or ".." in parts:
            errors.append(f"{tarball.name}: member {member.name!r} leaves the archive folder")
            continue
        if not parts:
            continue
        tops.add(parts[0])
        if not (member.isfile() or member.isdir()):
            errors.append(f"{tarball.name}: member {member.name!r} is a link or a special file")
            continue
        if any(fnmatch.fnmatch(part, pattern) for part in parts[1:] for pattern in ARCHIVE_JUNK):
            errors.append(f"{tarball.name}: member {member.name!r} is a stray file that git would not ship")
        if len(parts) > 1 and member.isfile():
            files["/".join(parts[1:])] = member
    if len(tops) != 1:
        return errors + [f"{tarball.name}: expected one top folder, found {sorted(tops) or 'none'}"]
    (top,) = tops
    if top != stem:
        errors.append(f"{tarball.name}: the top folder is {top!r}, expected {stem!r}")
    errors += [f"{tarball.name}: {top}/{rel} is missing" for rel in ARCHIVE_FILES if rel not in files]
    if "crook" in files and not files["crook"].mode & 0o111:
        errors.append(f"{tarball.name}: {top}/crook is not executable")
    return errors


def _tarballs(ctx: Context, check: str, archive_dir: Path) -> list[Path]:
    if not archive_dir.is_dir():
        ctx.error(f"{check}: archives failed: {archive_dir} is not a directory", markup=False)
        ctx.exit(1)
    found = sorted(p for p in archive_dir.iterdir() if p.is_file() and p.name.endswith(".tar.gz"))
    if not found:
        ctx.error(f"{check}: archives failed: no *.tar.gz files found in {archive_dir}", markup=False)
        ctx.exit(1)
    return found


@group.command
def archive_check(ctx: Context, archive_dir: ResolvedPath) -> None:
    """Check each release tarball's layout: one top folder with `crook`, `plugin/` and `mod/` (0019).

    Each tarball needs a `.sha256` file that matches it, and no member may be a link or a stray file.

    Args:
        archive_dir: Directory holding the `*.tar.gz` release archives.
    """
    tarballs = _tarballs(ctx, "archive-check", Path(archive_dir))
    errors: list[str] = []
    for tarball in tarballs:
        errors += check_archive(tarball)
        sidecar = tarball.with_name(f"{tarball.name}.sha256")
        if not sidecar.is_file():
            errors.append(f"{sidecar.name}: is missing")
        elif sidecar.read_text(encoding="utf-8").strip() != f"{_sha256(tarball)}  {tarball.name}":
            errors.append(f"{sidecar.name}: does not match {tarball.name}")
    for error in errors:
        ctx.error(f"archive-check: {error}", markup=False)
    if errors:
        ctx.exit(1)
    ctx.info(f"archive-check: ok ({len(tarballs)} archives)")


class PackslipFailure(NamedTuple):
    """One failed `packslip-check` assertion: which check, and why."""

    check: str
    detail: str


def _bin_path(entry: object) -> object:
    return entry.get("path") if isinstance(entry, dict) else entry


def _packslip_statement_failures(statement: object) -> list[PackslipFailure]:
    """Check a `packslip show` statement against what a crook release must ship.

    Args:
        statement: The parsed `packslip show` JSON.

    Returns:
        Every failed assertion, in check order. Empty means the statement passes.
    """
    predicate = (statement.get("predicate") or {}) if isinstance(statement, dict) else None
    if not isinstance(predicate, dict):
        return [PackslipFailure("statement shape", "statement has no predicate object")]
    artifacts = predicate.get("artifacts") or []
    resources = predicate.get("resources") or []
    failures = [
        PackslipFailure("statement shape", f"predicate.{key} is not a list of objects")
        for key, entries in (("artifacts", artifacts), ("resources", resources))
        if not isinstance(entries, list) or not all(isinstance(e, dict) for e in entries)
    ]
    if failures:
        return failures

    if not artifacts:
        failures.append(PackslipFailure("artifacts", "statement lists no artifacts"))

    for artifact in artifacts:
        name = artifact.get("name")
        bins = artifact.get("bin") or []
        if not isinstance(bins, list):
            failures.append(PackslipFailure("artifact bin entries", f"{name}: bin is not a list"))
            continue
        if len(bins) != 1:
            failures.append(
                PackslipFailure("artifact bin entries", f"{name}: expected exactly one bin entry, got {len(bins)}")
            )
            continue
        path = _bin_path(bins[0])
        if not (isinstance(path, str) and path.endswith("/crook")):
            failures.append(PackslipFailure("artifact bin entries", f"{name}: bin entry {path} does not end in /crook"))

    failures.extend(
        PackslipFailure(
            "linux libc",
            f"{a.get('name')}: os=linux but libc={a.get('libc') or 'null'} (expected musl)",
        )
        for a in artifacts
        if a.get("os") == "linux" and a.get("libc") != "musl"
    )

    # packslip installs skills user-wide, so a skill here would load in sessions crook did not start
    # (0019). The plugin's skills ship inside `plugin/`, which Claude Code loads per session.
    skills = sorted(str(r.get("name")) for r in resources if r.get("kind") == "skill")
    if skills:
        failures.append(
            PackslipFailure(
                "skill resources",
                f"statement lists skill resources ({' '.join(skills)}); packslip would install "
                "them user-wide, so they must ship inside plugin/ instead",
            )
        )

    if not any(r.get("kind") == "completion" and r.get("shells") == PACKSLIP_COMPLETION_SHELLS for r in resources):
        failures.append(
            PackslipFailure(
                "completion resource",
                f"no completion resource with shells == {json.dumps(PACKSLIP_COMPLETION_SHELLS)} found",
            )
        )

    return failures


def _packslip_fail(ctx: Context, check: str, detail: str) -> None:
    ctx.error(f"packslip-check: {check} failed: {detail}", markup=False)


def _as_text(data: str | bytes) -> str:
    return data.decode(errors="replace") if isinstance(data, bytes) else data


def _packslip_run(ctx: Context, check: str, *cmdline: str) -> str:
    ret = ctx.run(*cmdline, capture_output=True, stream_output=False)
    stdout = _as_text(ret.stdout.read())
    if ret.returncode != 0:
        stderr = _as_text(ret.stderr.read())
        output = "\n".join(part for part in (stdout.rstrip(), stderr.rstrip()) if part)
        _packslip_fail(ctx, check, output)
        ctx.exit(1)
    return stdout


@group.command
def packslip_check(
    ctx: Context,
    archive_dir: ResolvedPath,
    packslip: ResolvedPath | None = None,
    manifest: ResolvedPath | None = None,
) -> None:
    """Sign a throwaway packslip bundle from the built archives, then verify it.

    Catches drift between `.github/packslip.toml` and the release archives before release day: every
    archive has exactly one `crook` binary, Linux archives are musl, no skill resources are listed, and
    the completion resource covers bash, zsh and fish. The bundle is signed with an ephemeral key and
    never logged.

    Args:
        archive_dir: Directory holding the `*.tar.gz` release archives.
        packslip: Path to the packslip CLI. Defaults to `packslip` on `PATH`.
        manifest: The packslip manifest. Defaults to `.github/packslip.toml` in the repository root.
    """
    archives = [str(p) for p in _tarballs(ctx, "packslip-check", Path(archive_dir))]

    if packslip is None:
        packslip_bin = ctx.which("packslip")
        if packslip_bin is None:
            _packslip_fail(ctx, "packslip", "packslip not found on PATH")
            ctx.exit(1)
    elif packslip.is_file():
        packslip_bin = str(packslip)
    else:
        _packslip_fail(ctx, "packslip", f"{packslip} is not a file")
        ctx.exit(1)

    manifest_path = manifest if manifest is not None else ctx.repo_root / ".github" / "packslip.toml"
    if not manifest_path.is_file():
        _packslip_fail(ctx, "manifest", f"{manifest_path} not found")
        ctx.exit(1)

    commit = _packslip_run(ctx, "git rev-parse HEAD", "git", "-C", str(ctx.repo_root), "rev-parse", "HEAD").strip()

    with tempfile.TemporaryDirectory() as tmp:
        key = Path(tmp) / "k"
        out = Path(tmp) / "out"
        _packslip_run(ctx, "packslip keygen", packslip_bin, "keygen", "-o", str(key))
        _packslip_run(
            ctx,
            "packslip create",
            packslip_bin,
            "create",
            "--manifest",
            str(manifest_path),
            "--key",
            str(key),
            "--no-log",
            "--out",
            str(out),
            "--project",
            PACKSLIP_PROJECT,
            "--version",
            "0.0.0-ci",
            "--commit",
            commit,
            "--source-repo",
            PACKSLIP_SOURCE_REPO,
            *archives,
        )
        bundle = out / "packslip.sigstore.json"
        if not bundle.is_file():
            _packslip_fail(ctx, "packslip create", f"expected bundle at {bundle}, none produced")
            ctx.exit(1)
        artifact_args = [arg for archive in archives for arg in ("--artifact", archive)]
        _packslip_run(
            ctx,
            "packslip verify",
            packslip_bin,
            "verify",
            str(bundle),
            "--pubkey",
            f"{key}.pub",
            "--allow-unlogged",
            *artifact_args,
        )
        show_out = _packslip_run(ctx, "packslip show", packslip_bin, "show", str(bundle))

    try:
        statement = json.loads(show_out)
    except json.JSONDecodeError as exc:
        _packslip_fail(ctx, "packslip show", f"output is not JSON: {exc}")
        ctx.exit(1)

    failures = _packslip_statement_failures(statement)
    for failure in failures:
        _packslip_fail(ctx, failure.check, failure.detail)
    if failures:
        ctx.exit(1)
    ctx.print("packslip-check: ok")
