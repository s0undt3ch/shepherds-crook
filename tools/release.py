"""
Release utilities.
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

from packaging.version import InvalidVersion
from packaging.version import Version

from toolr import Context
from toolr import command_group
from toolr.types import ResolvedPath

# tools/release.py -> repo root (two parents up resolves the `tools/` directory).
REPO_ROOT = Path(__file__).resolve().parents[1]
PLUGIN_MANIFEST = REPO_ROOT / "plugin" / ".claude-plugin" / "plugin.json"

# v1's last commit (Q33). The changelog starts after it until a final release is tagged.
CHANGELOG_BASE = "714568f"

# What the plugin's version reads on `main` between releases (Q17); the release commit stamps the real one.
DEV_VERSION = "0.0.0-dev"

group = command_group("release", "Release utilities", docstring=__doc__)

# `packaging` alone also accepts `1.2`, `v1.2.3` and `1.2.3rc1`; a release names exactly one
# spelling, which is also the SemVer form Cargo and the `v<version>` tag use.
_RELEASE_VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-rc\.(0|[1-9]\d*))?")
# What `dev-version` prints (D124): X.Y.Z-devN or X.Y.Z-rc.K.devN, with +g<sha> on pull requests.
_DEV_VERSION = re.compile(
    r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)-(?:rc\.(?:0|[1-9]\d*)\.)?dev(?:0|[1-9]\d*)(?:\+g[0-9a-f]{7,40})?"
)
# The tags git-cliff counts as releases (cliff.toml `tag_pattern`): final versions only.
_FINAL_TAG = re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
# The `version` line of Cargo.toml's `[package]` table: the first `version =` before the next table header.
_CARGO_VERSION = re.compile(
    r'(^\[package\][^\n]*\n(?:[^\[\n][^\n]*\n|\n)*?version\s*=\s*)"[^"\n]*"', re.MULTILINE
)
# The top-level `"version"` line of plugin.json. The file is flat, so the first match is the plugin's.
_PLUGIN_VERSION = re.compile(r'("version"\s*:\s*)"[^"\n]*"')


def _valid_release_version(version: str) -> bool:
    if not _RELEASE_VERSION.fullmatch(version):
        return False
    try:
        Version(version)
    except InvalidVersion:
        return False
    return True


def stamp_cargo_version(manifest: Path, version: str) -> None:
    """Write `version` into a Cargo.toml's `[package]` table, leaving the rest of the file as it was.

    `cargo set-version` refuses to go down (`0.1.0` to `0.1.0-rc.1` is a downgrade to it), and the first
    release is a pre-release of the version the crate already carries.

    Raises:
        ValueError: The file has no `[package]` version line to replace.
    """
    text = manifest.read_text(encoding="utf-8")
    stamped, count = _CARGO_VERSION.subn(lambda m: f'{m[1]}"{version}"', text, count=1)
    if count != 1:
        raise ValueError(f"{manifest} has no [package] version line")
    manifest.write_text(stamped, encoding="utf-8")


def stamp_plugin_version(manifest: Path, version: str) -> None:
    """Write `version` into a plugin.json, leaving the rest of the file as it was.

    Raises:
        ValueError: The file has no `version` line to replace.
    """
    text = manifest.read_text(encoding="utf-8")
    stamped, count = _PLUGIN_VERSION.subn(lambda m: f'{m[1]}"{version}"', text, count=1)
    if count != 1:
        raise ValueError(f'{manifest} has no "version" line')
    manifest.write_text(stamped, encoding="utf-8")


@group.command
def bump(
    ctx: Context,
    version: str,
    dev: bool = False,
    manifest: ResolvedPath | None = None,
    plugin_manifest: ResolvedPath | None = None,
) -> None:
    """
    Set the release version in Cargo.toml, Cargo.lock and the plugin's plugin.json.

    Fetches the locked dependencies, writes the `[package]` version in Cargo.toml, then runs
    `cargo update --workspace --offline` so Cargo.lock follows (it changes only the crate's own entry; the
    fetch is what lets it run offline on a cold cargo home), then stamps `version` in plugin.json. The mod has no
    version file: `crook setup` writes its marker from the binary's version (0019).

    Args:
        version: The release version: `X.Y.Z`, or `X.Y.Z-rc.N` for a pre-release.
        dev: Take a dev version from `dev-version` instead, as CI does (D124). A dispatched release never passes it.
        manifest: The Cargo.toml to update. Defaults to the one in the repository root.
        plugin_manifest: The plugin.json to stamp. Defaults to `plugin/.claude-plugin/plugin.json`.
    """
    if dev and not _DEV_VERSION.fullmatch(version):
        ctx.error(f"{version!r} is not a dev version from `toolr release dev-version`", markup=False)
        ctx.exit(1)
    if not dev and not _valid_release_version(version):
        ctx.error(
            f"{version!r} is not a release version: use X.Y.Z or X.Y.Z-rc.N, with no leading 'v'",
            markup=False,
        )
        ctx.exit(1)
    manifest_path = manifest or REPO_ROOT / "Cargo.toml"
    plugin_path = plugin_manifest or PLUGIN_MANIFEST
    # Check before touching anything, so a missing file leaves no half-bumped tree.
    for path in (manifest_path, plugin_path):
        if not path.is_file():
            ctx.error(f"{path} does not exist", markup=False)
            ctx.exit(1)
    for path, pattern in ((manifest_path, _CARGO_VERSION), (plugin_path, _PLUGIN_VERSION)):
        if not pattern.search(path.read_text(encoding="utf-8")):
            ctx.error(f"{path} has no version line to replace", markup=False)
            ctx.exit(1)
    # Before the stamp, while Cargo.lock still matches Cargo.toml: `--locked` would refuse it afterwards.
    ret = ctx.run("cargo", "fetch", "--locked", "--manifest-path", str(manifest_path))
    if ret.returncode != 0:
        ctx.error(f"cargo fetch failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)
    try:
        stamp_cargo_version(manifest_path, version)
    except ValueError as exc:
        ctx.error(str(exc), markup=False)
        ctx.exit(1)
    # Syncs Cargo.lock with the new version and touches no dependency.
    ret = ctx.run("cargo", "update", "--workspace", "--offline", "--manifest-path", str(manifest_path))
    if ret.returncode != 0:
        ctx.error(f"cargo update failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)
    stamp_plugin_version(plugin_path, version)
    ctx.print(version)


@group.command
def dev_stamp(ctx: Context, plugin_manifest: ResolvedPath | None = None) -> None:
    """
    Put plugin.json's `version` back to `0.0.0-dev`, for the commit after a release (Q17).

    `main` keeps the dev stamp: tests and a debug build read it, and a stamped plugin there would let a debug
    `crook setup` replace a released mod. The release commit is tagged first; this is the commit on top of it.

    Args:
        plugin_manifest: The plugin.json to stamp. Defaults to `plugin/.claude-plugin/plugin.json`.
    """
    plugin_path = plugin_manifest or PLUGIN_MANIFEST
    try:
        stamp_plugin_version(plugin_path, DEV_VERSION)
    except (OSError, ValueError) as exc:
        ctx.error(str(exc), markup=False)
        ctx.exit(1)
    ctx.print(DEV_VERSION)


def _git(repo: Path, *args: str) -> tuple[int, str]:
    ret = subprocess.run(  # noqa: S603
        ["git", "-C", str(repo), *args],  # noqa: S607
        capture_output=True,
        text=True,
        check=False,
    )
    return ret.returncode, ret.stdout.strip()


def compute_dev_version(repo: Path, sha: str | None = None) -> str:
    """The version CI prepares a release as, after the latest release tag (D124), as ToolR does.

    After `vX.Y.Z` it is `X.Y.(Z+1)-devN`, after `vX.Y.Z-rc.K` it is `X.Y.Z-rc.(K+1).devN`, N being the commits since
    the tag, so it sorts above the tag and below the next release. With no tag the base is `v0.0.0`, below crook's
    first release, and N counts every commit. `sha` adds `+g<sha>`, which CI does on pull requests, so two PRs with
    the same count still differ.

    Raises:
        RuntimeError: git failed, or the latest tag is not a release tag.
    """
    code, tag = _git(repo, "describe", "--tags", "--abbrev=0", "--match", "v[0-9]*.[0-9]*.[0-9]*")
    if code != 0 or not tag:
        code, count = _git(repo, "rev-list", "--count", "HEAD")
        major, minor, patch, rc = 0, 0, 0, None
    else:
        m = _RELEASE_VERSION.fullmatch(tag.removeprefix("v"))
        if not m:
            raise RuntimeError(f"the latest tag {tag!r} is not a release tag (vX.Y.Z or vX.Y.Z-rc.N)")
        major, minor, patch = int(m[1]), int(m[2]), int(m[3])
        rc = int(m[5]) if m[5] is not None else None
        code, count = _git(repo, "rev-list", "--count", f"{tag}..HEAD")
    if code != 0 or not count.isdigit():
        raise RuntimeError(f"git rev-list failed in {repo}")
    if rc is None:
        version = f"{major}.{minor}.{patch + 1}-dev{count}"
    else:
        version = f"{major}.{minor}.{patch}-rc.{rc + 1}.dev{count}"
    return f"{version}+g{sha}" if sha else version


@group.command
def dev_version(ctx: Context, sha: str | None = None, repo: ResolvedPath | None = None) -> None:
    """
    Print the dev version CI prepares a release as (D124).

    Args:
        sha: A short commit hash to add as `+g<sha>`. CI passes it on pull requests.
        repo: The repository to look in. Defaults to this one.
    """
    if sha is not None and not re.fullmatch(r"[0-9a-f]{7,40}", sha):
        ctx.error(f"{sha!r} is not a short commit hash", markup=False)
        ctx.exit(1)
    try:
        version = compute_dev_version(repo or REPO_ROOT, sha)
    except RuntimeError as exc:
        ctx.error(str(exc), markup=False)
        ctx.exit(1)
    ctx.print(version, markup=False, highlight=False)


def final_tags(repo: Path) -> list[str]:
    """Final release tags (`vX.Y.Z`) that are ancestors of HEAD."""
    ret = subprocess.run(  # noqa: S603
        ["git", "-C", str(repo), "tag", "--list", "v*", "--merged", "HEAD"],  # noqa: S607
        capture_output=True,
        text=True,
        check=False,
    )
    if ret.returncode != 0:
        raise RuntimeError(f"git tag failed: {ret.stderr.strip() or ret.returncode}")
    return [t for t in ret.stdout.split() if _FINAL_TAG.fullmatch(t)]


@group.command
def changelog_range(ctx: Context, repo: ResolvedPath | None = None) -> None:
    """
    Print the commit range git-cliff must read for this release's notes, or nothing.

    Until a final `vX.Y.Z` tag exists, the range is `714568f..HEAD`, so v1's history stays out (Q33). That
    covers every pre-release too, as cliff.toml's `tag_pattern` ignores `-rc.N` tags. Once a final tag
    exists the output is empty, and git-cliff's `--unreleased` reads from that tag.

    Args:
        repo: The repository to look in. Defaults to this one.
    """
    root = repo or REPO_ROOT
    try:
        tags = final_tags(root)
    except RuntimeError as exc:
        ctx.error(str(exc), markup=False)
        ctx.exit(1)
    if not tags:
        ctx.print(f"{CHANGELOG_BASE}..HEAD", markup=False, highlight=False, emoji=False, soft_wrap=True)
