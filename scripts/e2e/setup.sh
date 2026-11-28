#!/usr/bin/env bash
# setup.sh [--tier 2|3] [slug]: makes a throwaway run under $TMPDIR/crook-e2e-run.* and a short socket folder
# under /tmp/crook-e2e-rt.*, and prints the path of the run's env file. Starts nothing.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"

tier=2
if [ "${1:-}" = "--tier" ]; then
  tier="${2:-}"
  shift 2
fi
case "$tier" in 2 | 3) ;; *) die "--tier must be 2 or 3" ;; esac
unset_git_env
slug="${1:-}"
# A slug is at most 16 characters of a-z, 0-9 and - (D27), and must not look like an address (D36).
check_slug() {
  case "$1" in *[!a-z0-9-]* | '') die "slug must be lowercase letters, digits and -: $1" ;; esac
  [ "${#1}" -le 16 ] || die "slug is longer than 16 characters: $1"
  case "$1" in t[0-9]*) die "slug looks like an hc address: $1" ;; esac
}
[ -z "$slug" ] || check_slug "$slug"

run=''
rt=''
finished=0
# Any exit before the run is complete, die included, removes what was made.
trap '[ "$finished" = 1 ] || { [ -z "$run" ] || rm -rf "$run"; [ -z "$rt" ] || rm -rf "$rt"; }' EXIT
run="$(mktemp -d "$(e2e_tmp)/${RUN_PREFIX}XXXXXX")"
rt="$(mktemp -d "$(e2e_rt_base)/${RT_PREFIX}XXXXXX")"
run="$(resolve_dir "$run")"
rt="$(resolve_dir "$rt")"
id="${run##*.}"
lower="$(printf '%s' "$id" | tr '[:upper:]' '[:lower:]')"
session="${SESSION_PREFIX}${lower}"
[ -n "$slug" ] || slug="e2e-$lower"
check_slug "$slug"

mkdir "$run/repo" "$run/data" "$run/config" "$run/state" "$run/cache" "$run/herdr-app" "$run/install" \
  "$run/shims" "$run/stubs" "$run/home" "$run/claude" "$run/tmp" "$run/notes"

# The repository the children get worktrees of, with the remote the push rule in 0023 looks at.
git_run() { git -c user.name=e2e -c user.email=e2e@example.invalid -c commit.gpgsign=false "$@"; }
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
git init -q --bare -b main "$run/remote.git"
git init -q -b main "$run/repo"
printf '.worktrees/\n' >"$run/repo/.gitignore"
echo "e2e" >"$run/repo/README.md"
git -C "$run/repo" add .gitignore README.md
git_run -C "$run/repo" commit -q -m "init"
git -C "$run/repo" remote add origin "$run/remote.git"
git_run -C "$run/repo" push -q origin main

# A cheap coordinator, so the maintainer's default model is never used by a test run (Q32).
mkdir "$run/config/crook"
printf 'coordinator_model = "haiku"\ndefault_child_model = "haiku"\nnotes_root = "%s"\n' "$run/notes" >"$run/config/crook/config.toml"

# herdr-app is always a stub: the real app may be watching the maintainer's own core. claude is a stub in tier 2.
cp "$here/stubs/herdr-app" "$run/stubs/herdr-app"
if [ "$tier" = 2 ]; then cp "$here/stubs/claude" "$run/stubs/claude"; fi
chmod +x "$run"/stubs/*

{
  for kv in "RUN=$run" "RT=$rt" "SESSION=$session" "SLUG=$slug" "TIER=$tier" "REAL_PATH=$PATH" \
    "REPO=$run/repo" "REMOTE=$run/remote.git" "DATA=$run/data" "CONFIG=$run/config" "STATE=$run/state" \
    "CACHE=$run/cache" "APP=$run/herdr-app" "RUN_HOME=$run/home" "CLAUDE_DIR=$run/claude" "STUBS=$run/stubs" \
    "SHIMS=$run/shims" "INSTALL=$run/install" "RUN_TMP=$run/tmp" "SHIM=$run/shims/crook"; do
    printf '%s=%q\n' "${kv%%=*}" "${kv#*=}"
  done
} >"$run/run.env"

load_run "$run/run.env"
finished=1
echo "$run/run.env"
