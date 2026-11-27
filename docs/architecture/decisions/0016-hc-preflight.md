---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# Preflight checks before using hc

Amended by [0020](0020-mechanical-steps-run-through-crook.md): when crook starts the core, it removes
Claude's session variables instead of refusing.

## Context and problem statement

hc is at 0.1.x and may change. A core started from inside a Claude Code session passes that session's
variables to every child, which turns off the children's transcripts and so breaks `claude --resume`.

## Decision outcome

Before starting an effort, crook checks:

- `hc --version` is at least a pinned floor (0.1.22 today).
- `hc schema` has `terminal.run` with `labels`.
- The hc core is already running, or, if crook would start it, `CLAUDECODE` is not set in crook's
  environment. Otherwise crook stops and says why.

### Consequences

- Good, because API drift and the transcript leak fail early with a clear message.
- Bad, because the floor needs bumping as hc moves.

### Confirmation

Spikes 6 and 9 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).
