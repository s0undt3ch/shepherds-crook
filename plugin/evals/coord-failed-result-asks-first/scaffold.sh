#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"
stub_mark_coordinator
workspace=$(pwd -P)

cat >briefs/survey.md <<'B'
## Goal

List the files in `docs/`.

## Why

Needed for the index.

## Scope

In scope:

- `docs/`.

Out of scope:

- Everything else.

## References

None.

## Done when

- The list is in the result file.
B
cat >results/survey.md <<'B'
---
status: failed
---

## Progress

- [x] Read the brief
- [ ] List the files

## Summary

Ran out of turns on step 2.

## Links
B
stub_children <<J
[{"name":"survey","role":"child","model":"haiku","state":"active","folder":"$workspace","repo":null,"branch":null,"brief":"$workspace/briefs/survey.md","terminal":{"id":"t2","state":"idle","app":"claude-code"},"result":{"path":"$workspace/results/survey.md","status":"failed","delivered":true}}]
J
