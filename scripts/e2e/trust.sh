#!/usr/bin/env bash
# trust.sh <run.env> <terminal> [seconds]: answers Claude Code's trust and add-dir dialogs in one of the run's
# terminals, only when the dialog names a path under the run. The answer is `hc keys <terminal> down enter`
# (Q36): the default is "No, exit", so Down moves to "Yes". Stops after the screen shows no dialog for three
# polls in a row, or after the time limit (default 120 s).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
envf="${1:-}"
load_run "$envf"
t="${2:-}"
limit="${3:-120}"
[ -n "$t" ] || die "usage: trust.sh <run.env> <terminal> [seconds]"
hc() { "$BASH" "$here/hc.sh" "$envf" "$@"; }

cwd="$(hc get "$t" --json | jq -r '.cwd // ""')" || die "cannot read terminal $t"
resolved="$(resolve_dir "$cwd" || true)"
in_run_path "$cwd" || in_run_path "$resolved" || die "terminal $t runs in '$cwd', outside the run"

answered=0
quiet=0
start=$SECONDS
while :; do
  [ $((SECONDS - start)) -lt "$limit" ] || die "still prompting after $limit s"
  screen="$(hc read "$t")" || die "cannot read terminal $t"
  path=''
  if printf '%s' "$screen" | grep -q 'Accessing workspace:'; then
    path="$(printf '%s\n' "$screen" | awk '/Accessing workspace:/{f=1; next} f && NF {print $1; exit}')"
  elif printf '%s' "$screen" | grep -q "hasn't worked here before"; then
    path="$(printf '%s\n' "$screen" | awk "/hasn't worked here before/{print prev; exit} NF {prev=\$1}")"
  fi
  if [ -z "$path" ]; then
    quiet=$((quiet + 1))
    [ "$quiet" -lt 3 ] || break
    sleep 1
    continue
  fi
  quiet=0
  in_run_path "$path" || {
    echo "REFUSED: not inside the run: $path" >&2
    exit 1
  }
  hc keys "$t" down enter >/dev/null
  answered=$((answered + 1))
  echo "answered: $path"
  sleep 2
done
echo "dialogs answered: $answered"
