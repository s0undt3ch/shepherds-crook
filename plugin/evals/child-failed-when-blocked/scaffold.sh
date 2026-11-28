#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"
cat >briefs/totals.md <<'B'
## Goal

Write the total of the `amount` column of `data/sales.csv` to `out/total.txt`.

## Why

A smoke test of the child workflow.

## Scope

In scope:

- Reading `data/sales.csv`, writing `out/total.txt`.

Out of scope:

- Everything else.

## References

None.

## Done when

- `out/total.txt` holds the total, computed from `data/sales.csv`.
B
