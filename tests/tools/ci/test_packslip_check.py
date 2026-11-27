"""Tests for ``toolr ci packslip-check`` in ``tools/ci.py``.

The statement assertions run through ``_packslip_statement_failures`` against
factory-built statements, so they need no packslip CLI. The orchestration runs
with a stubbed ``ctx.run``.
"""

from __future__ import annotations

import copy
import json
import tarfile
from collections.abc import Callable
from pathlib import Path
from typing import Any

import pytest

from toolr.testing import make_command_result
from toolr.testing import make_context
from toolr.types import ResolvedPath
from toolr.utils.command import CommandResult

from tools.ci import PACKSLIP_PROJECT
from tools.ci import PACKSLIP_SOURCE_REPO
from tools.ci import PackslipFailure
from tools.ci import _packslip_statement_failures
from tools.ci import packslip_check

REPO_ROOT = Path(__file__).resolve().parents[3]
StatementFactory = Callable[..., dict[str, Any]]


def _checks(failures: list[PackslipFailure]) -> list[str]:
    return [f.check for f in failures]


@pytest.fixture
def statement() -> StatementFactory:
    """Factory: a passing packslip statement, with artifacts/resources overridable."""

    default_artifacts = [
        {
            "name": "crook-0.0.0-x86_64-unknown-linux-musl.tar.gz",
            "os": "linux",
            "libc": "musl",
            "bin": ["crook-0.0.0-x86_64-unknown-linux-musl/crook"],
        },
        {
            "name": "crook-0.0.0-aarch64-apple-darwin.tar.gz",
            "os": "macos",
            "bin": [{"path": "crook-0.0.0-aarch64-apple-darwin/crook", "name": "crook"}],
        },
    ]
    default_resources = [{"kind": "completion", "shells": ["bash", "zsh", "fish"]}]

    def _make(
        *,
        artifacts: list[dict[str, Any]] | None = None,
        resources: list[dict[str, Any]] | None = None,
    ) -> dict[str, Any]:
        return {
            "predicate": {
                "artifacts": copy.deepcopy(default_artifacts) if artifacts is None else artifacts,
                "resources": copy.deepcopy(default_resources) if resources is None else resources,
            }
        }

    return _make


def test_packslip_statement(statement: StatementFactory) -> None:
    """Each broken shape yields exactly one named failure; a good statement yields none."""
    two_bins = [{"name": "a.tar.gz", "os": "macos", "bin": ["d/crook", "e/crook"]}]
    glibc = [{"name": "b.tar.gz", "os": "linux", "libc": "gnu", "bin": ["d/crook"]}]
    skill = [
        {"kind": "completion", "shells": ["bash", "zsh", "fish"]},
        {"kind": "skill", "name": "crook-child", "repo": "plugin/skills/crook-child"},
    ]
    assert _packslip_statement_failures(statement()) == []
    assert _checks(_packslip_statement_failures(statement(artifacts=two_bins))) == [
        "artifact bin entries"
    ]
    assert _checks(_packslip_statement_failures(statement(artifacts=glibc))) == ["linux libc"]
    assert _checks(_packslip_statement_failures(statement(resources=skill))) == ["skill resources"]
    assert _checks(_packslip_statement_failures(statement(resources=[]))) == ["completion resource"]


def test_skill_failure_names_the_skills(statement: StatementFactory) -> None:
    resources = [
        {"kind": "completion", "shells": ["bash", "zsh", "fish"]},
        {"kind": "skill", "name": "beta"},
        {"kind": "skill", "name": "alpha"},
    ]
    (failure,) = _packslip_statement_failures(statement(resources=resources))
    assert "alpha beta" in failure.detail


def test_no_artifacts_fails(statement: StatementFactory) -> None:
    assert _checks(_packslip_statement_failures(statement(artifacts=[]))) == ["artifacts"]


@pytest.mark.parametrize(
    "bin_entries",
    [
        pytest.param(None, id="missing"),
        pytest.param([], id="empty"),
        pytest.param(["a/crook", "b/crook"], id="two"),
    ],
)
def test_bin_entry_count_must_be_one(
    statement: StatementFactory, bin_entries: list[str] | None
) -> None:
    artifact: dict[str, Any] = {"name": "x.tar.gz", "os": "macos"}
    if bin_entries is not None:
        artifact["bin"] = bin_entries
    failures = _packslip_statement_failures(statement(artifacts=[artifact]))
    assert _checks(failures) == ["artifact bin entries"]
    assert "expected exactly one bin entry" in failures[0].detail


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param("dir/crook", id="string"),
        pytest.param({"path": "dir/crook", "name": "crook"}, id="object"),
    ],
)
def test_bin_entry_accepted_shapes(statement: StatementFactory, entry: object) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "macos", "bin": [entry]}]
    assert _packslip_statement_failures(statement(artifacts=artifacts)) == []


@pytest.mark.parametrize(
    "entry",
    [
        pytest.param("dir/tool", id="wrong-name"),
        pytest.param("dir/crook.exe", id="windows-suffix"),
        pytest.param("crook", id="no-directory"),
        pytest.param({"path": "dir/other", "name": "crook"}, id="object-wrong-path"),
        pytest.param({"name": "crook"}, id="object-no-path"),
        pytest.param({"path": None}, id="object-null-path"),
    ],
)
def test_bin_entry_must_end_in_crook(statement: StatementFactory, entry: object) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "macos", "bin": [entry]}]
    failures = _packslip_statement_failures(statement(artifacts=artifacts))
    assert _checks(failures) == ["artifact bin entries"]
    assert "does not end in /crook" in failures[0].detail


def test_bin_entry_must_be_a_list(statement: StatementFactory) -> None:
    artifacts = statement()["predicate"]["artifacts"]
    artifacts[0]["bin"] = "crook"
    assert _packslip_statement_failures(statement(artifacts=artifacts)) == [
        PackslipFailure("artifact bin entries", f"{artifacts[0]['name']}: bin is not a list")
    ]


@pytest.mark.parametrize("libc", [None, "gnu"])
def test_linux_artifact_must_be_musl(statement: StatementFactory, libc: str | None) -> None:
    artifact: dict[str, Any] = {"name": "x.tar.gz", "os": "linux", "bin": ["d/crook"]}
    if libc is not None:
        artifact["libc"] = libc
    assert _packslip_statement_failures(statement(artifacts=[artifact])) == [
        PackslipFailure(
            "linux libc", f"x.tar.gz: os=linux but libc={libc or 'null'} (expected musl)"
        )
    ]


@pytest.mark.parametrize(
    "shells",
    [
        pytest.param(None, id="no-completion"),
        pytest.param(["bash", "zsh"], id="missing-shell"),
        pytest.param(["zsh", "bash", "fish"], id="wrong-order"),
        pytest.param(["bash", "zsh", "fish", "nu"], id="extra-shell"),
    ],
)
def test_completion_resource_shells_must_match(
    statement: StatementFactory, shells: list[str] | None
) -> None:
    resources = [] if shells is None else [{"kind": "completion", "shells": shells}]
    failures = _packslip_statement_failures(statement(resources=resources))
    assert _checks(failures) == ["completion resource"]


@pytest.mark.parametrize(
    ("value", "detail"),
    [
        pytest.param([], "statement has no predicate object", id="statement-not-object"),
        pytest.param(
            {"predicate": ["x"]}, "statement has no predicate object", id="predicate-not-object"
        ),
        pytest.param(
            {"predicate": {"artifacts": {"name": "x"}}},
            "predicate.artifacts is not a list of objects",
            id="artifacts-not-list",
        ),
        pytest.param(
            {"predicate": {"artifacts": [{"name": "x"}], "resources": ["skill"]}},
            "predicate.resources is not a list of objects",
            id="resource-not-object",
        ),
    ],
)
def test_unexpected_statement_shape_fails(value: object, detail: str) -> None:
    assert _packslip_statement_failures(value) == [PackslipFailure("statement shape", detail)]


def test_every_failure_is_reported(statement: StatementFactory) -> None:
    artifacts = [{"name": "x.tar.gz", "os": "linux", "bin": ["d/other"]}]
    resources = [{"kind": "skill", "name": "s"}]
    failures = _packslip_statement_failures(statement(artifacts=artifacts, resources=resources))
    assert _checks(failures) == [
        "artifact bin entries",
        "linux libc",
        "skill resources",
        "completion resource",
    ]


def test_constants_name_this_repo() -> None:
    assert PACKSLIP_PROJECT == "github.com/s0undt3ch/shepherds-crook"
    assert PACKSLIP_SOURCE_REPO == "https://github.com/s0undt3ch/shepherds-crook"


def test_packslip_manifest_matches_the_assertions() -> None:
    manifest = (REPO_ROOT / ".github" / "packslip.toml").read_text(encoding="utf-8")
    assert 'bin = ["crook"]' in manifest
    assert 'shells = ["bash", "zsh", "fish"]' in manifest
    assert 'exec = ["crook", "completion", "print", "{shell}"]' in manifest
    assert 'kind = "skill"' not in manifest


@pytest.fixture
def archive_dir(tmp_path: Path) -> ResolvedPath:
    out = tmp_path / "archives"
    out.mkdir()
    binary = tmp_path / "crook"
    binary.write_bytes(b"\x7fELF")
    for triple in ("x86_64-unknown-linux-musl", "aarch64-apple-darwin"):
        stem = f"crook-0.0.0-{triple}"
        with tarfile.open(out / f"{stem}.tar.gz", "w:gz") as tf:
            tf.add(binary, arcname=f"{stem}/crook")
        (out / f"{stem}.tar.gz.sha256").write_text("0" * 64)
    return ResolvedPath(out)


@pytest.fixture
def fake_packslip(tmp_path: Path) -> ResolvedPath:
    path = tmp_path / "bin" / "packslip"
    path.parent.mkdir()
    path.write_text("#!/bin/sh\n")
    path.chmod(0o755)
    return ResolvedPath(path)


@pytest.fixture
def repo_root(tmp_path: Path) -> Path:
    manifest = tmp_path / ".github" / "packslip.toml"
    manifest.parent.mkdir()
    manifest.write_text('bin = ["crook"]\n')
    return tmp_path


def _stub_packslip(
    statement: dict[str, Any], *, fail: str | None = None
) -> Callable[..., CommandResult[str] | CommandResult[bytes]]:
    """A ``ctx.run`` stand-in: git and each packslip subcommand, with one optionally failing."""

    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        if cmdline[0] == "git":
            if fail == "git":
                return make_command_result(args=list(cmdline), stderr="git broke", returncode=1)
            return make_command_result(args=list(cmdline), stdout="a" * 40 + "\n")
        sub = cmdline[1]
        if sub == fail:
            return make_command_result(args=list(cmdline), stderr=f"{sub} broke", returncode=1)
        if sub == "create":
            out = Path(cmdline[cmdline.index("--out") + 1])
            out.mkdir()
            (out / "packslip.sigstore.json").write_text("{}")
        if sub == "show":
            return make_command_result(args=list(cmdline), stdout=json.dumps(statement))
        return make_command_result(args=list(cmdline))

    return _run


def test_command_passes(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
) -> None:
    calls: list[tuple[str, ...]] = []
    stub = _stub_packslip(statement())

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return stub(cmdline, **kwargs)

    ctx = make_context(repo_root, run=_run)
    packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert "packslip-check: ok" in ctx.stdout

    expected = sorted(str(p) for p in archive_dir.iterdir() if not p.name.endswith(".sha256"))
    create = next(c for c in calls if c[1:2] == ("create",))
    assert list(create[-2:]) == expected
    assert create[create.index("--manifest") + 1] == str(repo_root / ".github" / "packslip.toml")
    assert create[create.index("--commit") + 1] == "a" * 40
    assert create[create.index("--project") + 1] == PACKSLIP_PROJECT
    assert create[create.index("--source-repo") + 1] == PACKSLIP_SOURCE_REPO
    verify = next(c for c in calls if c[1:2] == ("verify",))
    assert verify[2].endswith("packslip.sigstore.json")
    assert [verify[i + 1] for i, a in enumerate(verify) if a == "--artifact"] == expected


def test_command_honours_an_explicit_manifest(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
    tmp_path: Path,
) -> None:
    other = tmp_path / "other.toml"
    other.write_text('bin = ["crook"]\n')
    calls: list[tuple[str, ...]] = []
    stub = _stub_packslip(statement())

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return stub(cmdline, **kwargs)

    packslip_check(
        make_context(repo_root, run=_run),
        archive_dir,
        packslip=fake_packslip,
        manifest=ResolvedPath(other),
    )
    create = next(c for c in calls if c[1:2] == ("create",))
    assert create[create.index("--manifest") + 1] == str(other)


@pytest.mark.parametrize("sub", ["keygen", "create", "verify", "show"])
def test_command_names_the_failing_packslip_step(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
    sub: str,
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip(statement(), fail=sub))
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert exc.value.code == 1
    assert f"packslip-check: packslip {sub} failed: {sub} broke" in ctx.stderr


def test_command_fails_when_git_rev_parse_fails(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
) -> None:
    ctx = make_context(repo_root, run=_stub_packslip(statement(), fail="git"))
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert exc.value.code == 1
    assert "packslip-check: git rev-parse HEAD failed: git broke" in ctx.stderr


def test_command_fails_when_create_produces_no_bundle(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
) -> None:
    stub = _stub_packslip(statement())

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        if cmdline[1:2] == ("create",):
            return make_command_result(args=list(cmdline))
        return stub(cmdline, **kwargs)

    ctx = make_context(repo_root, run=_run)
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert exc.value.code == 1
    assert "packslip-check: packslip create failed: expected bundle at" in ctx.stderr


def test_command_fails_when_show_is_not_json(
    repo_root: Path, archive_dir: ResolvedPath, fake_packslip: ResolvedPath
) -> None:
    stub = _stub_packslip({})

    def _run(cmdline: tuple[str, ...], **kwargs: Any) -> CommandResult[str] | CommandResult[bytes]:
        if cmdline[1:2] == ("show",):
            return make_command_result(args=list(cmdline), stdout="not json")
        return stub(cmdline, **kwargs)

    ctx = make_context(repo_root, run=_run)
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert exc.value.code == 1
    assert "packslip-check: packslip show failed: output is not JSON" in ctx.stderr


def test_command_reports_statement_failures(
    statement: StatementFactory,
    repo_root: Path,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
) -> None:
    skill = [{"kind": "skill", "name": "crook-child"}]
    ctx = make_context(repo_root, run=_stub_packslip(statement(resources=skill)))
    with pytest.raises(SystemExit) as exc:
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert exc.value.code == 1
    assert "packslip-check: skill resources failed" in ctx.stderr
    assert "packslip-check: completion resource failed" in ctx.stderr


def test_command_rejects_a_missing_archive_dir(
    repo_root: Path, fake_packslip: ResolvedPath, tmp_path: Path
) -> None:
    ctx = make_context(repo_root)
    with pytest.raises(SystemExit):
        packslip_check(ctx, ResolvedPath(tmp_path / "nope"), packslip=fake_packslip)
    assert "packslip-check: archives failed" in ctx.stderr


def test_command_rejects_an_empty_archive_dir(
    repo_root: Path, fake_packslip: ResolvedPath, tmp_path: Path
) -> None:
    empty = tmp_path / "empty"
    empty.mkdir()
    ctx = make_context(repo_root)
    with pytest.raises(SystemExit):
        packslip_check(ctx, ResolvedPath(empty), packslip=fake_packslip)
    assert "no *.tar.gz files found" in ctx.stderr


def test_command_rejects_a_missing_packslip_binary(
    repo_root: Path, archive_dir: ResolvedPath, tmp_path: Path
) -> None:
    ctx = make_context(repo_root)
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir, packslip=ResolvedPath(tmp_path / "missing"))
    assert "packslip-check: packslip failed" in ctx.stderr


def test_command_looks_for_packslip_on_path(
    repo_root: Path, archive_dir: ResolvedPath, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("PATH", str(repo_root))
    ctx = make_context(repo_root)
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir)
    assert "packslip not found on PATH" in ctx.stderr


def test_command_rejects_a_missing_manifest(
    statement: StatementFactory,
    archive_dir: ResolvedPath,
    fake_packslip: ResolvedPath,
    tmp_path: Path,
) -> None:
    root = tmp_path / "bare"
    root.mkdir()
    ctx = make_context(root, run=_stub_packslip(statement()))
    with pytest.raises(SystemExit):
        packslip_check(ctx, archive_dir, packslip=fake_packslip)
    assert "packslip-check: manifest failed" in ctx.stderr
