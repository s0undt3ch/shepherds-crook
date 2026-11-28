#!/usr/bin/env bash
# teardown.sh <run.env> [--keep]: stops the run's own core, ends leftover processes whose working folder is in
# the run, and removes both run folders. When anything is left or a step failed it keeps the run, says why and
# exits non-zero. --keep stops everything but removes nothing. Never touches any other core.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
envf="${1:?usage: teardown.sh <run.env> [--keep]}"
keep=0
[ "${2:-}" = "--keep" ] && keep=1
load_run "$envf"
command -v lsof >/dev/null || die "lsof is needed to find what still runs in the run"
# Out of the run, so this shell is not one of the processes left in it.
cd / || exit 1
failed=0

# Claude keeps its own sockets in cc-socks/ until its processes exit; the leftovers loop below ends those.
# The run's own core only: hc.sh sets the run's HC_SESSION and sockets, and refuses anything else.
if [ -n "$(find "$RT" -type s -not -path "*/cc-socks/*" 2>/dev/null)" ]; then
  "$BASH" "$here/hc.sh" "$envf" server stop >/dev/null 2>&1 || {
    echo "teardown: hc server stop failed" >&2
    failed=1
  }
  # The core removes its sockets as it exits; poll for them instead of guessing a delay.
  for _ in $(seq 1 20); do
    [ -z "$(find "$RT" -type s -not -path "*/cc-socks/*" 2>/dev/null)" ] && break
    sleep 0.5
  done
fi
if [ -n "$(find "$RT" -type s -not -path "*/cc-socks/*" 2>/dev/null)" ]; then
  echo "teardown: a socket is still in $RT after hc server stop" >&2
  failed=1
fi

# Any process still working inside the run (hc's terminals run in the flock and worktree folders).
leftovers() {
  lsof -d cwd -Fpn 2>/dev/null | awk '/^p/{p=substr($0,2)} /^n/{print p, substr($0,2)}' |
    while read -r p cwd; do
      if [ "$p" != "$$" ] && in_run_path "$cwd"; then echo "$p $cwd"; fi
    done
}
for attempt in 1 2 3; do
  left="$(leftovers)"
  [ -z "$left" ] && break
  while read -r pid cwd; do
    [ -n "$pid" ] || continue
    echo "teardown: ending pid $pid (in $cwd)" >&2
    (kill_in_run "$pid") || true
  done <<E2E_LEFT
$left
E2E_LEFT
  sleep "$attempt"
done
# A process that carries this run's HC_SESSION in its environment but works outside the run is not ended (the kill
# guard refuses it), and it is not ignored: the run is kept. `ps ewwax` prints the environment after the command on
# macOS and on procps (Linux); where a platform does not, nothing is found and this check says nothing.
env_strays() {
  ps ewwax -o pid=,command= 2>/dev/null | awk -v s="HC_SESSION=$SESSION" -v me="$$" \
    '$1 != me && index($0, " " s) && !/awk -v s=/ { print $1 }' |
    while read -r p; do
      # No working folder: the process is gone or a zombie waiting to be reaped.
      cwd="$(cwd_of "$p")"
      [ -n "$cwd" ] || continue
      in_run_path "$cwd" || echo "$p $cwd"
    done
}
strays="$(env_strays)"
if [ -n "$strays" ]; then
  echo "teardown: a process with this run's HC_SESSION runs outside the run (left alone):" >&2
  echo "$strays" >&2
  failed=1
fi
left="$(leftovers)"
if [ -n "$left" ]; then
  echo "teardown: still running in the run:" >&2
  echo "$left" >&2
  failed=1
fi

if [ "$failed" = 0 ] && [ "$keep" = 0 ]; then
  rm -rf "$RUN" "$RT" && echo "removed $RUN and $RT"
elif [ "$keep" = 1 ] && [ "$failed" = 0 ]; then
  echo "kept $RUN and $RT (--keep)"
else
  echo "kept $RUN and $RT: a step failed or a process is left" >&2
  exit 1
fi
