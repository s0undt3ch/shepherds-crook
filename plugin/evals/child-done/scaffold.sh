#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"
cat >briefs/greeting.md <<'B'
## Goal

Create the file `out/greeting.txt` holding the single line `hello from the flock`.

## Why

A smoke test of the child workflow.

## Scope

In scope:

- `out/greeting.txt`.

Out of scope:

- Everything else.

## References

None.

## Done when

- `out/greeting.txt` exists and holds exactly that line.
B
