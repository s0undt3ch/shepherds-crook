# shellcheck shell=bash
# Sourced by the scripts/e2e/ helpers. load_run <run.env> sources a run's env file and refuses one that
# could point anywhere but a run setup.sh made: every destructive step trusts RUN, S, D and R.

e2e_tmp() { realpath "${TMPDIR:-/tmp}"; }

die() { echo "e2e: $*" >&2; exit 1; }

# True when $1 is $RUN or under it, with no `..` component.
in_run_path() {
  case "/$1/" in */../*) return 1 ;; esac
  [ "$1" = "$RUN" ] || case "$1" in "$RUN"/*) return 0 ;; *) return 1 ;; esac
}

load_run() {
  [ -f "${1:-}" ] || die "no run env file: ${1:-}"
  # shellcheck disable=SC2034 # read indirectly below and by the callers
  RUN='' S='' D='' R='' N='' SLUG='' CROOK=''
  # shellcheck disable=SC1090
  source "$1"
  [ -n "$RUN" ] && [ -d "$RUN" ] || die "RUN is empty or missing: '$RUN'"
  [ "$(realpath "$RUN")" = "$RUN" ] || die "RUN is not a resolved path: $RUN"
  local base; base="$(e2e_tmp)"
  case "$RUN" in "$base"/crook-e2e-run.*) ;; *) die "RUN is not a run directory: $RUN" ;; esac
  case "${RUN#"$base"/}" in */*) die "RUN is not a direct child of $base: $RUN" ;; esac
  local v
  for v in S D R N; do
    in_run_path "${!v}" || die "$v is not inside RUN: '${!v}'"
  done
  [ -n "$SLUG" ] && [ -x "$CROOK" ] || die "SLUG or CROOK is missing"
}

# True when pane $1's cwd is inside the run, so typing into it cannot reach the user's own sessions.
pane_in_run() {
  local cwd
  cwd="$(herdr pane get "$1" 2>/dev/null | jq -r '.result.pane.cwd // ""')"
  in_run_path "$cwd"
}
