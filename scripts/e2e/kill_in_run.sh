#!/usr/bin/env bash
# kill_in_run.sh <run.env> <pid>: SIGTERMs a process, only when its cwd is inside the run.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
command -v lsof >/dev/null || die "lsof is needed to check a process's cwd"
case "$2" in '' | *[!0-9]*) die "not a pid: '$2'" ;; esac
cwd="$(cwd_of "$2")"
in_run_path "$cwd" || die "pid $2 runs in '$cwd', outside the run; not killing it"
exec kill "$2"
