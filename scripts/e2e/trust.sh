#!/usr/bin/env bash
# trust.sh <run.env> <pane>: accepts pending trust and add-dir prompts in a run pane, only for paths inside the run.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
pane="$2"
pane_in_run "$pane" || die "pane $pane is not inside the run"
n=0
start=$SECONDS
while :; do
  [ $((SECONDS - start)) -lt 120 ] || die "still prompting after 120 s"
  screen="$(herdr pane read "$pane" --source visible)"
  if printf '%s' "$screen" | grep -q 'Accessing workspace:'; then
    path="$(printf '%s\n' "$screen" | awk '/Accessing workspace:/{f=1; next} f && NF {print $1; exit}')"
    yes='Yes, I trust this folder'
  elif printf '%s' "$screen" | grep -q "hasn't worked here before"; then
    path="$(printf '%s\n' "$screen" | awk "/hasn't worked here before/{print prev; exit} NF {prev=\$1}")"
    yes='Yes, add it'
  elif printf '%s' "$screen" | grep -q '⚠ crook:'; then
    break
  else
    sleep 1
    continue
  fi
  in_run_path "$path" || { echo "REFUSED: not inside the run: $path" >&2; exit 1; }
  # The default is "No, exit", and a Down can be dropped while the dialog draws, so check before Enter.
  for _ in 1 2 3; do
    herdr pane read "$pane" --source visible | grep -q "❯ $yes" && break
    herdr pane send-keys "$pane" down >/dev/null
    sleep 1
  done
  herdr pane read "$pane" --source visible | grep -q "❯ $yes" || die "REFUSED: could not highlight '$yes'"
  herdr pane send-keys "$pane" enter >/dev/null
  echo "accepted ($yes): $path"
  n=$((n + 1))
  sleep 3
done
echo "prompts accepted: $n"
