---
number: 20
title: Mechanical steps run through `crook` subcommands
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
links:
- target: 7
  kind: amends
- target: 8
  kind: amends
- target: 9
  kind: amends
- target: 16
  kind: amends
- target: 17
  kind: amends
- target: 27
  kind: amendedby
- target: 28
  kind: amendedby
- target: 29
  kind: amendedby
---

# Mechanical steps run through `crook` subcommands

Amended by [0029](0029-a-flocks-panel-and-each-flock-s-task-links-and-repositories.md) (proposed): a new `crook edit` command, and `--link` and `--repo` on `crook new`.

Amended by [0027](0027-the-coordinator-calls-crook-by-a-literal-path.md): the coordinator calls crook by the
literal path `crook open` resolves from the shim, not by the shim form; hooks and the mod keep the shim form.

Amended by [0028](0028-the-coordinator-s-hc-calls-and-brief-writes-under-default-mode.md): the coordinator talks to
its children with `crook send` and `crook read` and makes no `hc` call; `crook open` also allows its brief, README
and notes writes.

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
- As few subcommands as the steps need.

## Considered options

- `crook` subcommands; the model, the hooks and the mod call them.
- The status quo: skill text and shell scripts in the plugin.

## Decision outcome

Chosen option: `crook` subcommands.

| Command | Called by | Does |
| --- | --- | --- |
| `crook new <slug>` | The mod's "New flock" dialog, or the maintainer | Creates the flock folder ([0004](0004-flock-storage-location.md)) and its roster with the coordinator's session id, then does what `open` does |
| `crook open <slug>` | The mod, or the maintainer | Starts the coordinator terminal with its `herdr.space`, `crook.flock` and `crook.version` labels ([0015](0015-spaces-from-herdr-labels.md)): `--session-id <id>` the first time, `--resume <id>` after that ([0018](0018-coordinator-runs-claude-directly.md)) |
| `crook close <slug>` | The mod, or the maintainer | Marks the roster done, so the flock is closed rather than paused (0018) |
| `crook spawn <role> <child> <brief>` | The coordinator, through Bash | Picks the session id, writes the roster entry, then runs `hc run` with the flags in a fixed order and `--parent "$HC_TERMINAL_ID"` (0007) |
| `crook stop <child>` | The coordinator | Marks the child `done` in the roster and ends its terminal, so a later resume does not bring it back |
| `crook resume [<child>]` | The coordinator's SessionStart hook, or the maintainer | Resumes one child, or every `active` child with no live terminal, under its original name (0017) |
| `crook status --json` | The mod and the coordinator | Merges the roster, `hc ls` labels and result marks |
| `crook hook <event>` | The plugin's hook config | See below |
| `crook completion print <shell>` | mise, through the release manifest (below) | Prints tab completion for bash, zsh and fish, so the slugs and child names above complete |

`new` and `open` stay separate so a mistyped slug fails instead of creating a flock.

### Completion

The release manifest declares completion as a command, as v1's `.github/packslip.toml` did:

```toml
bin = ["crook"]

[[resource]]
kind = "completion"
shells = ["bash", "zsh", "fish"]
exec = ["crook", "completion", "print", "{shell}"]
```

packslip requires an `exec` completion to list its shells, put `{shell}` in its command, and start with one of
the release's `bin` names ([packslip resources guide](https://packslip.dev/docs/resources/)). mise runs the
command the first time the maintainer presses Tab, caches the output per version and shell, and loads it when
mise is activated in the shell; it does not run it at install time
([mise packslip resources](https://mise.jdx.dev/dev-tools/packslip-resources.html)).

### Hooks

The plugin's hook config only calls `crook hook <event>`:

- `session-start` records the plugin version (0019). In a coordinator it also runs `resume`, so resuming no
  longer depends on the model.
- `stop` does the result check, the status-app check and the send (0008, 0009), and the self-reload (0019).
- `user-prompt-submit` does the results rescan (0008).

A hook must never break the session. `crook hook` exits 0 on any failure, writes errors to stderr only, and
catches panics, as v1's hook did (`src/hook/mod.rs` on `backup_crook_v1`). A failed send already leaves the
result file for the rescan (0008), so nothing is lost.

### Preflight

Every command that touches hc runs the 0016 preflight first. When no core is running, crook starts one with
`hc server start` before any other hc call, with the variables a running Claude session sets removed, instead
of refusing. The call that starts the core fixes the environment its terminals get; later callers' variables
do not reach them (spike 23). The variables:

- `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_EFFORT`, `AI_AGENT` (D93 in [`docs/plans/build-log.md`](../../plans/build-log.md));
- `CLAUDE_CODE_SESSION_*`, `CLAUDE_CODE_CHILD_*`, `CLAUDE_CODE_MESSAGING_*`;
- `CLAUDE_CODE_ENTRYPOINT`, `CLAUDE_CODE_EXECPATH`;
- `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET`, `HC_SCOPE`: the calling terminal's identity.

It keeps `CLAUDE_CONFIG_DIR` and every other variable. The real herdr-app core passes `CLAUDE_CONFIG_DIR` to
its shells (spike 6). Other `CLAUDE_CODE_*` variables are user settings, from the login shell or the `env`
block of Claude's `settings.json`.

The rest of 0007, 0008, 0009, 0016 and 0017 stands: the same commands, files and checks, run by crook.

### Consequences

- Good, because the launch command, roster writes and resume are the same every time.
- Good, because resume no longer depends on the coordinator remembering to do it.
- Good, because hooks and commands get unit tests instead of end-to-end spikes only.
- Bad, because more Rust must be written and tested before the first real flock.
- Bad, because the variable list follows Claude Code's internals and can go stale.
- Good, because hooks and the mod call `crook` through mise's shim ([0019](0019-ship-crook-as-a-binary.md)),
  which runs the active version even in a core started before an upgrade (spike 22).

### Confirmation

Spike 23: a scratch core started from inside a Claude session with the variables above removed gave its
children only user settings and no transcript warning, even for children started later from a caller that
still had every variable.

After the first build, run spikes 14 and 15 again through `crook spawn`, `crook hook` and `crook resume`
instead of hand-built commands.

## Pros and cons of the options

### The status quo: skill text and shell scripts

- Good, because there is nothing to build beyond the plugin.
- Bad, because the model can get the launch command wrong without anyone noticing (spike 16).
- Bad, because hook logic in shell is hard to test and depends on the tools installed on each machine.

## More information

Looked at in v1 and left out:

- Typed tools (`$.tool.register`) backed by a JSON verb interface. `crook spawn` takes three plain arguments
  and builds the risky command itself, so a typed tool adds a TypeScript layer and a second interface for
  little gain.
- `list` and `wipe`: not needed for the steps above. Add them when they are.
