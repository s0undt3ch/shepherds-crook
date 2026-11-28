"""Tests for ``toolr release bump`` in ``tools/release.py``.

Every test works on copies in a temp folder: none points at the repository's own ``Cargo.toml``. The
validation and the ``cargo update`` call run against a stubbed ``ctx.run``; two tests run the real ``cargo
update`` on a temp crate, so they prove ``Cargo.toml`` and ``Cargo.lock`` move together, also from a final
version down to its own release candidate, which ``cargo set-version`` refuses. The plugin stamp runs on a copy
of the real ``plugin.json``.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import pytest

from toolr.testing import make_command_result
from toolr.testing import make_context
from toolr.types import ResolvedPath
from toolr.utils.command import CommandResult

from tools.release import DEV_VERSION
from tools.release import PLUGIN_MANIFEST
from tools.release import REPO_ROOT
from tools.release import bump
from tools.release import dev_stamp
from tools.release import stamp_cargo_version
from tools.release import stamp_plugin_version

CARGO_TOML = """\
[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
"""

needs_cargo = pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo is not installed")

Calls = list[tuple[str, ...]]


@pytest.fixture
def crate(tmp_path: Path) -> Path:
    """A minimal crate with a lock file, so `cargo update` has both files to move."""
    (tmp_path / "src").mkdir()
    (tmp_path / "src" / "lib.rs").write_text("")
    (tmp_path / "Cargo.toml").write_text(CARGO_TOML, encoding="utf-8")
    subprocess.run(  # noqa: S603
        ["cargo", "generate-lockfile", "--offline"],  # noqa: S607
        cwd=tmp_path,
        check=True,
        capture_output=True,
    )
    return tmp_path


@pytest.fixture
def plugin_json(tmp_path: Path) -> Path:
    """A copy of the real plugin.json, which a test may stamp."""
    dest = tmp_path / "plugin.json"
    shutil.copy(PLUGIN_MANIFEST, dest)
    return dest


@pytest.fixture
def cargo_toml(tmp_path: Path) -> Path:
    path = tmp_path / "Cargo.toml"
    path.write_text(CARGO_TOML, encoding="utf-8")
    return path


def _version_of(crate: Path) -> str:
    return tomllib.loads((crate / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]


def _locked_version(crate: Path) -> str:
    lock = tomllib.loads((crate / "Cargo.lock").read_text(encoding="utf-8"))
    return next(p["version"] for p in lock["package"] if p["name"] == "demo")


def _plugin_version(path: Path) -> str:
    return json.loads(path.read_text(encoding="utf-8"))["version"]


def _recording_ctx(tmp_path: Path, calls: Calls, returncode: int = 0):
    def _run(cmdline: tuple[str, ...], **_: Any) -> CommandResult[str] | CommandResult[bytes]:
        calls.append(cmdline)
        return make_command_result(args=list(cmdline), returncode=returncode)

    return make_context(tmp_path, run=_run)


@needs_cargo
@pytest.mark.parametrize("version", ["1.2.3", "1.0.0-rc.1", "0.1.0-rc.1", "10.20.30-rc.12"])
def test_bump_updates_cargo_toml_cargo_lock_and_plugin_json(
    crate: Path, plugin_json: Path, version: str
) -> None:
    ctx = make_context(crate)
    bump(
        ctx,
        version,
        manifest=ResolvedPath(crate / "Cargo.toml"),
        plugin_manifest=ResolvedPath(plugin_json),
    )
    assert _version_of(crate) == version
    assert _locked_version(crate) == version
    assert _plugin_version(plugin_json) == version


@pytest.mark.parametrize(
    "version",
    [
        "1.2",
        "v1.2.3",
        "1.2.3.4",
        "1.2.3rc1",
        "1.2.3-rc1",
        "1.2.3-beta.1",
        "1.2.3-rc.",
        "01.2.3",
        "",
        "latest",
    ],
)
def test_bump_refuses_anything_but_x_y_z_or_x_y_z_rc_n(
    version: str, tmp_path: Path, plugin_json: Path, cargo_toml: Path
) -> None:
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls)
    with pytest.raises(SystemExit) as exc:
        bump(
            ctx,
            version,
            manifest=ResolvedPath(cargo_toml),
            plugin_manifest=ResolvedPath(plugin_json),
        )
    assert exc.value.code == 1
    assert f"{version!r}" in ctx.stderr
    assert calls == []
    assert _version_of(cargo_toml.parent) == "0.1.0"


@needs_cargo
@pytest.mark.parametrize("version", ["0.0.1-dev3", "0.1.1-dev0+gabc1234", "0.1.0-rc.2.dev5"])
def test_bump_takes_a_dev_version_with_dev(crate: Path, plugin_json: Path, version: str) -> None:
    ctx = make_context(crate)
    bump(ctx, version, dev=True, manifest=ResolvedPath(crate / "Cargo.toml"), plugin_manifest=ResolvedPath(plugin_json))
    assert _version_of(crate) == version
    assert _plugin_version(plugin_json) == version


@pytest.mark.parametrize(("version", "dev"), [("0.0.1-dev3", False), ("1.2.3", True), ("0.1.1-dev1+gXYZ1234", True)])
def test_a_dev_version_needs_dev_and_dev_takes_only_a_dev_version(
    version: str, dev: bool, tmp_path: Path, plugin_json: Path, cargo_toml: Path
) -> None:
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls)
    with pytest.raises(SystemExit):
        bump(ctx, version, dev=dev, manifest=ResolvedPath(cargo_toml), plugin_manifest=ResolvedPath(plugin_json))
    assert calls == []


def test_bump_syncs_the_lock_file_of_the_manifest_it_was_given(
    tmp_path: Path, plugin_json: Path, cargo_toml: Path
) -> None:
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls)
    bump(ctx, "0.2.0", manifest=ResolvedPath(cargo_toml), plugin_manifest=ResolvedPath(plugin_json))
    assert calls == [
        ("cargo", "fetch", "--locked", "--manifest-path", str(cargo_toml)),
        ("cargo", "update", "--workspace", "--offline", "--manifest-path", str(cargo_toml)),
    ]
    assert _version_of(cargo_toml.parent) == "0.2.0"
    assert _plugin_version(plugin_json) == "0.2.0"


def test_bump_fetches_before_it_stamps_and_stops_when_the_fetch_fails(
    tmp_path: Path, plugin_json: Path, cargo_toml: Path
) -> None:
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls, returncode=101)
    with pytest.raises(SystemExit) as exc:
        bump(ctx, "0.2.0", manifest=ResolvedPath(cargo_toml), plugin_manifest=ResolvedPath(plugin_json))
    assert exc.value.code == 101
    assert "cargo fetch failed" in ctx.stderr
    assert [c[:2] for c in calls] == [("cargo", "fetch")]
    assert _version_of(cargo_toml.parent) == "0.1.0"
    assert _plugin_version(plugin_json) == DEV_VERSION


@needs_cargo
def test_bump_works_on_a_crate_with_a_dependency_and_an_empty_cargo_home(
    tmp_path: Path, plugin_json: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A path dependency stands in for a registry one, which would need the network."""
    monkeypatch.setenv("CARGO_HOME", str(tmp_path / "cold-cargo-home"))
    dep = tmp_path / "dep"
    (dep / "src").mkdir(parents=True)
    (dep / "src" / "lib.rs").write_text("")
    (dep / "Cargo.toml").write_text('[package]\nname = "dep"\nversion = "0.1.0"\nedition = "2024"\n')
    app = tmp_path / "app"
    (app / "src").mkdir(parents=True)
    (app / "src" / "lib.rs").write_text("")
    (app / "Cargo.toml").write_text(
        CARGO_TOML.replace("[dependencies]\n", '[dependencies]\ndep = { path = "../dep" }\n'),
        encoding="utf-8",
    )
    subprocess.run(  # noqa: S603
        ["cargo", "generate-lockfile", "--offline"],  # noqa: S607
        cwd=app,
        check=True,
        capture_output=True,
    )
    bump(
        make_context(app),
        "0.1.0-rc.1",
        manifest=ResolvedPath(app / "Cargo.toml"),
        plugin_manifest=ResolvedPath(plugin_json),
    )
    assert _version_of(app) == "0.1.0-rc.1"
    assert _locked_version(app) == "0.1.0-rc.1"


def test_bump_passes_cargo_failure_through_and_leaves_plugin_json_alone(
    tmp_path: Path, plugin_json: Path, cargo_toml: Path
) -> None:
    before = plugin_json.read_text(encoding="utf-8")
    ctx = _recording_ctx(tmp_path, [], returncode=3)
    with pytest.raises(SystemExit) as exc:
        bump(ctx, "0.2.0", manifest=ResolvedPath(cargo_toml), plugin_manifest=ResolvedPath(plugin_json))
    assert exc.value.code == 3
    assert "cargo fetch failed" in ctx.stderr
    assert plugin_json.read_text(encoding="utf-8") == before


def test_bump_refuses_before_any_write_when_plugin_json_is_missing(
    tmp_path: Path, cargo_toml: Path
) -> None:
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls)
    with pytest.raises(SystemExit) as exc:
        bump(
            ctx,
            "0.2.0",
            manifest=ResolvedPath(cargo_toml),
            plugin_manifest=ResolvedPath(tmp_path / "nope.json"),
        )
    assert exc.value.code == 1
    assert "does not exist" in ctx.stderr
    assert calls == []
    assert _version_of(cargo_toml.parent) == "0.1.0"


def test_bump_refuses_before_any_write_when_plugin_json_has_no_version(
    tmp_path: Path, cargo_toml: Path
) -> None:
    plugin = tmp_path / "plugin.json"
    plugin.write_text('{"name": "crook"}\n', encoding="utf-8")
    calls: Calls = []
    ctx = _recording_ctx(tmp_path, calls)
    with pytest.raises(SystemExit) as exc:
        bump(ctx, "0.2.0", manifest=ResolvedPath(cargo_toml), plugin_manifest=ResolvedPath(plugin))
    assert exc.value.code == 1
    assert "no version line" in ctx.stderr
    assert calls == []
    assert _version_of(cargo_toml.parent) == "0.1.0"


def test_stamp_plugin_changes_only_the_version_line(plugin_json: Path) -> None:
    before = plugin_json.read_text(encoding="utf-8").splitlines()
    stamp_plugin_version(plugin_json, "1.2.3-rc.4")
    after = plugin_json.read_text(encoding="utf-8").splitlines()
    changed = [(b, a) for b, a in zip(before, after, strict=True) if b != a]
    assert len(changed) == 1
    assert '"version"' in changed[0][0]
    assert changed[0][1].strip() == '"version": "1.2.3-rc.4",'
    assert _plugin_version(plugin_json) == "1.2.3-rc.4"


def test_stamp_plugin_refuses_a_file_without_a_version(tmp_path: Path) -> None:
    path = tmp_path / "plugin.json"
    path.write_text('{"name": "crook"}\n', encoding="utf-8")
    with pytest.raises(ValueError, match='no "version" line'):
        stamp_plugin_version(path, "1.0.0")


def test_stamp_cargo_changes_only_the_package_version(tmp_path: Path) -> None:
    path = tmp_path / "Cargo.toml"
    path.write_text(
        '[package]\nname = "demo"\nversion = "0.1.0"\n\n[dependencies]\nserde = { version = "1" }\n'
        '\n[lints.clippy]\nversion = "kept"\n',
        encoding="utf-8",
    )
    stamp_cargo_version(path, "0.1.0-rc.1")
    assert path.read_text(encoding="utf-8") == (
        '[package]\nname = "demo"\nversion = "0.1.0-rc.1"\n\n[dependencies]\nserde = { version = "1" }\n'
        '\n[lints.clippy]\nversion = "kept"\n'
    )


def test_stamp_cargo_refuses_a_manifest_whose_package_has_no_version(tmp_path: Path) -> None:
    path = tmp_path / "Cargo.toml"
    path.write_text('[package]\nname = "demo"\n\n[dependencies]\nserde = { version = "1" }\nversion = "9"\n')
    with pytest.raises(ValueError, match="no .package. version line"):
        stamp_cargo_version(path, "1.0.0")


def test_the_stamps_read_the_committed_manifests(tmp_path: Path) -> None:
    """The patterns find a version in the committed files, so a release's bump cannot meet a shape it cannot read."""
    cargo = tmp_path / "Cargo.toml"
    shutil.copy(REPO_ROOT / "Cargo.toml", cargo)
    stamp_cargo_version(cargo, "9.9.9-rc.9")
    assert tomllib.loads(cargo.read_text(encoding="utf-8"))["package"]["version"] == "9.9.9-rc.9"
    plugin = tmp_path / "plugin.json"
    shutil.copy(PLUGIN_MANIFEST, plugin)
    stamp_plugin_version(plugin, "9.9.9-rc.9")
    assert _plugin_version(plugin) == "9.9.9-rc.9"


def test_dev_stamp_puts_the_plugin_back_to_the_dev_version(tmp_path: Path, plugin_json: Path) -> None:
    stamp_plugin_version(plugin_json, "0.1.0-rc.1")
    ctx = make_context(tmp_path)
    dev_stamp(ctx, plugin_manifest=ResolvedPath(plugin_json))
    assert _plugin_version(plugin_json) == DEV_VERSION == "0.0.0-dev"


def test_dev_stamp_refuses_a_missing_file(tmp_path: Path) -> None:
    with pytest.raises(SystemExit) as exc:
        dev_stamp(make_context(tmp_path), plugin_manifest=ResolvedPath(tmp_path / "nope.json"))
    assert exc.value.code == 1


def test_main_carries_the_dev_stamp() -> None:
    """Q17: a release commit is stamped and tagged, then a commit on top puts `0.0.0-dev` back.

    The binary and the plugin agree on the release commit (`bump` leaves them equal, tested above); on
    `main` the plugin reads the dev stamp, which the open and status tests and a debug build rely on.
    """
    assert _plugin_version(PLUGIN_MANIFEST) == DEV_VERSION
