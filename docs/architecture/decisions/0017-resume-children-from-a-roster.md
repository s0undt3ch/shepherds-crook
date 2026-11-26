---
status: Proposed
date: 2026-11-26
decision-makers: maintainer
---

# Resume children from a roster after a restart

## Context and problem statement

When the coordinator's terminal ends (pane closed, core crash, reboot, or a coordinator killed outright),
its children end too. The respawn loop ([0011](0011-shell-hosted-coordinator.md)) only covers Claude
exiting inside a terminal that stays. The maintainer asked whether a restarted coordinator could start its
children again and have them carry on.

## Considered options

- A roster per effort; on start, the coordinator resumes every unfinished child by session id.
- `--outlive-parent` children that survive and are adopted again.
- Accept the loss and start fresh children.

## Decision outcome

Chosen option: a roster, plus the respawn loop. They cover different failures:

| Failure | Covered by |
| --- | --- |
| Claude exits or crashes in the coordinator's terminal | respawn loop (children keep running) |
| Terminal lost, core crash, reboot, coordinator killed | roster resume (children restarted) |

How it works:

- The roster lives in the effort's default location
  ([0012](0012-split-child-traffic-from-curated-notes.md)). One entry per child: name, session id, folder,
  template, brief path, state (`active`, `done`).
- Session ids come from a SessionStart hook in crook's settings, which every template command loads. It
  records the session id against `$HC_TERMINAL_ID`, on start and on resume. Passing `--session-id` through
  a template is not possible: hc drops extra arguments.
- The coordinator itself also has a fixed session id, so it can be resumed the same way.
- On start, for every `active` child that has no live terminal, the coordinator runs the template's command
  with `--resume <id>` appended (hc runs template commands through `sh -c`), in the child's folder, as its own
  child, with a resume prompt: check what is
  already done on disk, then finish without redoing completed work.
- Each brief asks the child to keep a progress checklist in its results file. That is the state a resumed
  child reads, and the same checklist addresses the weak spot recorded in
  [0009](0009-child-stop-hook-reports-completion.md).

### Consequences

- Good, because one mechanism covers every way a terminal can be lost.
- Good, because a resumed child remembers its conversation and its brief.
- Bad, because a command running at the moment of the kill is cut off part-way (it ran on for 10 to 15
  seconds, then stopped). Children must be able to redo or check their last step.
- Bad, because a child blocked on a question asks again after resuming.

### Confirmation

Spikes 8, 11, 12 and 13 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md). Not tested:
a real reboot.

## Pros and cons of the options

### `--outlive-parent` children, adopted again

- Bad, because hc has no re-parent call; notes keep going to the dead address (spike 4).
- Bad, because it does not help after a reboot, when nothing survives.

### Start fresh children

- Bad, because the children lose their conversation and the coordinator has to brief them again.
