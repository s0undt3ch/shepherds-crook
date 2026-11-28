#!/usr/bin/env bash
# core_env.sh <run.env>: S1's environment check (ADR 0020, spike 23). The run's core must not be running; this
# starts it through `crook status` (crook's own preflight, with the scrubbed environment), runs a probe terminal
# that lists the names of its environment, and fails when one of ADR 0020's variables is there. It checks the
# result, not the command. Prints names only, never values.
#
# hc gives every terminal its own HC_TERMINAL_ID, HC_TERMINAL_SOCKET and HC_SCOPE, so those are not wrong by
# being set: the id must be the probe's address and the socket must be in the run's socket folder. A
# CLAUDE_CODE_* name outside the user settings seen in spike 23 is a warning: they may be a new
# session marker (the plan's risk list), or a setting the maintainer owns.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
envf="${1:-}"
load_run "$envf"
hc() { "$BASH" "$here/hc.sh" "$envf" "$@"; }

[ -z "$(find "$RT" -type s 2>/dev/null)" ] || die "a core already runs for this run; core_env.sh needs a cold start"
"$BASH" "$here/crook.sh" "$envf" status >/dev/null || die "crook status failed"
[ -n "$(find "$RT" -type s 2>/dev/null)" ] || die "crook status did not start a core"

# The probe writes what it sees to files in the run, and the check reads the files, not the screen: a long
# line wraps and a screen scrolls. Names only; the socket and id are the two values that are checked.
pdir="$RUN_TMP/probe"
rm -rf "$pdir"
mkdir -p "$pdir"
# shellcheck disable=SC2016 # the probe's own variables, expanded in its shell
probe='env | sed -n "s/^\([A-Za-z_][A-Za-z0-9_]*\)=.*/\1/p" >"$1/names.tmp"; printf "%s\n" "$HC_TERMINAL_SOCKET" >"$1/socket"; printf "%s\n" "$HC_TERMINAL_ID" >"$1/id"; mv "$1/names.tmp" "$1/names"; sleep 600'
out="$(hc run --bg --json --name e2e-probe --no-parent --cwd "$RUN_TMP" -- sh -c "$probe" sh "$pdir")" || die "cannot start the probe"
address="$(printf '%s' "$out" | jq -r '.address')"
for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  [ -f "$pdir/names" ] && break
  sleep 0.5
done
[ -f "$pdir/names" ] || die "the probe wrote nothing"

failed=0
warned=0
while IFS= read -r name; do
  case "$name" in
    HC_TERMINAL_ID | HC_TERMINAL_SOCKET | HC_SCOPE) continue ;;
    CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY | CLAUDE_CODE_ENABLE_FUNCTION_HOOKS | CLAUDE_CODE_ENABLE_TELEMETRY) continue ;;
    CLAUDE_CODE_SUBAGENT_MODEL) continue ;;
    CLAUDE_CODE_*)
      echo "warning: $name is in the probe's environment; is it a session marker the list should cover?"
      warned=$((warned + 1))
      continue
      ;;
  esac
  if removed_name "$name" && ! set_name "$name"; then
    echo "FAIL: $name is in the probe's environment"
    failed=1
  fi
done <"$pdir/names"
socket="$(cat "$pdir/socket")"
id="$(cat "$pdir/id")"
case "$socket" in "$RT"/*) ;; *)
  echo "FAIL: the probe's HC_TERMINAL_SOCKET is not in the run's socket folder"
  failed=1
  ;;
esac
[ "$id" = "$address" ] || {
  echo "FAIL: the probe's HC_TERMINAL_ID is '$id', not its own address '$address'"
  failed=1
}
hc kill e2e-probe >/dev/null 2>&1 || true
if [ "$failed" = 0 ]; then echo "core_env: ok ($warned warning(s))"; fi
exit "$failed"
