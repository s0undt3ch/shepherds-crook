#!/bin/sh
# usage: launch.sh <tag> [--bg]   (run run/setup.sh first)
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
tag=$1; shift
export XDG_DATA_HOME="$here/run/xdg" CROOK_PROJECT=spike9 CROOK_ROLE=child CROOK_CHILD=c1
export CROOK_BIN="$here/../../target/release/crook"
export SPIKE_LOG="$here/run/$tag-mod.log" SPIKE_HOOKLOG="$here/run/$tag-hook.log"
echo "shell CLAUDECODE=[${CLAUDECODE-unset}]" > "$here/run/$tag-shell.log"
cd "$here/run/work"
exec claude --model haiku --plugin-dir "$here/plugin" --settings "$here/run/settings.json" "$@" \
  --allowedTools "Bash(cd:*)" "Bash(pwd)" "mcp__crookspike__probe"
