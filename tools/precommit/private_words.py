"""
Pre-commit hook command that keeps private words out of the tracked tree.
"""

from __future__ import annotations

import os
import re
import tempfile
from pathlib import Path

from toolr import Context

from ._common import group

WORDS_ENV = "CROOK_PRIVATE_WORDS"
DEFAULT_WORDS = "~/.config/crook-dev/private-words"

# `git grep -n` prints `path:line:text`; `text` holds the matched word and is never shown.
_HIT = re.compile(r"^(?P<path>.*?):(?P<line>\d+):")
_BINARY_HIT = re.compile(r"^Binary file (?P<path>.*) matches$")


def _hits(output: str) -> list[str]:
    """Reduce `git grep -n` output to sorted, unique `path:line` (or `path`) locations."""
    found: set[str] = set()
    for line in output.splitlines():
        if binary := _BINARY_HIT.match(line):
            found.add(binary["path"])
        elif hit := _HIT.match(line):
            found.add(f"{hit['path']}:{hit['line']}")
    return sorted(found, key=lambda loc: (loc.partition(":")[0], int(loc.partition(":")[2] or 0)))


@group.command
def private_words(ctx: Context) -> None:
    """Fail when a tracked file matches a private word.

    Runs `git grep -n -i -E -f <file>` over the tracked tree, where `<file>` is
    `$CROOK_PRIVATE_WORDS`, or `~/.config/crook-dev/private-words` when that is
    unset. The file holds one regular expression per line and lives outside the
    repository, so the words never reach it. When the file is missing the hook
    passes with a notice, so CI and other contributors are not blocked.

    A match prints `path:line` only, never the word, its line or the pattern.
    """
    words = Path(os.environ.get(WORDS_ENV) or DEFAULT_WORDS).expanduser()
    if not words.is_file():
        ctx.info(f"private-words: no words file at {words}, skipping")
        return

    # A blank line is a regular expression that matches every line.
    patterns = [p for p in words.read_text(encoding="utf-8").splitlines() if p.strip()]
    if not patterns:
        ctx.info("private-words: ok (the words file is empty)")
        return

    with tempfile.TemporaryDirectory() as tmp:
        cleaned = Path(tmp) / "patterns"
        cleaned.write_text("\n".join(patterns) + "\n", encoding="utf-8")
        ret = ctx.run(
            "git",
            "-C",
            str(ctx.repo_root),
            "grep",
            "-n",
            "-i",
            "-E",
            "-f",
            str(cleaned),
            capture_output=True,
            stream_output=False,
        )
    stdout = ret.stdout.read()
    stdout = stdout.decode(errors="replace") if isinstance(stdout, bytes) else stdout

    # git grep exits 1 for "no match" and above that for errors; its stderr can echo a pattern.
    if ret.returncode > 1:
        ctx.error(f"private-words: git grep failed (exit {ret.returncode})", markup=False)
        ctx.exit(1)
    locations = _hits(stdout)
    if locations:
        for location in locations:
            ctx.error(f"private-words: {location}", markup=False)
        ctx.exit(1)
    ctx.info("private-words: ok")
