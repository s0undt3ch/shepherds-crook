---
number: 27
title: "The coordinator calls crook by a literal path"
status: proposed
date: 2026-11-28
decision-makers:
- maintainer
---

# The coordinator calls crook by a literal path

## Context and problem statement

The coordinator skill writes every `crook` call as
`"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" …`
([0019](0019-ship-crook-as-a-binary.md), Q20), and `crook open` passes `--settings` with an allow rule for that
form (Q21). Claude Code refuses to match any command that holds a variable against an allow rule, so that rule can
never match: under `default` mode every call is refused or asks the maintainer. A literal absolute path matches
(spike 27).

## Decision drivers

- The coordinator's calls must not ask the maintainer each time.
- A call must still reach the version mise has active now, or `CROOK_BIN` in development (0019).
- The path must survive `/clear`, compaction and a resume.

## Considered options

- Option A: `crook open` resolves the shim form to one literal absolute path and hands it to the coordinator.
- Option B: keep the shim form and let the maintainer approve or allow each call.
- Option C: put `crook` on the session's `PATH` and call it bare.

## Decision outcome

Chosen option: Option A, because it is the only form an allow rule matches, and it keeps 0019's shim.

- `crook open` resolves `CROOK_BIN`, else the shim path, to an absolute path with no variables. It refuses a path
  that holds a double quote.
- It passes `--settings` with the allow rule `Bash("<path>" *)`.
- The first prompt names the path. The coordinator's SessionStart hook repeats it in `additionalContext` on every
  source (`startup`, `resume`, `clear`, `compact`), resolved the same way from the hook's own environment.
- The coordinator skill tells the model to call crook only as that quoted path, exactly as given.
- Hooks and the mod keep the shim form: they do not go through Claude's Bash permission check.
- Children run no `crook` commands, so nothing changes for them.

### Consequences

- Good, because the coordinator's calls run with no prompt under `default` mode.
- Good, because the shim still picks the active version, so an upgrade needs no new path.
- Bad, because a path that moves while a coordinator runs (a changed `CROOK_BIN`, a moved mise data folder)
  needs a new session.
- Bad, because the skill depends on context the hook injects, and a model that ignores it falls back to asking.

### Confirmation

- A unit test: `settings_json()` holds no `$`, and its rule matches the path in the first prompt.
- A hook test: the coordinator's `session-start` emits the path for each source.
- Tier 3 under `--permission-mode default`: the coordinator's `crook status` runs with no denial.

## Pros and cons of the options

### Option B

- Good, because nothing changes.
- Bad, because the maintainer approves every call, or allows every Bash command.

### Option C

- Good, because the call is short.
- Bad, because a bare `crook` finds an old version's folder after an upgrade (spike 22, 0019).

## More information

- Spike 27 in [the spikes](../research/2026-11-26-hc-herdr-app-spikes.md).
- Build log D103 and D105; open question Q46.
- If accepted, it amends [0019](0019-ship-crook-as-a-binary.md) and
  [0020](0020-mechanical-steps-run-through-crook.md); record that with `adrs link` then.
