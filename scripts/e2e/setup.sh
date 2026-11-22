#!/usr/bin/env bash
# setup.sh [slug]: makes a throwaway run under $TMPDIR/crook-e2e-run.* and prints its env file.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/e2e/lib.sh
source "$here/lib.sh"
slug="${1:-}"
# This checkout's build by default, so a run never replaces the user's installed crook.
crook="${CROOK:-$here/../../target/release/crook}"
[ -x "$crook" ] || { echo "no crook at $crook; run cargo build --release, or set CROOK" >&2; exit 1; }
crook="$(cd "$(dirname "$crook")" && pwd)/$(basename "$crook")"

# Under the resolved $TMPDIR, because Claude Code reports /private/var on macOS and the guards compare prefixes.
run="$(mktemp -d "$(e2e_tmp)/${RUN_PREFIX}XXXXXX")"
trap 'rm -rf "$run"' ERR
# Unique per run: herdr workspace labels are global, and a dead run's crook-<slug> would be reused.
[ -n "$slug" ] || slug="e2e$(printf '%s' "${run##*.}" | tr '[:upper:]' '[:lower:]')"
case "$slug" in *[!a-z0-9-]*) echo "slug must be lowercase letters, digits and -: $slug" >&2; false ;; esac
if command -v herdr >/dev/null && herdr workspace list 2>/dev/null |
  jq -e --arg l "crook-$slug" '.result.workspaces[] | select(.label == $l)' >/dev/null; then
  echo "a herdr workspace crook-$slug already exists; pick another slug" >&2
  false
fi
mkdir "$run/repo" "$run/state" "$run/data" "$run/config" "$run/notes"
git -C "$run/repo" init -q
git -C "$run/repo" -c user.name=e2e -c user.email=e2e@example.invalid -c commit.gpgsign=false \
  commit -q --allow-empty -m init
echo "e2e private note" > "$run/notes/plan.md"

for kv in "RUN=$run" "ENVF=$run/run.env" "E2E=$here" "SLUG=$slug" "CROOK=$crook" \
  "R=$run/repo" "S=$run/state" "D=$run/data" "C=$run/config" "N=$run/notes"; do
  printf '%s=%q\n' "${kv%%=*}" "${kv#*=}"
done > "$run/run.env"

bash "$here/crook.sh" "$run/run.env" init "$slug" >/dev/null
bash "$here/crook.sh" "$run/run.env" project "$slug" add-repo "$run/repo" >/dev/null
bash "$here/crook.sh" "$run/run.env" project "$slug" add-notes "$run/notes" >/dev/null
trap - ERR
echo "$run/run.env"
