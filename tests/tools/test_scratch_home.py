from __future__ import annotations

import os
import subprocess
from pathlib import Path

from tests.conftest import HOME_VARS


def test_home_is_a_scratch_folder(_scratch_home: Path) -> None:
    assert os.environ["HOME"] == str(_scratch_home)
    assert Path("~").expanduser() == _scratch_home
    assert not any(name in os.environ for name in HOME_VARS)


def test_a_child_process_sees_the_scratch_home(_scratch_home: Path) -> None:
    out = subprocess.run(["sh", "-c", 'printf %s "$HOME"'], capture_output=True, text=True, check=True)  # noqa: S607
    assert out.stdout == str(_scratch_home)


def test_no_mise_shim_is_on_the_path() -> None:
    assert not [e for e in os.environ["PATH"].split(os.pathsep) if e.rstrip("/").endswith("/mise/shims")]
