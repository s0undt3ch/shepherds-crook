#!/usr/bin/env bash
# install.sh <run.env> <version> [<binary>]: stages a release-shaped archive folder with `toolr ci stage-archive`
# (so the layout has one source, 0019) into $RUN/install/<version>/, and writes the fake mise shim
# $RUN/shims/crook, which runs the version named in $RUN/active. E2E_STAGED=<folder> installs that folder
# instead of staging one. With --no-activate the shim keeps its current version.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=lib.sh
source "$here/lib.sh"
load_run "${1:-}"
version="${2:-}"
shift 2 || die "usage: install.sh <run.env> <version> [<binary>] [--no-activate]"
activate=1
binary=''
for arg in "$@"; do
  if [ "$arg" = "--no-activate" ]; then activate=0; else binary="$arg"; fi
done
case "$version" in '' | *[!A-Za-z0-9._-]*) die "not a version: '$version'" ;; esac
dest="$INSTALL/$version"
[ ! -e "$dest" ] || die "version $version is already installed in the run"

if [ -n "${E2E_STAGED:-}" ]; then
  [ -d "$E2E_STAGED" ] || die "E2E_STAGED is not a folder: $E2E_STAGED"
  stage="$E2E_STAGED"
else
  [ -x "$binary" ] || die "no crook binary at '$binary'; pass one (cargo build --release)"
  triple="${E2E_TRIPLE:-$(rustc -vV 2>/dev/null | sed -n 's/^host: //p')}"
  [ -n "$triple" ] || triple="$(uname -m)-unknown-$(uname -s | tr '[:upper:]' '[:lower:]')"
  out="$RUN_TMP/stage"
  mkdir -p "$out"
  (cd "$here/../.." && toolr ci stage-archive "$triple" "$version" --binary "$binary" --out "$out") >&2
  stage="$out/crook-$version-$triple"
  [ -d "$stage" ] || die "stage-archive made no $stage"
fi
[ -x "$stage/crook" ] || die "$stage/crook is not an executable file"
cp -R "$stage" "$dest"
# A release stamps the plugin's version; so does the run, so two installs can be told apart (S7, S13). The
# staged copy is the run's own, never the repository's plugin.json.
plugin_json="$dest/plugin/.claude-plugin/plugin.json"
[ -f "$plugin_json" ] || die "$plugin_json is missing"
jq --arg v "$version" '.version = $v' "$plugin_json" >"$plugin_json.new" || die "jq could not stamp $plugin_json"
mv "$plugin_json.new" "$plugin_json"

# The shim reads the active version on every call, as mise's shim runs whatever is active.
{
  printf '#!/bin/sh\n'
  # shellcheck disable=SC2016 # the shim's own variables, expanded when it runs
  printf 'v=$(cat %q) || exit 1\n' "$RUN/active"
  # shellcheck disable=SC2016
  printf 'exec %q/"$v"/crook "$@"\n' "$INSTALL"
} >"$SHIM"
chmod +x "$SHIM"
if [ "$activate" = 1 ]; then printf '%s\n' "$version" >"$RUN/active"; fi
echo "$dest"
