#!/usr/bin/env bash
# db.sh <run.env> "<sql>": reads the run's project database.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
case "$2" in .* | *$'\n'.*) die "db.sh takes SQL, not sqlite3 dot-commands" ;; esac
exec sqlite3 -readonly -header "$S/crook/$SLUG/crook.db" "$2"
