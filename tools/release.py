"""
Release utilities.
"""

from __future__ import annotations

import re
from pathlib import Path

from packaging.version import InvalidVersion
from packaging.version import Version

from toolr import Context
from toolr import command_group
from toolr.types import ResolvedPath

# tools/release.py → repo root (two parents up resolves the `tools/` directory).
REPO_ROOT = Path(__file__).resolve().parents[1]

group = command_group("release", "Release utilities", docstring=__doc__)

# `packaging` alone also accepts `1.2`, `v1.2.3` and `1.2.3rc1`; a release names exactly one
# spelling, which is also the SemVer form Cargo and the `v<version>` tag use.
_RELEASE_VERSION = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-rc\.(0|[1-9]\d*))?")


def _valid_release_version(version: str) -> bool:
    if not _RELEASE_VERSION.fullmatch(version):
        return False
    try:
        Version(version)
    except InvalidVersion:
        return False
    return True


@group.command
def bump(ctx: Context, version: str, manifest: ResolvedPath | None = None) -> None:
    """
    Set the crate version in Cargo.toml and Cargo.lock.

    Runs `cargo set-version` (cargo-edit), which updates both files together.

    Args:
        version: The release version: `X.Y.Z`, or `X.Y.Z-rc.N` for a pre-release.
        manifest: The Cargo.toml to update. Defaults to the one in the repository root.
    """
    if not _valid_release_version(version):
        ctx.error(
            f"{version!r} is not a release version: use X.Y.Z or X.Y.Z-rc.N, with no leading 'v'",
            markup=False,
        )
        ctx.exit(1)
    manifest_path = manifest or REPO_ROOT / "Cargo.toml"
    ret = ctx.run("cargo", "set-version", "--manifest-path", str(manifest_path), version)
    if ret.returncode != 0:
        ctx.error(f"cargo set-version failed with exit code {ret.returncode}")
        ctx.exit(ret.returncode)
    ctx.print(version)
