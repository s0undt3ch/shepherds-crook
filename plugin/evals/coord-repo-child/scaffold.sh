#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"

mkdir -p repo/src
git -C repo init -q -b main
git -C repo config user.email eval@example.invalid
git -C repo config user.name eval
# The misspelling is assembled here because the repository's typos hook rewrites a literal one.
printf '// %s the data\n' "rec""ieve" >repo/src/lib.rs
git -C repo add src/lib.rs
git -C repo commit -qm init
