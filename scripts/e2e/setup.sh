#!/usr/bin/env bash
# setup.sh [slug]: makes a throwaway run under $TMPDIR/crook-e2e-run.* and prints its env file.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
slug="${1:-e2ebot}"
# This checkout's build by default, so a run never replaces the user's installed crook.
crook="${CROOK:-$here/../../target/release/crook}"
[ -x "$crook" ] || { echo "no crook at $crook; run cargo build --release, or set CROOK" >&2; exit 1; }
crook="$(cd "$(dirname "$crook")" && pwd)/$(basename "$crook")"

# Resolved, because Claude Code reports /private/var on macOS and the guards compare prefixes.
tmp="$(realpath "${TMPDIR:-/tmp}")"
run="$(mktemp -d "$tmp/crook-e2e-run.XXXXXX")"
trap 'rm -rf "$run"' ERR
mkdir "$run/repo" "$run/state" "$run/data" "$run/notes"
git -C "$run/repo" init -q
git -C "$run/repo" -c user.name=e2e -c user.email=e2e@example.invalid -c commit.gpgsign=false \
  commit -q --allow-empty -m init
echo "e2e private note" > "$run/notes/plan.md"

cat > "$run/run.env" <<ENV
RUN=$run
ENVF=$run/run.env
E2E=$here
SLUG=$slug
CROOK=$crook
R=$run/repo
S=$run/state
D=$run/data
N=$run/notes
ENV

bash "$here/crook.sh" "$run/run.env" init "$slug" >/dev/null
bash "$here/crook.sh" "$run/run.env" project "$slug" add-repo "$run/repo" >/dev/null
bash "$here/crook.sh" "$run/run.env" project "$slug" add-notes "$run/notes" >/dev/null
trap - ERR
echo "$run/run.env"
