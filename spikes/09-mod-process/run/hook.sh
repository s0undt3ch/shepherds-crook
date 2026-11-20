#!/bin/sh
# PreToolUse command hook: appends the input's cwd, tool and command to $SPIKE_HOOKLOG.
jq -c '{cwd, tool: .tool_name, cmd: .tool_input.command}' >> "$SPIKE_HOOKLOG"
