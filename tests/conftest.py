from __future__ import annotations

import os
import shutil
import subprocess
from collections.abc import Iterator
from pathlib import Path

import pytest

from toolr._decorators import _get_command_group_storage


# Every folder a tool might write to under the developer's home. Tests get scratch ones, so nothing a test
# runs can reach the real home (build log D118).
HOME_VARS = ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "CLAUDE_CONFIG_DIR", "HERDR_APP_HOME")




def _path_without_shims() -> str:
    """Return PATH with mise's shims folder swapped for the install folders it would resolve to.

    A shim finds its tools through HOME, so under a scratch home it fails or reads the developer's mise
    config as a project config. The install folders are worked out here, with the real HOME, once.
    """
    entries = os.environ.get("PATH", "").split(os.pathsep)
    if not any(e.rstrip("/").endswith("/mise/shims") for e in entries):
        return os.pathsep.join(entries)
    mise = shutil.which("mise")
    found = subprocess.run([mise, "bin-paths"], capture_output=True, text=True, check=False) if mise else None  # noqa: S603
    installs = found.stdout.split() if found and found.returncode == 0 else []
    out: list[str] = []
    for entry in entries:
        if entry.rstrip("/").endswith("/mise/shims"):
            out.extend(i for i in installs if i not in out and i not in entries)
        else:
            out.append(entry)
    return os.pathsep.join(out)


def _toolchain_homes() -> dict[str, str]:
    """Return cargo's and rustup's folders as they are under the real HOME.

    They are toolchain caches, like mise's install folders, and cargo needs its registry cache to work offline.
    """
    real = Path("~").expanduser()
    homes = {"CARGO_HOME": real / ".cargo", "RUSTUP_HOME": real / ".rustup"}
    return {name: os.environ.get(name) or str(path) for name, path in homes.items()}


@pytest.fixture(scope="session")
def _test_path() -> str:
    return _path_without_shims()


@pytest.fixture(scope="session")
def _test_toolchain() -> dict[str, str]:
    return _toolchain_homes()


@pytest.fixture(autouse=True)
def _scratch_home(
    tmp_path_factory: pytest.TempPathFactory,
    monkeypatch: pytest.MonkeyPatch,
    _test_path: str,
    _test_toolchain: dict[str, str],
) -> Path:
    """Point HOME and the XDG, Claude and herdr-app folders at a scratch home for every test."""
    home = tmp_path_factory.mktemp("home")
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setenv("PATH", _test_path)
    for name, value in _test_toolchain.items():
        monkeypatch.setenv(name, value)
    for name in HOME_VARS:
        monkeypatch.delenv(name, raising=False)
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    return home


@pytest.fixture(autouse=True)
def _clean_registry() -> Iterator[None]:
    """Give every test an empty process-wide command-group registry.

    Snapshots the storage, clears it, and restores the snapshot afterwards,
    so a ``command_group`` call in one test can't be returned by the next.
    ``CommandsTester`` patches the storage function per test and is
    unaffected: this fixture sets up first and tears down last.
    """
    storage = _get_command_group_storage()
    saved = dict(storage)
    storage.clear()
    yield
    storage.clear()
    storage.update(saved)


@pytest.fixture(autouse=True)
def _restore_cwd() -> Iterator[None]:
    """Put the process back in its starting directory after every test.

    Guards against tests, and the code they call, that `os.chdir` without `monkeypatch`, so test
    authors need no chdir cleanup of their own.
    """
    original = os.getcwd()
    yield
    os.chdir(original)
