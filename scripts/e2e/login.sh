#!/usr/bin/env bash
# login.sh <run.env> [--min-minutes N]: gives a tier 3 run Claude's login without the caller's HOME or Claude config
# folder (D122). The run's sessions then use the run's HOME and CLAUDE_CONFIG_DIR, like tier 2.
#
# Claude Code keeps its login under a name made from the config folder: on macOS the keychain item
# "Claude Code-credentials", with "-<first 8 hex of sha256(folder)>" added for a custom folder; elsewhere
# <folder>/.credentials.json. This copies the caller's login to the run folder's name WITHOUT its refresh token, so
# the copy can never refresh, and so never rotates the caller's own login. A run therefore needs an access token that
# outlives it: the copy is refused when fewer than --min-minutes (default 60) remain. teardown.sh deletes the copy.
#
# On macOS the keychain is found through HOME, so the run's HOME gets one link, Library/Keychains, to the caller's.
# The caller's .claude.json is copied without `projects` and `mcpServers`, so the run starts past onboarding with no
# trust entries and none of the caller's MCP servers. Nothing of the caller's is changed. No secret is printed; the
# keychain copy passes the credentials to `security` as an argument, which other processes of the same user can see
# while it runs.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
envf="${1:?usage: login.sh <run.env> [--min-minutes N]}"
min=60
if [ "${2:-}" = "--min-minutes" ]; then min="${3:-}"; fi
case "$min" in '' | *[!0-9]*) die "--min-minutes takes a whole number" ;; esac
load_run "$envf"
[ "$TIER" = 3 ] || die "login.sh is for tier 3 runs only"
command -v jq >/dev/null || die "jq is needed"

real_cfg="${REAL_CLAUDE:-$REAL_HOME/.claude}"
# Claude reads .claude.json from inside a custom config folder, else from HOME.
if [ -n "$REAL_CLAUDE" ]; then real_json="$REAL_CLAUDE/.claude.json"; else real_json="$REAL_HOME/.claude.json"; fi
[ -f "$real_json" ] || die "no Claude config at $real_json; log in to Claude Code first"

# E2E_LOGIN_STORE=file forces the credentials file, for tests on a Mac; otherwise the keychain wherever `security` is.
use_keychain=0
if [ "${E2E_LOGIN_STORE:-}" != file ] && command -v security >/dev/null 2>&1; then use_keychain=1; fi

if [ "$use_keychain" = 1 ]; then
  src="$(claude_cred_service "$REAL_CLAUDE")"
  creds="$(security find-generic-password -s "$src" -w 2>/dev/null)" || die "no Claude login in the keychain under '$src'"
else
  creds="$(cat "$real_cfg/.credentials.json" 2>/dev/null)" || die "no Claude login at $real_cfg/.credentials.json"
fi
left="$(printf '%s' "$creds" | jq -r '((.claudeAiOauth.expiresAt // 0) / 1000 - now) / 60 | floor' 2>/dev/null)" ||
  die "the Claude login is not in the expected shape"
[ -n "$(printf '%s' "$creds" | jq -r '.claudeAiOauth.accessToken // empty' 2>/dev/null)" ] || die "the Claude login has no access token"
[ "$left" -ge "$min" ] ||
  die "Claude's access token expires in $left minutes and a run needs $min; use Claude Code once so it refreshes, then rerun"
copy="$(printf '%s' "$creds" | jq -c 'del(.claudeAiOauth.refreshToken)')"
unset creds

mkdir -p "$CLAUDE_DIR"
# hasSeenAutoDefaultNudge: a fresh config under `default` mode otherwise stops after the first turn to offer auto mode.
jq 'del(.projects, .mcpServers) | .hasSeenAutoDefaultNudge = true' "$real_json" >"$CLAUDE_DIR/.claude.json"
# From here on teardown.sh deletes what this made, even if a later step fails.
: >"$CLAUDE_DIR/$LOGIN_MARKER"
if [ "$use_keychain" = 1 ]; then
  mkdir -p "$RUN_HOME/Library"
  [ -e "$RUN_HOME/Library/Keychains" ] || ln -s "$REAL_HOME/Library/Keychains" "$RUN_HOME/Library/Keychains"
  acct="$(security find-generic-password -s "$src" 2>/dev/null | sed -n 's/.*"acct"<blob>="\(.*\)"$/\1/p')"
  security add-generic-password -U -s "$(claude_cred_service "$CLAUDE_DIR")" -a "${acct:-$USER}" -w "$copy" ||
    die "could not add the run's keychain item"
else
  (umask 077 && printf '%s' "$copy" >"$CLAUDE_DIR/.credentials.json")
fi
unset copy
echo "login: copied, without its refresh token; the access token has $left minutes left"
