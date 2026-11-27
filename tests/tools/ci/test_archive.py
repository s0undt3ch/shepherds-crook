"""Tests for ``toolr ci stage-archive`` and ``toolr ci archive-check`` in ``tools/ci.py``."""

from __future__ import annotations

import io
import shutil
import subprocess
import tarfile
from pathlib import Path

import pytest
from toolr.testing import make_context
from toolr.types import ResolvedPath

from tools.ci import archive_check
from tools.ci import check_archive
from tools.ci import pack_archive
from tools.ci import stage_archive
from tools.ci import stage_archive_folder

TRIPLE = "x86_64-unknown-linux-musl"
BASE = f"crook-0.0.0-ci-{TRIPLE}"


def _git(root: Path, *args: str) -> None:
    subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)  # noqa: S603, S607


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    root = tmp_path / "repo"
    (root / "plugin/.claude-plugin").mkdir(parents=True)
    (root / "plugin/.claude-plugin/plugin.json").write_text("{}")
    (root / "plugin/skills").mkdir()
    (root / "plugin/skills/a.md").write_text("a")
    (root / "mod").mkdir()
    (root / "mod/mod.json").write_text("{}")
    (root / "mod/lib.js").write_text("")
    (root / "LICENSE").write_text("license")
    (root / "README.md").write_text("readme")
    _git(root, "init", "-q")
    _git(root, "add", "plugin", "mod")
    # What a working tree collects and git does not track.
    (root / "plugin/skills/.DS_Store").write_text("junk")
    (root / "mod/gpui-kit.d.ts").write_text("types")
    (root / "mod/jsconfig.json").write_text("{}")
    (root / "mod/main.js~").write_text("backup")
    (root / "mod/leak").symlink_to("/etc/passwd")
    return root


@pytest.fixture
def binary(tmp_path: Path) -> Path:
    path = tmp_path / "crook"
    path.write_bytes(b"\x7fELF")
    path.chmod(0o600)
    return path


def test_stage_lays_out_the_archive(repo: Path, binary: Path, tmp_path: Path) -> None:
    folder = stage_archive_folder(repo, binary, tmp_path / "out", version="0.0.0-ci", triple=TRIPLE)
    assert folder == tmp_path / "out" / BASE
    assert sorted(p.name for p in folder.iterdir()) == ["LICENSE", "README.md", "crook", "mod", "plugin"]
    assert (folder / "crook").stat().st_mode & 0o111
    assert (folder / "plugin/.claude-plugin/plugin.json").is_file()
    assert (folder / "mod/lib.js").is_file()
    assert not (folder / "plugin/skills/.DS_Store").exists()


def test_stage_ships_only_what_git_tracks(repo: Path, binary: Path, tmp_path: Path) -> None:
    folder = stage_archive_folder(repo, binary, tmp_path / "out", version="0.0.0-ci", triple=TRIPLE)
    shipped = sorted(str(p.relative_to(folder)) for p in folder.rglob("*") if not p.is_dir())
    assert shipped == [
        "LICENSE",
        "README.md",
        "crook",
        "mod/lib.js",
        "mod/mod.json",
        "plugin/.claude-plugin/plugin.json",
        "plugin/skills/a.md",
    ]


def test_stage_refuses_a_tracked_symlink(repo: Path, binary: Path, tmp_path: Path) -> None:
    _git(repo, "add", "-f", "mod/leak")
    with pytest.raises(ValueError, match="symbolic link"):
        stage_archive_folder(repo, binary, tmp_path / "out", version="0.0.0-ci", triple=TRIPLE)


def test_stage_refuses_outside_a_git_repository(repo: Path, binary: Path, tmp_path: Path) -> None:
    standalone = tmp_path / "plain"
    shutil.copytree(repo, standalone, symlinks=True, ignore=shutil.ignore_patterns(".git"))
    with pytest.raises(ValueError, match="cannot list"):
        stage_archive_folder(standalone, binary, tmp_path / "out", version="0.0.0-ci", triple=TRIPLE)


def test_packing_is_reproducible(repo: Path, binary: Path, tmp_path: Path) -> None:
    folder = stage_archive_folder(repo, binary, tmp_path / "out", version="0.0.0-ci", triple=TRIPLE)
    first = pack_archive(folder, tmp_path / "a", mtime=1_700_000_000).read_bytes()
    for path in folder.rglob("*"):
        path.touch()
    second = pack_archive(folder, tmp_path / "b", mtime=1_700_000_000).read_bytes()
    assert first == second
    with tarfile.open(fileobj=io.BytesIO(first)) as tf:
        assert {m.mtime for m in tf.getmembers()} == {1_700_000_000}


def test_stage_adds_the_changelog_when_present(repo: Path, binary: Path, tmp_path: Path) -> None:
    (repo / "CHANGELOG.md").write_text("log")
    folder = stage_archive_folder(repo, binary, tmp_path / "out", version="1.2.3", triple=TRIPLE)
    assert (folder / "CHANGELOG.md").read_text() == "log"


def test_stage_replaces_an_earlier_staging(repo: Path, binary: Path, tmp_path: Path) -> None:
    out = tmp_path / "out"
    first = stage_archive_folder(repo, binary, out, version="1.2.3", triple=TRIPLE)
    (first / "stale").write_text("x")
    stage_archive_folder(repo, binary, out, version="1.2.3", triple=TRIPLE)
    assert not (first / "stale").exists()


@pytest.mark.parametrize(
    ("missing", "version", "triple"),
    [
        pytest.param("plugin/.claude-plugin/plugin.json", "1.0.0", TRIPLE, id="no-plugin-json"),
        pytest.param("mod/mod.json", "1.0.0", TRIPLE, id="no-mod-json"),
        pytest.param("LICENSE", "1.0.0", TRIPLE, id="no-license"),
        pytest.param(None, "v1", TRIPLE, id="bad-version"),
        pytest.param(None, "1.0.0", "linux", id="bad-triple"),
        pytest.param(None, "1.0.0/../x", TRIPLE, id="path-in-version"),
    ],
)
def test_stage_refuses_a_broken_source(
    repo: Path, binary: Path, tmp_path: Path, missing: str | None, version: str, triple: str
) -> None:
    if missing:
        (repo / missing).unlink()
    with pytest.raises(ValueError, match=r"."):
        stage_archive_folder(repo, binary, tmp_path / "out", version=version, triple=triple)


def test_stage_refuses_a_missing_binary(repo: Path, tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="does not exist"):
        stage_archive_folder(repo, tmp_path / "nope", tmp_path / "out", version="1.0.0", triple=TRIPLE)


def test_command_stages_and_packs(repo: Path, binary: Path, tmp_path: Path) -> None:
    ctx = make_context(repo)
    dist = tmp_path / "dist"
    stage_archive(
        ctx,
        TRIPLE,
        "0.0.0-ci",
        binary=ResolvedPath(binary),
        out=str(tmp_path / "out"),
        dist=str(dist),
    )
    assert sorted(p.name for p in dist.iterdir()) == [f"{BASE}.tar.gz", f"{BASE}.tar.gz.sha256"]
    assert check_archive(dist / f"{BASE}.tar.gz") == []
    archive_check(make_context(repo), ResolvedPath(dist))


def test_command_defaults_to_the_repo_folders(repo: Path, binary: Path) -> None:
    target = repo / "target" / TRIPLE / "release"
    target.mkdir(parents=True)
    (target / "crook").write_bytes(binary.read_bytes())
    stage_archive(make_context(repo), TRIPLE, "0.0.0-ci")
    assert (repo / "stage" / BASE / "crook").is_file()


def test_command_exits_on_a_broken_source(repo: Path, binary: Path) -> None:
    (repo / "mod/mod.json").unlink()
    ctx = make_context(repo)
    with pytest.raises(SystemExit) as exc:
        stage_archive(ctx, TRIPLE, "0.0.0-ci", binary=ResolvedPath(binary))
    assert exc.value.code == 1
    assert "stage-archive:" in ctx.stderr
    assert "mod.json" in ctx.stderr


def _tar(path: Path, files: dict[str, int | None], *, extra: dict[str, bytes] | None = None) -> Path:
    """A tarball holding each name with its mode; a `None` mode makes a directory."""
    with tarfile.open(path, "w:gz") as tf:
        for name, mode in files.items():
            info = tarfile.TarInfo(name)
            if mode is None:
                info.type = tarfile.DIRTYPE
                info.mode = 0o755
                tf.addfile(info)
            else:
                info.mode = mode
                tf.addfile(info, io.BytesIO(b"x"))
        for name, data in (extra or {}).items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            tf.addfile(info, io.BytesIO(data))
    return path


GOOD = {
    BASE: None,
    f"{BASE}/crook": 0o755,
    f"{BASE}/plugin/.claude-plugin/plugin.json": 0o644,
    f"{BASE}/mod/mod.json": 0o644,
}


def test_a_good_tarball_passes(tmp_path: Path) -> None:
    assert check_archive(_tar(tmp_path / f"{BASE}.tar.gz", GOOD)) == []


def test_a_tarball_without_mod_fails(tmp_path: Path) -> None:
    files = {k: v for k, v in GOOD.items() if "mod/" not in k}
    (error,) = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert f"{BASE}/mod/mod.json is missing" in error


def test_a_tarball_without_plugin_fails(tmp_path: Path) -> None:
    files = {k: v for k, v in GOOD.items() if "plugin/" not in k}
    (error,) = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert "plugin.json is missing" in error


def test_a_tarball_with_two_top_folders_fails(tmp_path: Path) -> None:
    files = {**GOOD, "other/crook": 0o755}
    (error,) = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert "expected one top folder" in error


def test_a_tarball_with_files_at_the_top_fails(tmp_path: Path) -> None:
    files = {"crook": 0o755, "plugin/.claude-plugin/plugin.json": 0o644, "mod/mod.json": 0o644}
    errors = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert any("expected one top folder" in e for e in errors)


def test_a_misnamed_top_folder_fails(tmp_path: Path) -> None:
    files = {k.replace(BASE, "crook"): v for k, v in GOOD.items()}
    (error,) = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert "expected" in error
    assert BASE in error


def test_a_non_executable_binary_fails(tmp_path: Path) -> None:
    files = {**GOOD, f"{BASE}/crook": 0o644}
    (error,) = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", files))
    assert "not executable" in error


@pytest.mark.parametrize("name", ["../evil", "/etc/evil"])
def test_a_member_leaving_the_folder_fails(tmp_path: Path, name: str) -> None:
    errors = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", {**GOOD, name: 0o644}))
    assert any("leaves the archive folder" in e for e in errors)


def test_an_unreadable_tarball_fails(tmp_path: Path) -> None:
    bad = tmp_path / f"{BASE}.tar.gz"
    bad.write_bytes(b"not a tarball")
    (error,) = check_archive(bad)
    assert "cannot read the tarball" in error


def test_command_reports_every_bad_tarball(tmp_path: Path) -> None:
    out = tmp_path / "out"
    out.mkdir()
    _tar(out / f"{BASE}.tar.gz", GOOD)
    _tar(out / "crook-1.0.0-aarch64-apple-darwin.tar.gz", {"crook-1.0.0-aarch64-apple-darwin/crook": 0o755})
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit) as exc:
        archive_check(ctx, ResolvedPath(out))
    assert exc.value.code == 1
    assert "aarch64-apple-darwin" in ctx.stderr
    assert "mod/mod.json is missing" in ctx.stderr


def test_a_symlink_or_hardlink_member_fails(tmp_path: Path) -> None:
    for kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
        path = tmp_path / f"{BASE}-{kind.decode()}.tar.gz"
        with tarfile.open(path, "w:gz") as tf:
            for name, mode in GOOD.items():
                info = tarfile.TarInfo(name)
                info.mode = mode or 0o755
                if mode is None:
                    info.type = tarfile.DIRTYPE
                    tf.addfile(info)
                else:
                    tf.addfile(info, io.BytesIO(b"x"))
            link = tarfile.TarInfo(f"{BASE}/mod/leak")
            link.type = kind
            link.linkname = "/etc/passwd" if kind == tarfile.SYMTYPE else f"{BASE}/crook"
            tf.addfile(link)
        assert any("link or a special file" in e for e in check_archive(path))


@pytest.mark.parametrize("name", ["mod/gpui-kit.d.ts", "mod/jsconfig.json", "plugin/.DS_Store", "mod/main.js~"])
def test_a_stray_file_fails(tmp_path: Path, name: str) -> None:
    errors = check_archive(_tar(tmp_path / f"{BASE}.tar.gz", {**GOOD, f"{BASE}/{name}": 0o644}))
    assert any("stray file" in e for e in errors)


def test_command_requires_the_sha256_file(tmp_path: Path) -> None:
    out = tmp_path / "out"
    out.mkdir()
    _tar(out / f"{BASE}.tar.gz", GOOD)
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit):
        archive_check(ctx, ResolvedPath(out))
    assert f"{BASE}.tar.gz.sha256: is missing" in ctx.stderr


def test_command_checks_the_sha256_file(tmp_path: Path) -> None:
    out = tmp_path / "out"
    out.mkdir()
    _tar(out / f"{BASE}.tar.gz", GOOD)
    (out / f"{BASE}.tar.gz.sha256").write_text("0" * 64 + f"  {BASE}.tar.gz\n")
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit):
        archive_check(ctx, ResolvedPath(out))
    assert "does not match" in ctx.stderr


@pytest.mark.parametrize("make", [False, True])
def test_command_rejects_a_missing_or_empty_folder(tmp_path: Path, make: bool) -> None:
    out = tmp_path / "out"
    if make:
        out.mkdir()
    ctx = make_context(tmp_path)
    with pytest.raises(SystemExit):
        archive_check(ctx, ResolvedPath(out))
    assert "archive-check: archives failed" in ctx.stderr
