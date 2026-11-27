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

[ -f "${archive}.sha256" ] || fail "$(basename "$archive").sha256 is missing next to the archive"
(
  cd "$(dirname "$archive")"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$(basename "$archive").sha256"
  else
    shasum -a 256 -c "$(basename "$archive").sha256"
  fi
) || fail "$(basename "$archive") does not match its .sha256"

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

echo "-- version"
version_line="$(run --version 2>/dev/null)"
echo "$version_line"
if [ -n "$expected" ]; then
  [ "$version_line" = "crook $expected" ] || fail "expected 'crook $expected', got '$version_line'"
else
  echo "$version_line" | grep -Eq '^crook [0-9]+\.[0-9]+\.[0-9]+' || fail "no version in '$version_line'"
fi

echo "-- help"
run --help >/dev/null 2>&1 || fail "crook --help failed"

link="$home/data/crook/current"
echo "-- doctor writes nothing without --fix"
# Nothing has made the link or the mod yet: `--version` and `--help` stop before the link refresh, and
# `doctor` and `setup` skip it, so what is checked here is setup's own work, not a side effect.
set +e
run doctor >/dev/null 2>&1
set -e
[ ! -e "$link" ] && [ ! -L "$link" ] || fail "doctor made $link"
[ ! -e "$app_home/mods" ] || fail "doctor installed something under $app_home/mods"

echo "-- setup"
set +e
out="$(run setup 2>&1)"
status=$?
set -e
[ "$status" -eq 0 ] || fail "crook setup exited $status: $out"
marker="$app_home/mods/crook/.crook-version"
[ -f "$marker" ] || fail "setup did not write $marker"
# The marker holds the plugin's version, which a release stamps to the binary's own.
plugin_version="$(sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$folder/plugin/.claude-plugin/plugin.json" | head -n 1)"
[ -n "$plugin_version" ] || fail "no version in the archive's plugin.json"
[ "$(tr -d '[:space:]' <"$marker")" = "$plugin_version" ] || fail "$marker does not hold $plugin_version"
if [ -n "$expected" ]; then
  [ "$plugin_version" = "$expected" ] || fail "the archive's plugin.json says $plugin_version, expected $expected"
fi
for leftover in RECIPE.md gpui-kit.d.ts jsconfig.json; do
  [ ! -e "$app_home/mods/crook/$leftover" ] || fail "setup installed $leftover"
done
[ -L "$link" ] || fail "setup did not make $link"
[ "$(cd "$link" && pwd -P)" = "$folder" ] || fail "$link points at $(readlink "$link"), not $folder"
[ -f "$link/plugin/.claude-plugin/plugin.json" ] || fail "$link/plugin is not the archive's plugin/"

echo "-- doctor"
# No hc here, so doctor exits 1; what matters is the report on standard output.
set +e
out="$(run doctor --json 2>/dev/null)"
set -e
for path in "$folder/plugin" "$folder/mod" "$app_home"; do
  printf '%s' "$out" | grep -qF "$path" || fail "doctor --json does not report $path: $out"
done
# There is no mise shim here, so the report says so and names the way round it (Q23).
printf '%s' "$out" | grep -qF CROOK_BIN || fail "doctor --json does not name CROOK_BIN: $out"
printf '%s' "$out" | grep -q '"id":"mod","status":"ok"\|"id": *"mod", *"status": *"ok"' || fail "doctor does not find the mod it was just given: $out"

echo "-- completion"
for shell in bash zsh fish; do
  # Standard output only: a warning on standard error is not part of the script.
  out="$(run completion print "$shell" 2>/dev/null)" || fail "crook completion print $shell failed"
  printf '%s\n' "$out" | grep -q crook || fail "completion for $shell does not name crook"
done

echo "-- hooks exit 0 on bad input (0020)"
run hook stop </dev/null >/dev/null 2>&1 || fail "crook hook stop </dev/null did not exit 0"
echo 'not json' | run hook session-start >/dev/null 2>&1 || fail "crook hook session-start with bad JSON did not exit 0"

echo "-- a release build without plugin/ is broken (0019)"
# Control: with plugin/ in place a command that works offline succeeds and never says "broken".
set +e
out="$(run completion print bash 2>&1 >/dev/null)"
status=$?
set -e
[ "$status" -eq 0 ] || fail "crook completion print bash failed with plugin/ present: $out"
case "$out" in *broken*) fail "crook says the install is broken with plugin/ present: $out" ;; esac

mv "$folder/plugin" "$folder/plugin.away"
set +e
out="$(run status 2>&1)"
status=$?
set -e
mv "$folder/plugin.away" "$folder/plugin"
[ "$status" -ne 0 ] || fail "crook status succeeded without plugin/"
# The refusal itself, not the `warning:` line the link refresh prints on every run.
printf '%s\n' "$out" | grep -v '^warning:' | grep -qF "install is broken" \
  || fail "crook status without plugin/ did not stop with 'the install is broken': $out"

echo "smoke: ok ($(basename "$archive"))"
