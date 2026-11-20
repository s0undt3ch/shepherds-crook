#!/bin/sh
# Builds the throwaway data dir, work dir and hand-written deny settings (absolute paths, not committed).
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
xdg="$here/run/xdg"
rm -rf "$xdg" "$here/run/work" "$here/run/"*.log
mkdir -p "$here/run/work/sub"
(cd "$here/mkdb" && cargo run -q -- "$xdg" spike9)
cat > "$here/run/settings.json" <<JSON
{
  "permissions": { "deny": ["Read(/$xdg/**)", "Edit(/$xdg/**)"] },
  "hooks": {
    "PreToolUse": [
      { "matcher": "*", "hooks": [ { "type": "command", "command": "$here/run/hook.sh" } ] }
    ]
  }
}
JSON
cat "$here/run/settings.json"
