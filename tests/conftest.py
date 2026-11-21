from __future__ import annotations

import os
from collections.abc import Iterator

import pytest

from toolr._decorators import _get_command_group_storage


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
