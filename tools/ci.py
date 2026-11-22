"""
CI related utilities.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
from pathlib import Path
from typing import NamedTuple

from toolr import Context
from toolr import command_group
from toolr.types import ResolvedPath

# tools/ci.py → repo root (two parents up resolves the `tools/` directory).
REPO_ROOT = Path(__file__).resolve().parents[1]

group = command_group("ci", "CI utilities", docstring=__doc__)

PACKSLIP_PROJECT = "github.com/s0undt3ch/shepherds-crook"
PACKSLIP_SOURCE_REPO = "https://github.com/s0undt3ch/shepherds-crook"
PACKSLIP_COMPLETION_SHELLS = ["bash", "zsh", "fish"]


class Target(NamedTuple):
    """One release archive: the Rust target triple and the runner that builds it natively."""

    triple: str
    runner: str
    # Built and smoke-run on every CI run. The rest only build at release, to keep
    # PR CI to one Linux and one macOS runner.
    ci: bool


TARGETS = (
    Target("x86_64-unknown-linux-musl", "ubuntu-latest", ci=True),
    Target("aarch64-unknown-linux-musl", "ubuntu-24.04-arm", ci=False),
    Target("aarch64-apple-darwin", "macos-15", ci=True),
    Target("x86_64-apple-darwin", "macos-15-intel", ci=False),
)


def targets_json(release: bool) -> str:
    """The `{triple, runner}` array the build and smoke matrices take."""
    return json.dumps(
        [{"triple": t.triple, "runner": t.runner} for t in TARGETS if release or t.ci],
        separators=(",", ":"),
    )


@group.command
def targets(ctx: Context, release: bool = False) -> None:
    """
    Print the release targets as JSON, and set them as the `targets` step output.

    The one list the CI build, the CI smoke job and the release build all read.
    The JSON goes to stdout as one plain line, so it can be piped.

    Args:
        release: Every release target. Without it, only the ones CI builds.
    """
    payload = targets_json(release)
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with Path(output).open("a", encoding="utf-8") as fh:
            fh.write(f"targets={payload}\n")
    sys.stdout.write(payload + "\n")


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
                PackslipFailure(
                    "artifact bin entries",
                    f"{name}: expected exactly one bin entry, got {len(bins)}",
                )
            )
            continue
        path = _bin_path(bins[0])
        if not (isinstance(path, str) and path.endswith("/crook")):
            failures.append(
                PackslipFailure(
                    "artifact bin entries",
                    f"{name}: bin entry {path} does not end in /crook",
                )
            )

    failures.extend(
        PackslipFailure(
            "linux libc",
            f"{a.get('name')}: os=linux but libc={a.get('libc') or 'null'} (expected musl)",
        )
        for a in artifacts
        if a.get("os") == "linux" and a.get("libc") != "musl"
    )

    # packslip installs skills user-wide, so a skill here would load in sessions crook did
    # not start. The protocol skill ships only inside the mod, which the binary embeds.
    skills = sorted(str(r.get("name")) for r in resources if r.get("kind") == "skill")
    if skills:
        failures.append(
            PackslipFailure(
                "skill resources",
                f"statement lists skill resources ({' '.join(skills)}); packslip would install "
                "them user-wide, so they must ship inside the mod instead",
            )
        )

    if not any(
        r.get("kind") == "completion" and r.get("shells") == PACKSLIP_COMPLETION_SHELLS
        for r in resources
    ):
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
    """
    Sign a throwaway packslip bundle from the built archives, then verify it.

    Catches drift between `.github/packslip.toml` and the release archives
    before release day: every archive has exactly one `crook` binary, Linux
    archives are musl, no skill resources are listed, and the completion
    resource covers bash, zsh and fish. The bundle is signed with an
    ephemeral key and never logged.

    Args:
        archive_dir: Directory holding the `*.tar.gz` release archives.
        packslip: Path to the packslip CLI. Defaults to `packslip` on `PATH`.
        manifest: The packslip manifest. Defaults to `.github/packslip.toml`
            in the repository root.
    """
    if not archive_dir.is_dir():
        _packslip_fail(ctx, "archives", f"{archive_dir} is not a directory")
        ctx.exit(1)
    archives = sorted(
        str(p) for p in archive_dir.iterdir() if p.is_file() and p.name.endswith(".tar.gz")
    )
    if not archives:
        _packslip_fail(ctx, "archives", f"no *.tar.gz files found in {archive_dir}")
        ctx.exit(1)

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

    manifest_path = (
        manifest if manifest is not None else ctx.repo_root / ".github" / "packslip.toml"
    )
    if not manifest_path.is_file():
        _packslip_fail(ctx, "manifest", f"{manifest_path} not found")
        ctx.exit(1)

    commit = _packslip_run(
        ctx, "git rev-parse HEAD", "git", "-C", str(ctx.repo_root), "rev-parse", "HEAD"
    ).strip()

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
