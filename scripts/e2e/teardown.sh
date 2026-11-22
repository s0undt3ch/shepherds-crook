#!/usr/bin/env bash
# teardown.sh <run.env>: stops everything the run started, wipes it and deletes its directory.
set -uo pipefail
here="$(dirname "$0")"
# shellcheck source=scripts/e2e/lib.sh
source "$here/lib.sh"
load_run "${1:?usage: teardown.sh <run.env>}"
envf="$1"
failed=0
c() { bash "$here/crook.sh" "$envf" "$@"; }
cwd_of() { lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p'; }

db="$S/crook/$SLUG/crook.db"
if [ -f "$db" ]; then
  for child in $(sqlite3 -readonly "$db" "select child from children where archived = 0"); do
    c stop "$SLUG/$child" || failed=1
  done
  pid="$(sqlite3 -readonly "$db" "select pid from orchestrator" 2>/dev/null)"
  if [ -n "$pid" ] && in_run_path "$(cwd_of "$pid")"; then kill "$pid" || failed=1; fi
fi

# Background sessions crook has no row for, such as a --bg child that started under another identity.
for id in $(claude agents --json 2>/dev/null | jq -r --arg run "$RUN/" \
  '.[] | select(.kind == "background") | select((.cwd // "") | startswith($run)) | .id'); do
  env -u CLAUDECODE claude stop "$id" </dev/null || failed=1
done
sleep 3

# Only workspaces whose every pane is inside the run, so none of the user's is touched.
for ws in $(herdr pane list 2>/dev/null | jq -r --arg run "$RUN" '
  .result.panes | group_by(.workspace_id)[]
  | select(all(.[]; (.cwd // "") as $c | $c == $run or ($c | startswith($run + "/"))))
  | .[0].workspace_id'); do
  if herdr workspace close "$ws" >/dev/null; then echo "closed herdr workspace $ws"; else failed=1; fi
done

if [ -f "$db" ]; then c wipe "$SLUG" </dev/null || failed=1; fi

left=0
# Any process still working inside the run, claude or not (a bg session runs as "claude bg-spare").
while read -r p cwd; do
  if in_run_path "$cwd"; then echo "LEFT: pid $p still runs in $cwd" >&2; left=1; fi
done < <(lsof -d cwd -Fpn 2>/dev/null | awk '/^p/{p=substr($0,2)} /^n/{print p, substr($0,2)}')
if [ "$left" = 0 ] && [ "$failed" = 0 ]; then
  rm -rf "$RUN" && echo "removed $RUN"
else
  echo "kept $RUN: a step failed or a process is left" >&2
  exit 1
fi
