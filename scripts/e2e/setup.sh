#!/usr/bin/env bash
# Makes a throwaway run under $TMPDIR/crook-e2e-run.* and prints its env file: setup.sh [slug].
set -euo pipefail

slug="${1:-e2ebot}"
crook="${CROOK:-crook}"
command -v "$crook" >/dev/null || { echo "crook not found; set CROOK=/path/to/crook" >&2; exit 1; }

# Resolved, because Claude Code reports /private/var on macOS and trust.sh compares prefixes.
tmp="$(realpath "${TMPDIR:-/tmp}")"
run="$(mktemp -d "$tmp/crook-e2e-run.XXXXXX")"
mkdir "$run/repo" "$run/state" "$run/data" "$run/notes"
git -C "$run/repo" init -q
git -C "$run/repo" -c user.name=e2e -c user.email=e2e@example.invalid -c commit.gpgsign=false commit -q --allow-empty -m init
echo "e2e private note" > "$run/notes/plan.md"

cat > "$run/run.env" <<EOF
RUN=$run
SLUG=$slug
CROOK=$(command -v "$crook")
R=$run/repo
S=$run/state
D=$run/data
N=$run/notes
EOF

bash "$(dirname "$0")/crook.sh" "$run/run.env" init "$slug" >/dev/null
bash "$(dirname "$0")/crook.sh" "$run/run.env" project "$slug" add-repo "$run/repo" >/dev/null
bash "$(dirname "$0")/crook.sh" "$run/run.env" project "$slug" add-notes "$run/notes" >/dev/null
echo "$run/run.env"
