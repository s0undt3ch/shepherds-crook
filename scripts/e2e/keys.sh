#!/usr/bin/env bash
# keys.sh <run.env> <pane> <key>...: sends key presses to one of the run's herdr panes.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
pane="$2"
shift 2
pane_in_run "$pane" || die "pane $pane is not inside the run; not sending keys"
exec herdr pane send-keys "$pane" "$@"
