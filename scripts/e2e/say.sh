#!/usr/bin/env bash
# say.sh <run.env> <pane> "<text>": types a prompt into one of the run's herdr panes and submits it.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
pane="$2"
pane_in_run "$pane" || die "pane $pane is not inside the run; not typing into it"
herdr pane send-text "$pane" "$3" >/dev/null
sleep 0.3
herdr pane send-keys "$pane" enter >/dev/null
