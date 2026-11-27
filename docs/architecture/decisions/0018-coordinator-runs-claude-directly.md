---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# The coordinator terminal runs `claude` directly

## Context and problem statement

[0011](0011-shell-hosted-coordinator.md) hosted the coordinator in a respawn loop so children survive a
Claude restart. The maintainer rejected it: `/exit` and Ctrl+C must end the coordinator, not restart it.

## Considered options

- The coordinator terminal runs `claude`; a lost coordinator is recovered by roster resume
  ([0017](0017-resume-children-from-a-roster.md)).
- The respawn loop ([0011](0011-shell-hosted-coordinator.md)).
- An interactive host shell, Claude started inside it.

## Decision outcome

Chosen option: the terminal's program is `claude --session-id <id> '<first task>'`, because `/exit` then
does what it says, and 0017 already restores the coordinator and its children from disk.

- When the coordinator exits, for any reason, its terminal ends and its children end with it (spike 4).
- Starting the effort again runs `claude --resume <id>` in a new terminal, which then resumes every
  `active` child from the roster as its own child (spike 11).
- `/exit` pauses an effort; it does not close it. Closing an effort is a separate, explicit action that marks
  the roster done.
- The child hook's status-app check ([0009](0009-child-stop-hook-reports-completion.md)) stays. It costs one
  `hc get`, and a send to an ended coordinator fails loudly anyway (spike 2), so the result file waits for
  the rescan.

### Consequences

- Good, because `/exit` and Ctrl+C behave as in any Claude session.
- Good, because there is no host shell, so sent text can never run as a shell command (spike 5).
- Good, because there is one recovery path for every failure, not two.
- Bad, because a coordinator restart now restarts its children too. In-flight commands are cut off part-way
  and a blocked child asks its question again (spike 11).

### Confirmation

Spikes 2, 4, 5 and 11 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).
