#!/bin/bash
set -eu
# shellcheck source=/dev/null
. "$(dirname "$0")/../_lib/install.sh"
# The stub's SessionStart hook tells a coordinator its crook path (ADR 0027).
: >.stub-coordinator
