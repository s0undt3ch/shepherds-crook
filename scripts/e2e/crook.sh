#!/usr/bin/env bash
# crook.sh <run.env> <args...>: crook on the run's state, without the caller's identity. NO_HERDR=1 hides herdr.
set -euo pipefail
# shellcheck source=scripts/e2e/lib.sh
source "$(dirname "$0")/lib.sh"
load_run "$1"
shift
unset_herdr=()
if [ -n "${NO_HERDR:-}" ]; then unset_herdr=(-u HERDR_ENV); fi
exec env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD ${unset_herdr[@]+"${unset_herdr[@]}"} \
  XDG_STATE_HOME="$S" XDG_DATA_HOME="$D" XDG_CONFIG_HOME="$C" "$CROOK" "$@"
