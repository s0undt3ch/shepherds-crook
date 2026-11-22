#!/usr/bin/env bash
# teardown.sh <run.env>: stops everything the run started, wipes it and deletes its directory.
set -uo pipefail
env_file="${1:?usage: teardown.sh <run.env>}"
failed=0
# shellcheck disable=SC1090
source "$env_file"
here="$(dirname "$0")"
c() { bash "$here/crook.sh" "$env_file" "$@"; }
in_run() { lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | grep -q "^$RUN/"; }

db="$S/crook/$SLUG/crook.db"
if [ -f "$db" ]; then
  for child in $(sqlite3 -readonly "$db" "select child from children where archived = 0"); do
    c stop "$SLUG/$child" || failed=1
  done
  pid="$(sqlite3 -readonly "$db" "select pid from orchestrator" 2>/dev/null)"
  if [ -n "$pid" ] && in_run "$pid"; then kill "$pid"; fi
fi

# Background sessions crook has no row for, such as a --bg child that started under another identity.
claude agents --json 2>/dev/null | jq -r --arg run "$RUN/" '.[] | select((.cwd // "") | startswith($run)) | select(.kind == "background") | .id' |
  while read -r id; do env -u CLAUDECODE claude stop "$id"; done
sleep 3

# Only workspaces with a pane inside the run, so none of the user's is touched.
for ws in $(herdr pane list 2>/dev/null | jq -r --arg run "$RUN/" '[.result.panes[] | select((.cwd // "") | startswith($run)) | .workspace_id] | unique | .[]'); do
  herdr workspace close "$ws" >/dev/null && echo "closed herdr workspace $ws"
done

if [ -f "$db" ]; then c wipe "$SLUG" </dev/null || failed=1; fi

left=0
# -f, not -x: background sessions run as "claude bg-spare".
for p in $(pgrep -f claude); do
  if in_run "$p"; then echo "LEFT: pid $p still runs in $RUN" >&2; left=1; fi
done
if [ "$left" = 0 ] && [ "$failed" = 0 ]; then
  rm -rf "$RUN" && echo "removed $RUN"
else
  echo "kept $RUN: a stop or wipe failed, or a process is left" >&2
  exit 1
fi
