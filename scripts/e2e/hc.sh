#!/usr/bin/env bash
# hc.sh <run.env> <args...>: hc on the run's own core and folders, without the caller's identity. Every hc call
# the harness makes goes through here; the environment is checked before hc runs.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "$0")/lib.sh"
load_run "${1:-}"
shift
e2e_build_env
e2e_verify_env
exec env ${E2E_ENV[@]+"${E2E_ENV[@]}"} hc "$@"
