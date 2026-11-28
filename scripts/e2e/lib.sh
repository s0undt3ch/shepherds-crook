# shellcheck shell=bash
# Sourced by the scripts/e2e/ helpers. Written for bash 3.2 (macOS /bin/bash): no associative arrays, no
# `mapfile`, no `${x,,}`, and an empty array is expanded as ${arr[@]+"${arr[@]}"}.
#
# load_run <run.env> sources a run's env file and refuses one that could point anywhere but a run setup.sh
# made. e2e_build_env turns the run into the `env` arguments every hc and crook call goes through.

# A run is a direct child of the resolved $TMPDIR, a socket folder a direct child of the resolved /tmp.
RUN_PREFIX=crook-e2e-run.
RT_PREFIX=crook-e2e-rt.
SESSION_PREFIX=crook-e2e-

die() {
  echo "e2e: $*" >&2
  exit 1
}

# A resolved directory path; macOS reports /private/var and /private/tmp.
resolve_dir() { (cd "$1" 2>/dev/null && pwd -P); }
e2e_tmp() { resolve_dir "${TMPDIR:-/tmp}"; }
e2e_rt_base() { resolve_dir /tmp; }

# True when $1 is $RUN or under it, with no `..` component.
in_run_path() {
  case "/$1/" in */../*) return 1 ;; esac
  [ -n "$1" ] || return 1
  [ "$1" = "$RUN" ] && return 0
  case "$1" in "$RUN"/*) return 0 ;; *) return 1 ;; esac
}

# The names every call to hc or crook starts without (ADR 0020): the Claude session variables and the calling
# terminal's identity. Beyond the list: every other HC_*, HERDR_* and CROOK_* name, because each of them can
# point a call at a core or a folder the run does not own. Names the run sets itself are not listed here.
adr0020_name() {
  case "$1" in
    CLAUDECODE | CLAUDE_PID | CLAUDE_EFFORT | CLAUDE_CODE_ENTRYPOINT | CLAUDE_CODE_EXECPATH | AI_AGENT) return 0 ;;
    HC_TERMINAL_ID | HC_TERMINAL_SOCKET | HC_SCOPE) return 0 ;;
    CLAUDE_CODE_SESSION_* | CLAUDE_CODE_CHILD_* | CLAUDE_CODE_MESSAGING_*) return 0 ;;
  esac
  return 1
}

# Every GIT_* name goes too (GIT_DIR, GIT_WORK_TREE and the like point git at the caller's repository); the
# run's own GIT_* values are set after the removal.
removed_name() {
  adr0020_name "$1" && return 0
  case "$1" in
    HC_SESSION | HERDR_APP_HOME | CROOK_BIN) return 1 ;;
    HC_* | HERDR_* | CROOK_* | GIT_*) return 0 ;;
  esac
  return 1
}

# Unsets, in this shell, every name `git rev-parse --local-env-vars` lists and every GIT_CONFIG_* name, so a
# GIT_DIR exported by the caller cannot make a git call act on the caller's repository.
unset_git_env() {
  local n
  for n in $(git rev-parse --local-env-vars 2>/dev/null) $(compgen -e | grep '^GIT_CONFIG' || true); do
    unset "$n"
  done
}

# The names the run sets, so none of them is also given a `-u`.
set_name() {
  case "$1" in
    HC_SESSION | HOME | TMPDIR | PATH | HERDR_APP_HOME | CLAUDE_CONFIG_DIR | CROOK_BIN) return 0 ;;
    XDG_STATE_HOME | XDG_CONFIG_HOME | XDG_DATA_HOME | XDG_CACHE_HOME | XDG_RUNTIME_DIR) return 0 ;;
    GIT_CONFIG_GLOBAL | GIT_CONFIG_NOSYSTEM | GIT_AUTHOR_NAME | GIT_AUTHOR_EMAIL) return 0 ;;
    GIT_COMMITTER_NAME | GIT_COMMITTER_EMAIL | E2E_RUN) return 0 ;;
  esac
  return 1
}

load_run() {
  [ -f "${1:-}" ] || die "no run env file: ${1:-}"
  # shellcheck disable=SC2034 # read indirectly below and by the callers
  RUN='' RT='' SESSION='' SLUG='' REAL_PATH='' REPO='' REMOTE='' DATA='' CONFIG='' STATE='' CACHE='' APP=''
  # shellcheck disable=SC2034
  RUN_HOME='' CLAUDE_DIR='' STUBS='' SHIMS='' INSTALL='' RUN_TMP='' SHIM=''
  # shellcheck disable=SC1090
  source "$1"
  [ -n "$RUN" ] && [ -d "$RUN" ] || die "RUN is empty or missing: '$RUN'"
  [ "$(resolve_dir "$RUN")" = "$RUN" ] || die "RUN is not a resolved path: $RUN"
  local base
  base="$(e2e_tmp)"
  case "$RUN" in "$base/$RUN_PREFIX"*) ;; *) die "RUN is not a run directory: $RUN" ;; esac
  case "${RUN#"$base"/}" in */*) die "RUN is not a direct child of $base: $RUN" ;; esac
  local id="${RUN##*.}"
  case "$id" in '' | *[!A-Za-z0-9]*) die "RUN has no run id: $RUN" ;; esac
  local want
  want="$SESSION_PREFIX$(printf '%s' "$id" | tr '[:upper:]' '[:lower:]')"
  [ "$SESSION" = "$want" ] || die "SESSION is '$SESSION', not this run's '$want'"
  local rtbase
  rtbase="$(e2e_rt_base)"
  case "$RT" in "$rtbase/$RT_PREFIX"*) ;; *) die "RT is not a run socket folder: '$RT'" ;; esac
  case "${RT#"$rtbase"/}" in */*) die "RT is not a direct child of $rtbase: $RT" ;; esac
  [ -d "$RT" ] || die "RT is missing: $RT"
  [ "$(resolve_dir "$RT")" = "$RT" ] || die "RT is not a resolved path: $RT"
  local v
  for v in REPO REMOTE DATA CONFIG STATE CACHE APP RUN_HOME CLAUDE_DIR STUBS SHIMS INSTALL RUN_TMP SHIM; do
    in_run_path "${!v}" || die "$v is not inside RUN: '${!v}'"
  done
  case "$SLUG" in '' | *[!a-z0-9-]*) die "SLUG is not a slug: '$SLUG'" ;; esac
  [ -n "$REAL_PATH" ] || die "REAL_PATH is empty"
}

# Fills E2E_ENV with the arguments of one `env` call: a `-u` for every exported name that must not reach hc
# or crook, then the run's own values. The names come from the live environment (`compgen -e`), so a variable
# this shell has and a fixed list does not is still removed. The list is a bash ARRAY: zsh does not split an
# unquoted string into words, so `-u NAME` flags kept in one string reach `env` as one argument and remove
# nothing (spike 23, Q37).
e2e_build_env() {
  E2E_ENV=()
  local name
  while IFS= read -r name; do
    if removed_name "$name" && ! set_name "$name"; then E2E_ENV[${#E2E_ENV[@]}]="-u"; E2E_ENV[${#E2E_ENV[@]}]="$name"; fi
  done < <(compgen -e)
  E2E_ENV[${#E2E_ENV[@]}]="HC_SESSION=$SESSION"
  E2E_ENV[${#E2E_ENV[@]}]="HOME=$RUN_HOME"
  E2E_ENV[${#E2E_ENV[@]}]="TMPDIR=$RUN_TMP"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_STATE_HOME=$STATE"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_CONFIG_HOME=$CONFIG"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_DATA_HOME=$DATA"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_CACHE_HOME=$CACHE"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_RUNTIME_DIR=$RT"
  E2E_ENV[${#E2E_ENV[@]}]="HERDR_APP_HOME=$APP"
  E2E_ENV[${#E2E_ENV[@]}]="CLAUDE_CONFIG_DIR=$CLAUDE_DIR"
  E2E_ENV[${#E2E_ENV[@]}]="CROOK_BIN=$SHIM"
  # The run's stub programs come first, so a stray `herdr-app` or `claude` is never the real one.
  E2E_ENV[${#E2E_ENV[@]}]="PATH=$STUBS:$REAL_PATH"
  E2E_ENV[${#E2E_ENV[@]}]="E2E_RUN=$RUN"
  # Git needs no global or system config, and no signing; the repository's own commits carry this identity.
  E2E_ENV[${#E2E_ENV[@]}]="GIT_CONFIG_GLOBAL=/dev/null"
  E2E_ENV[${#E2E_ENV[@]}]="GIT_CONFIG_NOSYSTEM=1"
  E2E_ENV[${#E2E_ENV[@]}]="GIT_AUTHOR_NAME=e2e"
  E2E_ENV[${#E2E_ENV[@]}]="GIT_AUTHOR_EMAIL=e2e@example.invalid"
  E2E_ENV[${#E2E_ENV[@]}]="GIT_COMMITTER_NAME=e2e"
  E2E_ENV[${#E2E_ENV[@]}]="GIT_COMMITTER_EMAIL=e2e@example.invalid"
}

# Checks the result, not the command: runs /usr/bin/env under E2E_ENV and refuses when the environment a call
# would get still holds a removed name, or lacks or misplaces one of the run's values.
e2e_verify_env() {
  local dump line name value
  dump="$(env ${E2E_ENV[@]+"${E2E_ENV[@]}"} /usr/bin/env)" || die "cannot build the scrubbed environment"
  local seen=''
  while IFS= read -r line; do
    case "$line" in *=*) ;; *) continue ;; esac
    name="${line%%=*}"
    value="${line#*=}"
    if removed_name "$name" && ! set_name "$name"; then die "$name is still set after the scrub"; fi
    case "$name" in
      HC_SESSION) [ "$value" = "$SESSION" ] || die "HC_SESSION is '$value', not the run's '$SESSION'"; seen="$seen $name" ;;
      HOME) [ "$value" = "$RUN_HOME" ] || die "HOME is outside the run: $value"; seen="$seen $name" ;;
      XDG_STATE_HOME | XDG_CONFIG_HOME | XDG_DATA_HOME | XDG_CACHE_HOME | HERDR_APP_HOME | CLAUDE_CONFIG_DIR)
        in_run_path "$value" || die "$name is outside the run: $value"
        seen="$seen $name"
        ;;
      XDG_RUNTIME_DIR) [ "$value" = "$RT" ] || die "XDG_RUNTIME_DIR is outside the run: $value"; seen="$seen $name" ;;
    esac
  done <<E2E_DUMP
$dump
E2E_DUMP
  for name in HC_SESSION HOME XDG_STATE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_RUNTIME_DIR \
    HERDR_APP_HOME CLAUDE_CONFIG_DIR; do
    case "$seen" in *" $name"*) ;; *) die "$name is missing from the scrubbed environment" ;; esac
  done
}

# The cwd of process $1, or nothing. Needs lsof, which every caller checks for first.
cwd_of() { lsof -a -p "$1" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p'; }

# SIGTERMs process $1 only when its cwd is inside the run.
kill_in_run() {
  case "$1" in '' | *[!0-9]*) die "not a pid: '$1'" ;; esac
  local cwd
  cwd="$(cwd_of "$1")"
  in_run_path "$cwd" || die "pid $1 runs in '$cwd', outside the run; not killing it"
  kill "$1"
}
