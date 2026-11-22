#!/usr/bin/env bash
# run_in.sh <run.env> <pane> <crook args...>: runs crook on the run's state in one of its herdr panes.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
pane="$2"
shift 2
pane_in_run "$pane" || die "pane $pane is not inside the run; not running in it"
line="$(printf '%q ' bash "$E2E/crook.sh" "$ENVF" "$@")"
if [ -n "${NO_HERDR:-}" ]; then line="NO_HERDR=1 $line"; fi
exec herdr pane run "$pane" "$line"
