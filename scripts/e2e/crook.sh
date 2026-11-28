#!/usr/bin/env bash
# crook.sh <run.env> <args...>: the run's active crook (its fake shim) with the same environment as hc.sh, so a
# core that crook starts belongs to the run and its terminals get a clean environment (spike 23).
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
load_run "${1:-}"
shift
[ -x "$SHIM" ] || die "no crook is installed in the run yet; run install.sh first"
e2e_build_env
e2e_verify_env
exec env ${E2E_ENV[@]+"${E2E_ENV[@]}"} "$SHIM" "$@"
