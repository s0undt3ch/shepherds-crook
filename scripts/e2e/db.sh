#!/usr/bin/env bash
# db.sh <run.env> "<sql>": reads the run's project database.
set -euo pipefail
# shellcheck disable=SC1090
source "$1"
exec sqlite3 -readonly -header "$S/crook/$SLUG/crook.db" "$2"
