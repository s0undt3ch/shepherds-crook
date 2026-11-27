"""Tests for ``toolr pre-commit private-words``.

Every test points ``CROOK_PRIVATE_WORDS`` at a temp words file, so none reads the real one.
"""

from __future__ import annotations

import subprocess
from collections.abc import Callable
from pathlib import Path

import pytest

from toolr.testing import make_context

from tools.precommit.private_words import WORDS_ENV
from tools.precommit.private_words import private_words

RepoFactory = Callable[[dict[str, str]], Path]


@pytest.fixture
def words_file(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    path = tmp_path / "private-words"
    monkeypatch.setenv(WORDS_ENV, str(path))
    return path


@pytest.fixture
def make_repo(tmp_path: Path) -> RepoFactory:
    """Factory: a git repo under tmp_path with ``{path: text}`` staged. Returns its root."""

    def factory(files: dict[str, str]) -> Path:
        root = tmp_path / "repo"
        root.mkdir()
        subprocess.run(["git", "init", "-q", str(root)], check=True)  # noqa: S607
        for name, text in files.items():
            (root / name).parent.mkdir(parents=True, exist_ok=True)
            (root / name).write_text(text, encoding="utf-8")
            subprocess.run(["git", "-C", str(root), "add", name], check=True)  # noqa: S607
        return root

    return factory


def test_a_match_fails_with_path_and_line_but_not_the_word(
    words_file: Path, make_repo: RepoFactory
) -> None:
    words_file.write_text("secretword\n", encoding="utf-8")
    root = make_repo({"docs/notes.txt": "clean\nthe SecretWord is here\n", "ok.txt": "fine\n"})
    ctx = make_context(root)
    with pytest.raises(SystemExit) as exc:
        private_words(ctx)
    assert exc.value.code == 1
    output = ctx.stdout + ctx.stderr
    assert "docs/notes.txt:2" in output
    assert "ok.txt" not in output
    assert "secretword" not in output.lower()


def test_each_matching_line_is_reported_once(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("secretword\nother[0-9]\n", encoding="utf-8")
    root = make_repo({"a.txt": "secretword other1\nnothing\nother2\n"})
    ctx = make_context(root)
    with pytest.raises(SystemExit):
        private_words(ctx)
    assert (ctx.stdout + ctx.stderr).count("a.txt:1") == 1
    assert "a.txt:3" in ctx.stdout + ctx.stderr


def test_no_match_passes(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("secretword\n", encoding="utf-8")
    ctx = make_context(make_repo({"a.txt": "nothing to see\n"}))
    private_words(ctx)
    assert "private-words: ok" in ctx.stdout + ctx.stderr


def test_a_missing_words_file_passes_with_a_notice(
    words_file: Path, make_repo: RepoFactory
) -> None:
    assert not words_file.exists()
    ctx = make_context(make_repo({"a.txt": "secretword\n"}))
    private_words(ctx)
    assert "no words file" in ctx.stdout + ctx.stderr


def test_the_default_location_is_under_the_home_config(
    make_repo: RepoFactory, monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.delenv(WORDS_ENV, raising=False)
    monkeypatch.setenv("HOME", str(tmp_path / "home"))
    ctx = make_context(make_repo({"a.txt": "secretword\n"}))
    private_words(ctx)
    assert ".config/crook-dev/private-words" in ctx.stdout + ctx.stderr


def test_blank_lines_in_the_words_file_match_nothing(
    words_file: Path, make_repo: RepoFactory
) -> None:
    words_file.write_text("\nsecretword\n\n", encoding="utf-8")
    ctx = make_context(make_repo({"a.txt": "innocent\n"}))
    private_words(ctx)


def test_an_empty_words_file_passes(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("\n", encoding="utf-8")
    private_words(make_context(make_repo({"a.txt": "innocent\n"})))


def test_untracked_files_are_not_searched(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("secretword\n", encoding="utf-8")
    root = make_repo({"a.txt": "innocent\n"})
    (root / "scratch.txt").write_text("secretword\n", encoding="utf-8")
    private_words(make_context(root))


def test_a_binary_match_fails_with_the_path(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("secretword\n", encoding="utf-8")
    root = make_repo({})
    (root / "blob.bin").write_bytes(b"\x00secretword\x00")
    subprocess.run(["git", "-C", str(root), "add", "blob.bin"], check=True)  # noqa: S607
    ctx = make_context(root)
    with pytest.raises(SystemExit):
        private_words(ctx)
    output = ctx.stdout + ctx.stderr
    assert "blob.bin" in output
    assert "secretword" not in output


def test_a_bad_pattern_fails_without_echoing_it(words_file: Path, make_repo: RepoFactory) -> None:
    words_file.write_text("(secretword\n", encoding="utf-8")
    ctx = make_context(make_repo({"a.txt": "innocent\n"}))
    with pytest.raises(SystemExit) as exc:
        private_words(ctx)
    assert exc.value.code == 1
    output = ctx.stdout + ctx.stderr
    assert "git grep failed" in output
    assert "secretword" not in output
