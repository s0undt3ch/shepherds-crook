#!/bin/bash
# Runs the plugin's eval suite (docs/plans/2026-11-27-crook-plugin.md, Tier 2). It spends model credit,
# so it is a manual step: `mise run plugin-eval`, or this script. Extra arguments go to
# `claude plugin eval` (for example `--case child-done --runs 1`).
#
# Safety: no case may reach the real hc, herdr-app or crook. The cases get a PATH that holds only
# `claude` and the system folders, a scratch HOME of their own and the stub in plugin/evals/_lib.
# The harness does not pass CROOK_BIN through, so the stub sits at the shim path instead.
#
# Set CLAUDE_BIN to choose the claude binary.
set -uo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
claude=${CLAUDE_BIN:-}
if [ -z "$claude" ]; then
    # The first `claude` on PATH that is a binary. A `claude` that is a shell script is a wrapper
    # that searches PATH for the real one, and it cannot find it on the scratch PATH below.
    while IFS= read -r candidate; do
        if [ "$(head -c 2 "$candidate")" != "#!" ]; then
            claude=$candidate
            break
        fi
    done < <(type -ap claude)
fi
[ -n "$claude" ] || { echo "plugin-eval: no claude binary on PATH; set CLAUDE_BIN" >&2; exit 1; }
bin=$(mktemp -d "${TMPDIR:-/tmp}/crook-eval-bin.XXXXXX")
trap 'rm -rf "$bin"' EXIT
ln -s "$claude" "$bin/claude"

# Haiku runs the cases: all they check is whether the skill text steers a model.
# One arm only: the no-plugin arm has no skills, so it would measure nothing and double the cost.
# The threshold works with the grader weights (safety graders weigh 20, the rest 1):
# tests/tools/ci/test_plugin_evals.py checks the arithmetic for every case.
env -u HC_TERMINAL_SOCKET -u HC_SCOPE -u HC_TERMINAL_ID -u HC_SESSION -u CROOK_BIN -u MISE_DATA_DIR \
    PATH="$bin:/usr/bin:/bin:/usr/sbin:/sbin" "$bin/claude" plugin eval "$root/plugin" \
    --scaffold --trust-plugin --no-publish --ablation none \
    --model haiku --threshold 0.96 --max-cost-usd 3 -j 2 \
    --allow-tools Write Edit Bash \
    "$@"
status=$?
exit "$status"
