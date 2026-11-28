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
  RUN_HOME='' CLAUDE_DIR='' STUBS='' SHIMS='' INSTALL='' RUN_TMP='' SHIM='' TIER=2 REAL_HOME='' REAL_CLAUDE=''
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
  case "$TIER" in
    2) ;;
    3)
      # Tier 3 keeps the caller's HOME and Claude config folder, for Claude's login, and nothing else of them.
      [ -n "$REAL_HOME" ] && [ -d "$REAL_HOME" ] || die "tier 3: REAL_HOME is empty or missing"
      in_run_path "$REAL_HOME" && die "tier 3: REAL_HOME is inside the run"
      [ -z "$REAL_CLAUDE" ] || [ -d "$REAL_CLAUDE" ] || die "tier 3: REAL_CLAUDE is not a folder"
      ;;
    *) die "TIER is '$TIER', not 2 or 3" ;;
  esac
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
  # `env` takes its -u options before the first NAME=value, so this one goes ahead of the run's own values.
  if [ "$TIER" = 3 ] && [ -z "$REAL_CLAUDE" ]; then E2E_ENV[${#E2E_ENV[@]}]="-u"; E2E_ENV[${#E2E_ENV[@]}]="CLAUDE_CONFIG_DIR"; fi
  E2E_ENV[${#E2E_ENV[@]}]="HC_SESSION=$SESSION"
  if [ "$TIER" = 3 ]; then
    E2E_ENV[${#E2E_ENV[@]}]="HOME=$REAL_HOME"
  else
    E2E_ENV[${#E2E_ENV[@]}]="HOME=$RUN_HOME"
  fi
  E2E_ENV[${#E2E_ENV[@]}]="TMPDIR=$RUN_TMP"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_STATE_HOME=$STATE"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_CONFIG_HOME=$CONFIG"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_DATA_HOME=$DATA"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_CACHE_HOME=$CACHE"
  E2E_ENV[${#E2E_ENV[@]}]="XDG_RUNTIME_DIR=$RT"
  E2E_ENV[${#E2E_ENV[@]}]="HERDR_APP_HOME=$APP"
  if [ "$TIER" != 3 ]; then
    E2E_ENV[${#E2E_ENV[@]}]="CLAUDE_CONFIG_DIR=$CLAUDE_DIR"
  elif [ -n "$REAL_CLAUDE" ]; then
    E2E_ENV[${#E2E_ENV[@]}]="CLAUDE_CONFIG_DIR=$REAL_CLAUDE"
  fi
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
      HOME)
        if [ "$TIER" = 3 ]; then [ "$value" = "$REAL_HOME" ] || die "HOME is not the tier-3 login home"
        else [ "$value" = "$RUN_HOME" ] || die "HOME is outside the run: $value"; fi
        seen="$seen $name"
        ;;
      CLAUDE_CONFIG_DIR)
        if [ "$TIER" = 3 ]; then [ "$value" = "$REAL_CLAUDE" ] || die "CLAUDE_CONFIG_DIR is not the tier-3 login folder"
        else in_run_path "$value" || die "$name is outside the run: $value"; fi
        seen="$seen $name"
        ;;
      XDG_STATE_HOME | XDG_CONFIG_HOME | XDG_DATA_HOME | XDG_CACHE_HOME | HERDR_APP_HOME)
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
    # Tier 3 without a custom Claude config folder has no CLAUDE_CONFIG_DIR at all.
    if [ "$name" = CLAUDE_CONFIG_DIR ] && [ "$TIER" = 3 ] && [ -z "$REAL_CLAUDE" ]; then continue; fi
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

# ---------------------------------------------------------------------------------------------------------------
# Helpers the tier 3 runner and its tests share.

# `--only` takes a comma list of the scenario names in $2 (space separated), exactly as spelled.
valid_only() {
  local item name found
  [ -n "$1" ] || return 1
  while IFS= read -r item; do
    found=0
    for name in $2; do [ "$item" = "$name" ] && found=1; done
    [ "$found" = 1 ] || return 1
  done <<VALID_ONLY
$(printf '%s\n' "$1" | tr ',' '\n')
VALID_ONLY
  return 0
}

# GNU stat reads -f as "filesystem status", so the flavour is probed once: `stat --version` works on GNU only.
mtime_list() {
  if stat --version >/dev/null 2>&1; then
    /usr/bin/find "$1" -exec stat -c '%Y %n' {} + 2>/dev/null | LC_ALL=C sort -k2
  else
    /usr/bin/find "$1" -exec stat -f '%m %N' {} + 2>/dev/null | LC_ALL=C sort -k2
  fi
}

# True when one process's command line holds "<flag> <id>" and the run's folder, so a flag with an empty or null id
# cannot match a process of the maintainer's own.
proc_with_id() { # proc_with_id <flag> <id> <other text>
  case "$2" in '' | null) return 1 ;; esac
  [ -n "$1" ] && [ -n "$3" ] && [ -n "${RUN:-}" ] || return 1
  ps -axww -o command= | awk -v a="$1 $2" -v b="$3" -v r="$RUN/" \
    'index($0, a) && index($0, b) && index($0, r) && !/awk -v a=/ { f = 1 } END { exit !f }'
}

# Transcript evidence. A missing or empty transcript is never evidence of absence.
tr_ok() { [ -n "${1:-}" ] && [ -s "$1" ]; }
# One line per tool call: the tool's name and its Bash command.
tool_uses() { jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use") | .name + " " + ((.input.command // "") | tostring)' "$1"; }
no_tool() { tr_ok "$1" || return 1; ! tool_uses "$1" | grep -q -- "$2"; }
tool_count() { tr_ok "$1" || return 1; tool_uses "$1" | grep -c -- "$2" || true; }
no_send_in() {
  tr_ok "$1" || return 1
  ! jq -r 'select(.type == "assistant") | .message.content[]? | select(.type == "tool_use") | (.input.command // "")' "$1" | grep -q 'hc send'
}
# What the session was TOLD, not what the model said: the text of user entries, the ones that start with <hc (hc's
# own notes and a child's tagged send), and the hook output added to a prompt.
user_lines() {
  jq -r 'select(.type == "user") | .message.content | if type == "string" then . else ([.[]? | select(.type == "text") | .text] | join("\n")) end' "$1"
}
hc_lines() { tr_ok "$1" || return 1; user_lines "$1" | grep '^<hc'; true; }
hook_lines() { tr_ok "$1" || return 1; jq -r 'select(.attachment != null) | .attachment | select(.type == "hook_additional_context") | .content[]?' "$1"; }
# count_in <text> <transcript> [hc|hook|user]: how many told-lines hold the text; fails on a missing transcript.
count_in() {
  local kind="${3:-hc}"
  tr_ok "$2" || return 1
  "${kind}_lines" "$2" | grep -c -F -- "$1" || true
}
# told_has <hc|hook|user> <transcript> <text>
told_has() { tr_ok "$2" || return 1; "${1}_lines" "$2" | grep -q -F -- "$3"; }
told_lacks() { tr_ok "$2" || return 1; ! "${1}_lines" "$2" | grep -q -F -- "$3"; }
