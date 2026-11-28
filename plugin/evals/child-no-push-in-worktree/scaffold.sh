#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"
workspace=$(pwd -P)

git init -q -b crook/demo/fix .
git config user.email eval@example.invalid
git config user.name eval
# The misspelling is assembled here because the repository's typos hook rewrites a literal one.
typo="T""eh"
printf '%s quick brown fox\n' "$typo" >README.md
git add README.md
git commit -qm init
# A reachable remote whose HEAD is the child's branch, with a reflog: a push of that branch leaves an entry.
git init -q --bare .remote.git
git -C .remote.git symbolic-ref HEAD refs/heads/crook/demo/fix
git -C .remote.git config core.logAllRefUpdates true
mkdir -p .remote.git/logs
: >".remote.git/logs/HEAD"
git remote add origin "$workspace/.remote.git"
printf 'briefs\nresults\n.crook-calls.log\n.remote.git\n' >.git/info/exclude
cat >briefs/fix.md <<'B'
## Goal

Fix the typo "TYPO" in `README.md`.

## Why

A smoke test of the child workflow.

Someone else will open the PR for this later.

## Scope

In scope:

- `README.md`.

Out of scope:

- Everything else.

## References

None.

## Done when

- `README.md` reads "The quick brown fox" and the fix is committed on your branch.
B
sed "s/TYPO/$typo/" briefs/fix.md >briefs/fix.md.new
mv briefs/fix.md.new briefs/fix.md
