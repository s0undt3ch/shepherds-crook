#!/bin/bash
# Shared scaffold step. Runs in the empty eval workspace with HOME set to the run's own home.
# Puts the crook stub at the shim path the skills use when CROOK_BIN is unset (the harness does not
# pass CROOK_BIN through), and makes the workspace the flock folder.
# The stub keeps its own files (call log, coordinator mark, scaffolded children) in $HOME/.crook-stub,
# outside the workspace: a model that finds them in its folder inspects them and chains crook calls
# after the inspection. Graders read the model's Bash commands instead.
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
stub_state="$home/.crook-stub"
mkdir -p "$(dirname "$shim")" "$stub_state" "$workspace/briefs" "$workspace/results"
# Never write through a symlink.
rm -f "$shim"
template=$(cat "$lib/crook-stub.sh")
template=${template//__WORKSPACE__/$workspace}
printf '%s\n' "${template//__STATE__/$stub_state}" >"$shim"
chmod 755 "$shim"
: >"$stub_state/calls.log"

# The stub's SessionStart hook tells a coordinator its crook path (ADR 0027).
stub_mark_coordinator() { : >"$stub_state/coordinator"; }
# The `children` array the stub's `crook status` reports, read from stdin.
stub_children() { cat >"$stub_state/children.json"; }
