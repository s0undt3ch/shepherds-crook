---
status: Proposed
date: 2026-11-26
decision-makers: maintainer
---

# Host the coordinator in a shell

## Context and problem statement

Children are linked to their parent terminal's address. If the coordinator's terminal program is `claude`,
restarting Claude ends the terminal: children end with it, or, with `--outlive-parent`, live on with notes
going to a dead address. hc has no re-parent call.

## Considered options

- The coordinator terminal runs a shell; Claude runs inside it.
- The coordinator terminal runs `claude`; children use `--outlive-parent`.
- The coordinator terminal runs a loop that starts Claude again when it exits.

## Decision outcome

Chosen option: a host process that outlives Claude, so the address survives restarts. Use a respawn loop
rather than an interactive shell, because of the hazard below.

While Claude is not running in an interactive host shell:

- parent notes are dropped;
- `hc send` succeeds and the shell runs the text as a command.

So the child hook checks the coordinator's status app before sending
([0009](0009-child-stop-hook-reports-completion.md)), and the coordinator rescans results when it starts.

### Consequences

- Good, because children and notes survive a coordinator restart without `--outlive-parent`.
- Bad, because the respawn loop is untested.

### Confirmation

Spikes 4 and 5 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md). Respawn loop: not yet.
