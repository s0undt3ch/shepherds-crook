#!/usr/bin/env bash
# Fails when a change touches a file outside the allowed list.
#
#   check-files.sh commit   <allowed file>...   # the last commit, HEAD^..HEAD
#   check-files.sh worktree <allowed file>...   # tracked changes against HEAD, staged or not
#
# The release owns a few files; anything else in its commit or patch is a mistake or an attack.
set -euo pipefail

mode="${1:-}"
[ "$#" -ge 2 ] || {
  echo "usage: $0 commit|worktree <allowed file>..." >&2
  exit 2
}
shift
case "$mode" in
  commit) changed="$(git diff --no-renames --name-only HEAD^ HEAD)" ;;
  worktree) changed="$(git diff --no-renames --name-only HEAD)" ;;
  *)
    echo "usage: $0 commit|worktree <allowed file>..." >&2
    exit 2
    ;;
esac

# An empty entry would be an empty pattern, which matches every line: leave those out.
allowed=()
for file in "$@"; do
  [ -z "$file" ] || allowed+=("$file")
done
[ "${#allowed[@]}" -gt 0 ] || {
  echo "::error::No allowed files were given" >&2
  exit 2
}

unexpected="$(printf '%s\n' "$changed" | grep -vxF -f <(printf '%s\n' "${allowed[@]}") | grep -v '^$' || true)"
if [ -n "$unexpected" ]; then
  echo "::error::The release changed files it does not own:" >&2
  echo "$unexpected" >&2
  exit 1
fi
