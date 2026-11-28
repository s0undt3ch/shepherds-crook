#!/usr/bin/env bash
# Checks the exact file set a release ships, before anything is attested or published:
#
#   scripts/check-release-assets.sh <dir> <version> <archive count>
#
# The folder holds one `crook-<version>-<triple>.tar.gz` and one matching `.sha256` per target, and
# nothing else; each `.sha256` matches its archive.
set -euo pipefail

fail() {
  echo "::error::release assets: $*" >&2
  exit 1
}

[ "$#" -eq 3 ] || {
  echo "usage: $0 <dir> <version> <archive count>" >&2
  exit 2
}
dir="$1"
version="$2"
count="$3"
[ -d "$dir" ] || fail "$dir is not a directory"
case "$count" in
  '' | *[!0-9]* | 0) fail "'$count' is not a positive archive count" ;;
esac

shopt -s nullglob
archives=("$dir"/crook-"$version"-*.tar.gz)
[ "${#archives[@]}" -eq "$count" ] || fail "expected $count archives for $version, found ${#archives[@]}"

expected=()
for archive in "${archives[@]}"; do
  [ -f "${archive}.sha256" ] || fail "$(basename "$archive").sha256 is missing"
  expected+=("$(basename "$archive")" "$(basename "$archive").sha256")
done

present=0
for file in "$dir"/* "$dir"/.[!.]*; do
  present=$((present + 1))
  name="$(basename "$file")"
  [ -f "$file" ] || fail "$name is not a file"
  found=false
  for want in "${expected[@]}"; do
    [ "$name" = "$want" ] && found=true
  done
  [ "$found" = true ] || fail "unexpected file $name"
done
[ "$present" -eq "${#expected[@]}" ] || fail "expected ${#expected[@]} files, found $present"

(
  cd "$dir"
  for archive in "${archives[@]}"; do
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "$(basename "$archive").sha256" || exit 1
    else
      shasum -a 256 -c "$(basename "$archive").sha256" || exit 1
    fi
  done
) || fail "a .sha256 does not match its archive"
echo "release assets: ok ($count archives)"
