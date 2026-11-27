---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Mechanical steps run through `crook` subcommands

## Context and problem statement

Before [0019](0019-ship-crook-as-a-binary.md) crook had no executable, so every step had to be done by the
coordinator model following skill text, or by a shell script in the plugin's hooks:

- The coordinator builds the `hc run … claude --plugin-dir … --agent … --session-id … "<prompt>"` command
  and writes the roster entry before it ([0007](0007-launch-children-with-hc-run.md),
  [0017](0017-resume-children-from-a-roster.md)). One wrong flag drops the prompt without an error: a
  variadic flag such as `--allowedTools` swallows it, and Claude sits idle (spike 16).
- On start, the coordinator compares the roster with `hc ls` and resumes each lost child (0017).
- Hook scripts track result checksums, check the coordinator's status app, send paths
  ([0008](0008-messages-over-hc-send-results-folder-inbox.md), [0009](0009-child-stop-hook-reports-completion.md))
  and reload after an upgrade (0019).
- Preflight refuses to start a core when `CLAUDECODE` is set ([0016](0016-hc-preflight.md)).

None of these steps needs judgement. The `crook` binary now ships with every install, so they can run as code.

## Decision drivers

- Steps that must be exact should not depend on a model getting them right each time.
- One implementation, testable, with no dependence on `sh`, `jq` or platform quirks.
- The model keeps the work that needs judgement: planning, briefs, reading results, talking to the maintainer.

## Considered options

- `crook` subcommands; skills and hooks call them.
- The status quo: skill text and shell scripts in the plugin.

## Decision outcome

Chosen option: `crook` subcommands.

| Command | Replaces | Does |
| --- | --- | --- |
| `crook new <slug>` | The mod's "New effort" steps | Creates the effort folder ([0004](0004-effort-storage-location.md)), picks the coordinator's session id, writes it to the roster, starts the coordinator terminal with its `herdr.space`, `crook.effort` and `crook.version` labels ([0015](0015-spaces-from-herdr-labels.md)) |
| `crook start <effort>` | Manual `claude --resume` ([0018](0018-coordinator-runs-claude-directly.md)) | Starts the coordinator again with `--resume <id>` in a new terminal |
| `crook spawn <role> <name> --brief <path>` | The coordinator's `hc run` command (0007) | Picks the session id, writes the roster entry, then runs `hc run` with the flags in a fixed order and `--parent "$HC_TERMINAL_ID"` |
| `crook resume` | The coordinator's start-up loop (0017) | For every `active` roster entry with no live terminal, resumes the child under its original name |
| `crook close <effort>` | The explicit close in 0018 | Marks the roster done |
| `crook status [--json]` | Parsing in the mod and the coordinator | Merges the roster, `hc ls` labels and result marks |
| `crook hook <event>` | Hook shell scripts (0008, 0009, 0019) | `session-start` records the version and, in a coordinator, runs `resume`; `stop` does the result check, status-app check, send and self-reload; `user-prompt-submit` does the results rescan |

- The plugin's `hooks.json` only calls `crook hook <event>`.
- The coordinator skill tells the model to use `crook spawn`, never a raw `hc run`.
- The mod calls `exec("crook", [...])` for `new`, `start`, `close` and `status`.
- Every subcommand that touches hc runs the 0016 preflight first. When crook starts a core itself, it
  removes `CLAUDECODE` and the `CLAUDE_CODE_*` variables from the core's environment instead of refusing;
  a core started that way gives its children a clean environment (spike 6).

The rest of 0007, 0008, 0009, 0016 and 0017 stands: the same commands, files and checks, run by crook.

### Consequences

- Good, because the launch command, roster writes and resume loop are the same every time.
- Good, because resume no longer depends on the coordinator remembering to do it.
- Good, because hooks and commands get unit tests instead of end-to-end spikes only.
- Bad, because more Rust must be written and tested before the first real effort.
- Bad, because hooks now need `crook` on the PATH inside hc-launched sessions. Not checked: spike 21 tested
  only herdr-app's `exec`. If it is missing, hooks call `${CLAUDE_PLUGIN_ROOT}/../crook`, also untested.

### Confirmation

Not yet tested. Before acceptance:

- Check that `crook` is on the PATH of a Claude session started by `hc run` from the real core.

After the first build, run spikes 14 and 15 again through `crook spawn`, `crook hook` and `crook resume`
instead of hand-built commands.

## Pros and cons of the options

### The status quo: skill text and shell scripts

- Good, because there is nothing to build beyond the plugin.
- Bad, because the model can get the launch command wrong without anyone noticing (spike 16).
- Bad, because hook logic in shell is hard to test and depends on the tools installed on each machine.

## More information

When this is accepted, add "amended by 0020" notes to 0007, 0008, 0009, 0016 and 0017.
