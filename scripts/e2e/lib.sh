# shellcheck shell=bash
# Sourced by the scripts/e2e/ helpers. load_run <run.env> sources a run's env file and refuses one that
# could point anywhere but a run setup.sh made: every step trusts RUN, S, D, C, R, N and SLUG.

# A run is a direct child of the resolved $TMPDIR named like this; setup.sh makes it, load_run checks it.
RUN_PREFIX=crook-e2e-run.

e2e_tmp() { realpath "${TMPDIR:-/tmp}"; }

# The run's project database.
run_db() { echo "$S/crook/$SLUG/crook.db"; }

die() { echo "e2e: $*" >&2; exit 1; }

# True when $1 is $RUN or under it, with no `..` component.
in_run_path() {
  case "/$1/" in */../*) return 1 ;; esac
  [ "$1" = "$RUN" ] || case "$1" in "$RUN"/*) return 0 ;; *) return 1 ;; esac
}

load_run() {
  [ -f "${1:-}" ] || die "no run env file: ${1:-}"
  # shellcheck disable=SC2034 # read indirectly below and by the callers
  RUN='' S='' D='' C='' R='' N='' SLUG='' CROOK=''
  # shellcheck disable=SC1090
  source "$1"
  [ -n "$RUN" ] && [ -d "$RUN" ] || die "RUN is empty or missing: '$RUN'"
  [ "$(realpath "$RUN")" = "$RUN" ] || die "RUN is not a resolved path: $RUN"
  local base; base="$(e2e_tmp)"
  case "$RUN" in "$base/$RUN_PREFIX"*) ;; *) die "RUN is not a run directory: $RUN" ;; esac
  case "${RUN#"$base"/}" in */*) die "RUN is not a direct child of $base: $RUN" ;; esac
  local v
  for v in S D C R N; do
    in_run_path "${!v}" || die "$v is not inside RUN: '${!v}'"
  done
  case "$SLUG" in '' | *[!a-z0-9-]*) die "SLUG is not a slug: '$SLUG'" ;; esac
  [ -x "$CROOK" ] || die "CROOK is not executable: '$CROOK'"
}

# True when pane $1's cwd is inside the run, so typing into it cannot reach the user's own sessions.
pane_in_run() {
  local cwd
  cwd="$(herdr pane get "$1" 2>/dev/null | jq -r '.result.pane.cwd // ""')"
  in_run_path "$cwd"
}

# The cwd of process $1, or nothing. Needs lsof, which every caller checks for first.
cwd_of() { lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p'; }
