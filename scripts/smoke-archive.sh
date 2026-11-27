#!/usr/bin/env bash
# Runs a release archive the way a user's machine would: no inherited environment, and every
# home folder in a scratch directory. Used by _smoke-binary-archive.yml; run it locally too:
#
#   toolr ci stage-archive <triple> 0.0.0-ci --binary target/release/crook --dist "$TMPDIR/dist"
#   scripts/smoke-archive.sh "$TMPDIR/dist"/crook-0.0.0-ci-<triple>.tar.gz [expected-version]
#
# `expected-version` is the version `crook --version` must print; without it any version passes.
set -euo pipefail

usage() {
  echo "usage: $0 <archive.tar.gz> [expected-version]" >&2
  exit 2
}
[ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage
archive="$(cd "$(dirname "$1")" && pwd -P)/$(basename "$1")"
expected="${2:-}"

fail() {
  echo "::error::smoke: $*" >&2
  exit 1
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ -f "${archive}.sha256" ]; then
  (
    cd "$(dirname "$archive")"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum -c "$(basename "$archive").sha256"
    else
      shasum -a 256 -c "$(basename "$archive").sha256"
    fi
  )
fi

mkdir "$work/unpacked"
tar -xzf "$archive" -C "$work/unpacked"
shopt -s nullglob
tops=("$work"/unpacked/*/)
[ "${#tops[@]}" -eq 1 ] || fail "expected one top folder in $(basename "$archive"), found ${#tops[@]}"
shopt -u nullglob
folder="$(cd "${tops[0]}" && pwd -P)"
bin="$folder/crook"
[ -x "$bin" ] || fail "$bin is not an executable file"
for rel in plugin/.claude-plugin/plugin.json mod/mod.json; do
  [ -f "$folder/$rel" ] || fail "the archive has no $rel"
done

home="$work/home"
mkdir -p "$home/app"
home="$(cd "$home" && pwd -P)"
app_home="$home/app"

# No inherited env, and every folder crook reads or writes in the scratch home.
run() {
  env -i PATH=/usr/bin:/bin HOME="$home" \
    XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" \
    XDG_STATE_HOME="$home/state" XDG_CACHE_HOME="$home/cache" \
    HERDR_APP_HOME="$app_home" "$bin" "$@"
}

# `completion`, `setup` and `doctor` are not in the binary yet (binary plan). Until they are, a run that
# says so is a warning, not a pass, and the checks that follow it run once the commands exist. Remove
# this function when the last of them lands.
pending() {
  case "$1" in
    *"crook $2: not implemented"*)
      echo "::warning::smoke: 'crook $2' is not implemented yet, so its checks were skipped"
      return 0
      ;;
  esac
  return 1
}

echo "-- version"
version_line="$(run --version 2>/dev/null)"
echo "$version_line"
if [ -n "$expected" ]; then
  [ "$version_line" = "crook $expected" ] || fail "expected 'crook $expected', got '$version_line'"
else
  echo "$version_line" | grep -Eq '^crook [0-9]+\.[0-9]+\.[0-9]+' || fail "no version in '$version_line'"
fi
version="${version_line#crook }"

echo "-- help and completion"
run --help >/dev/null 2>&1 || fail "crook --help failed"
for shell in bash zsh fish; do
  set +e
  out="$(run completion print "$shell" 2>&1)"
  set -e
  pending "$out" completion && break
  printf '%s' "$out" | grep -q crook || fail "completion for $shell does not name crook"
done

echo "-- hooks exit 0 on bad input (0020)"
run hook stop </dev/null >/dev/null 2>&1 || fail "crook hook stop </dev/null did not exit 0"
echo 'not json' | run hook session-start >/dev/null 2>&1 || fail "crook hook session-start with bad JSON did not exit 0"

echo "-- setup"
set +e
out="$(run setup 2>&1)"
status=$?
set -e
if ! pending "$out" setup; then
  [ "$status" -eq 0 ] || fail "crook setup exited $status: $out"
  marker="$app_home/mods/crook/.crook-version"
  [ -f "$marker" ] || fail "setup did not write $marker"
  [ "$(tr -d '[:space:]' <"$marker")" = "$version" ] || fail "$marker does not hold $version"
  link="$home/data/crook/current"
  [ -L "$link" ] || fail "setup did not make $link"
  [ "$(cd "$link" && pwd -P)" = "$folder" ] || fail "$link points at $(readlink "$link"), not $folder"
  [ -f "$link/plugin/.claude-plugin/plugin.json" ] || fail "$link/plugin is not the archive's plugin/"
fi

echo "-- doctor"
set +e
out="$(run doctor --json 2>&1)"
set -e
if ! pending "$out" doctor; then
  for path in "$folder/plugin" "$folder/mod" "$app_home"; do
    printf '%s' "$out" | grep -qF "$path" || fail "doctor --json does not report $path: $out"
  done
  # There is no mise shim here, so the report says so and names the way round it (Q23).
  printf '%s' "$out" | grep -qF CROOK_BIN || fail "doctor --json does not name CROOK_BIN: $out"
fi

echo "-- a release build without plugin/ is broken (0019)"
mv "$folder/plugin" "$folder/plugin.away"
set +e
out="$(run status 2>&1)"
status=$?
set -e
[ "$status" -ne 0 ] || fail "crook status succeeded without plugin/"
printf '%s' "$out" | grep -qF "install is broken" || fail "crook status without plugin/ did not say the install is broken: $out"
mv "$folder/plugin.away" "$folder/plugin"

echo "smoke: ok ($(basename "$archive"))"
