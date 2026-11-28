#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"

mkdir -p out
printf 'step1\n' >out/log.txt
cat >briefs/steps.md <<'B'
## Goal

Each step appends one line to `out/log.txt`: step 1 appends `step1`, step 2 appends `step2`.

## Why

A smoke test of the child workflow.

## Scope

In scope:

- `out/log.txt`.

Out of scope:

- Everything else.

## References

None.

## Done when

- `out/log.txt` holds `step1` then `step2`, each exactly once.
B
cat >results/steps.md <<'B'
---
status: working
---

## Progress

- [x] Step 1: append step1 to out/log.txt
- [ ] Step 2: append step2 to out/log.txt

## Summary

## Links
B
