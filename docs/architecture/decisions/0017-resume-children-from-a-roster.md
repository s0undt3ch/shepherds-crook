---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# Resume children from a roster after a restart

Amended by [0020](0020-mechanical-steps-run-through-crook.md): `crook spawn` writes the roster entry, and
the coordinator's SessionStart hook runs `crook resume`.

## Context and problem statement

When the coordinator's terminal ends (`/exit`, pane closed, core crash, reboot, or a coordinator killed
outright), its children end too ([0018](0018-coordinator-runs-claude-directly.md)). The maintainer asked
whether a restarted coordinator could start its children again and have them carry on.

## Considered options

- A roster per flock; on start, the coordinator resumes every unfinished child by session id.
- `--outlive-parent` children that survive and are adopted again.
- Accept the loss and start fresh children.

## Decision outcome

Chosen option: a roster. It is the only recovery path: the coordinator exiting, crashing, losing its
terminal or its core, and a reboot all end in the same place.

How it works:

- The roster lives in the flock's default location
  ([0012](0012-split-child-traffic-from-curated-notes.md)). One entry per child: name, session id, folder,
  role, brief path, state (`active`, `done`).
- The coordinator picks each child's session id and writes its roster entry before launching it with
  `--session-id` ([0007](0007-launch-children-with-hc-run.md)).
- The coordinator itself also has a fixed session id, so it can be resumed the same way.
- Children are resumed under their original hc name, so the roster keeps one entry per child. hc allows the
  name again once the old terminal has finished (spike 15).
- On start, for every `active` child that has no live terminal, the coordinator runs
  `claude --plugin-dir <crook> --agent <role> --resume <id> "<resume prompt>"` in the child's folder, as its
  own child. The resume prompt: check what is already done on disk, then finish without redoing completed
  work.
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

Spikes 8, 11, 13, 15 and 16 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md). Spike 15
stopped the whole core and had the coordinator resume its children from the roster. Not tested: what hc
and herdr-app restore at login after a real reboot.

## Pros and cons of the options

### `--outlive-parent` children, adopted again

- Bad, because hc has no re-parent call; notes keep going to the dead address (spike 4).
- Bad, because it does not help after a reboot, when nothing survives.

### Start fresh children

- Bad, because the children lose their conversation and the coordinator has to brief them again.
