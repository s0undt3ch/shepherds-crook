#!/bin/bash
# Shared scaffold step. Runs in the empty eval workspace with HOME set to the run's own home.
# Puts the crook stub at the shim path the skills use when CROOK_BIN is unset (the harness does not
# pass CROOK_BIN through), and makes the workspace the flock folder.
#
# It writes under $HOME/.local/share/mise/shims, which on a real home is the maintainer's mise shim, so
# it refuses unless HOME and the workspace both sit under a temporary folder.
set -eu

physical() { (cd "$1" 2>/dev/null && pwd -P); }

workspace=$(pwd -P)
home=$(physical "${HOME:-/nonexistent}") || { echo "install.sh: HOME is not a folder" >&2; exit 1; }
allowed=("$(physical /tmp)" "$(physical /private/tmp 2>/dev/null || echo /tmp)")
[ -n "${TMPDIR:-}" ] && allowed+=("$(physical "$TMPDIR")")
for path in "$home" "$workspace"; do
    ok=no
    for base in "${allowed[@]}"; do
        case "$path" in "$base"/*) ok=yes ;; esac
    done
    if [ "$ok" = no ]; then
        echo "install.sh: refusing to run, $path is not under a temporary folder" >&2
        exit 1
    fi
done
# The path goes into a script and into JSON: plain characters only.
case "$workspace" in
*[!A-Za-z0-9._/+-]*)
    echo "install.sh: the workspace path has characters other than A-Z a-z 0-9 . _ / + -" >&2
    exit 1
    ;;
esac

lib=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
shim="$home/.local/share/mise/shims/crook"
mkdir -p "$(dirname "$shim")" "$workspace/briefs" "$workspace/results"
# Never write through a symlink.
rm -f "$shim"
template=$(cat "$lib/crook-stub.sh")
printf '%s\n' "${template//__WORKSPACE__/$workspace}" >"$shim"
chmod 755 "$shim"
: >"$workspace/.crook-calls.log"
