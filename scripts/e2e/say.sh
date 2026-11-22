#!/usr/bin/env bash
# say.sh <pane> "<text>": types a prompt into a herdr pane and submits it.
set -euo pipefail
pane="$1"
# A pane scrolled up swallows typed text, so bring it to the bottom first.
herdr pane send-keys "$pane" end >/dev/null 2>&1 || true
herdr pane send-text "$pane" "$2" >/dev/null
sleep 0.3
herdr pane send-keys "$pane" enter >/dev/null
