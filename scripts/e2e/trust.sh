#!/usr/bin/env bash
# trust.sh <pane>: accepts pending trust and add-dir prompts, only for $TMPDIR/crook-e2e-* paths.
set -euo pipefail
pane="$1"
tmp="$(realpath "${TMPDIR:-/tmp}")"
n=0
waited=0
while :; do
  screen="$(herdr pane read "$pane" --source visible)"
  # Done only once the session is up (its crook status line shows), not while a dialog is still drawing.
  if ! printf '%s' "$screen" | grep -q -e 'Accessing workspace:' -e "hasn't worked here before" -e '⚠ crook:'; then
    [ "$waited" -lt 30 ] || { echo "REFUSED: no prompt and no session after 30 s" >&2; exit 1; }
    sleep 1; waited=$((waited + 1)); continue
  fi
  if printf '%s' "$screen" | grep -q 'Accessing workspace:'; then
    path="$(printf '%s\n' "$screen" | awk '/Accessing workspace:/{f=1; next} f && NF {print $1; exit}')"
    yes='Yes, I trust this folder'
  elif printf '%s' "$screen" | grep -q "hasn't worked here before"; then
    path="$(printf '%s\n' "$screen" | awk "/hasn't worked here before/{print prev; exit} NF {prev=\$1}")"
    yes='Yes, add it'
  else
    break
  fi
  case "$path" in
    "$tmp"/crook-e2e-*) ;;
    *) echo "REFUSED: not a crook-e2e path: $path" >&2; exit 1 ;;
  esac
  # The default is "No, exit", and the first Down is sometimes dropped, so check before Enter.
  ok=
  for _ in 1 2 3; do
    if herdr pane read "$pane" --source visible | grep -q "❯ $yes"; then ok=1; break; fi
    herdr pane send-keys "$pane" down >/dev/null
    sleep 1
  done
  [ -n "$ok" ] || { echo "REFUSED: could not highlight '$yes'" >&2; exit 1; }
  herdr pane send-keys "$pane" enter >/dev/null
  echo "accepted ($yes): $path"
  n=$((n + 1))
  sleep 3
done
echo "prompts accepted: $n"
