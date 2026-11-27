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

None of these steps needs judgement. v1 (`backup_crook_v1`) already ran them as a Rust binary with user verbs,
a hook entry point, a JSON verb interface and typed tools for the model. The redesign reuses that shape.

## Decision drivers

- Steps that must be exact should not depend on a model getting them right each time.
- One implementation, testable, with no dependence on `sh`, `jq` or platform quirks.
- The model keeps the work that needs judgement: planning, briefs, reading results, talking to the maintainer.
- Reuse v1's verbs, interfaces and code where the redesign still needs them.

## Considered options

- `crook` subcommands in v1's shape; skills, hooks and the mod call them.
- The status quo: skill text and shell scripts in the plugin.

## Decision outcome

Chosen option: `crook` subcommands in v1's shape.

### User verbs

Kept from v1, with the same names:

| Verb | v1 | Now |
| --- | --- | --- |
| `crook init <slug>` | Created the project record | Creates the effort folder ([0004](0004-effort-storage-location.md)) and its roster, with the coordinator's session id |
| `crook open <slug>` | Started the orchestrator | Starts the coordinator terminal with its `herdr.space`, `crook.effort` and `crook.version` labels ([0015](0015-spaces-from-herdr-labels.md)): `--session-id <id>` the first time, `--resume <id>` after that ([0018](0018-coordinator-runs-claude-directly.md)) |
| `crook resume <child>` | Restarted a child's session | Resumes one child under its original name (0017) |
| `crook stop <child>` | Archived a child, stopped it, removed its worktrees | Marks the child `done` in the roster and ends its terminal |
| `crook list` | One line per project | One line per effort |
| `crook status` | Every live child across projects | Merges the roster, `hc ls` labels and result marks |
| `crook wipe <slug>` | Removed a finished project's state | Removes a finished effort's default-location folder; never the notes root ([0004](0004-effort-storage-location.md)) |
| `crook completion print <shell>` | Shell completion, wired into the packslip manifest | Same |

New: `crook close <slug>` marks the roster done, so the effort is closed rather than paused (0018).

### Hooks

`crook hook <Event>`, as in v1. The plugin's hook config calls only this. It keeps v1's contract: routed before
argument parsing, always exits 0 with a defined answer, and a panic never reaches stdout.

- `SessionStart` records the plugin version (0019). In a coordinator it also resumes every `active` child that
  has no live terminal, with the same code as `crook resume`, so resume no longer depends on the model.
- `Stop` does the result check, the status-app check and the send (0008, 0009), and the self-reload (0019).
- `UserPromptSubmit` does the results rescan (0008).

### The model's tools

v1's plugin registered typed tools with `$.tool.register` (`mcp__crook__<name>`); each one ran
`crook mod <verb>`, one JSON object in on stdin, one out on stdout. Kept for:

- `spawn` (coordinator only): role, child name, brief path. crook picks the session id, writes the roster
  entry, then runs `hc run` with the flags in a fixed order and `--parent "$HC_TERMINAL_ID"` (0007). The model
  never builds an `hc run` command.
- `status`: the same data as `crook status`.

The herdr-app mod uses the same `crook mod <verb>` interface through `exec`.

### Preflight

Every verb that touches hc runs the 0016 preflight first, as v1 gated every user verb. When crook starts a
core itself, it removes the variables a running Claude session sets, instead of refusing:

- `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_EFFORT`;
- `CLAUDE_CODE_SESSION_*`, `CLAUDE_CODE_CHILD_*`, `CLAUDE_CODE_MESSAGING_*`;
- `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`;
- `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET`, `HC_SCOPE`: the calling terminal's identity.

It keeps `CLAUDE_CONFIG_DIR` and every other variable. The real herdr-app core passes `CLAUDE_CONFIG_DIR` to
its shells (spike 6). Other `CLAUDE_CODE_*` variables are user settings, from the login shell or the `env`
block of Claude's `settings.json`.

### Not carried over from v1

| v1 | Why not |
| --- | --- |
| `crook launch` (hidden) | A herdr pane could only type a command; `hc run` takes the argv directly |
| `park`, `unpark`, `need`, `take_task`, `done` | Replaced by hc's `blocked` status ([0010](0010-human-input-through-hc-blocked.md)) and the Stop hook (0009) |
| `project`, `project-set-*` | The coordinator writes the effort's `README.md` ([0012](0012-split-child-traffic-from-curated-notes.md)) |
| `worktree` | No worktree handling in the redesign yet |
| `next` | herdr-app shows which child is blocked; whether it can jump there is not checked |
| SQLite store | Files and labels ([0014](0014-state-in-files-and-labels.md)) |

The rest of 0007, 0008, 0009, 0016 and 0017 stands: the same commands, files and checks, run by crook.

### Consequences

- Good, because the launch command, roster writes and resume are the same every time.
- Good, because resume no longer depends on the coordinator remembering to do it.
- Good, because much of v1's code and tests carry over: verb gate, hook entry, JSON verb interface,
  completion.
- Bad, because more Rust must be written and tested before the first real effort.
- Bad, because the variable list follows Claude Code's internals and can go stale.
- Bad, because hooks and tools find `crook` on the PATH, as v1's did. Not checked for a session started by
  `hc run` from the real core: spike 21 tested only herdr-app's `exec`.

### Confirmation

Not yet tested. Before acceptance:

- Check that `crook` is on the PATH of a Claude session started by `hc run` from the real core.
- Check that `$.tool.register` tools load from a plugin passed with `--plugin-dir` in an `--agent` session.

After the first build, run spikes 14 and 15 again through `spawn`, `crook hook` and `crook resume` instead
of hand-built commands.

## Pros and cons of the options

### The status quo: skill text and shell scripts

- Good, because there is nothing to build beyond the plugin.
- Bad, because the model can get the launch command wrong without anyone noticing (spike 16).
- Bad, because hook logic in shell is hard to test and depends on the tools installed on each machine.

## More information

v1's source is on `backup_crook_v1`: `src/main.rs` (verb routing), `src/hook/mod.rs`, `src/modverb.rs`,
`plugin/hooks/core/tools.ts`, `src/launch/mod.rs` (removed variables).

When this is accepted, add "amended by 0020" notes to 0007, 0008, 0009, 0016 and 0017.
