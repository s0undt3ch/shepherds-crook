---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# A child Stop hook reports completion

Amended by [0020](0020-mechanical-steps-run-through-crook.md): the hook below is `crook hook stop`.

## Context and problem statement

The coordinator must learn when a child is done. hc notes the parent when a child blocks, errors or exits,
but not when a turn ends. The coordinator cannot block on `hc wait` and still talk to the maintainer.

## Considered options

- A Claude Code Stop hook in each child that reports its result.
- Trust the child to run `hc send` itself when done.
- The coordinator waits with `hc wait --children any`.

## Decision outcome

Chosen option: a Stop hook in each child. On each turn end it:

1. Does nothing unless `results/<child>.md` is new or changed since the last report.
2. Runs `hc get <coordinator>` and sends only if the status app is `claude-code`
   (see [0018](0018-coordinator-runs-claude-directly.md)).
3. Runs `hc send <coordinator> "<result path>"`. On failure it leaves the file for the rescan
   ([0008](0008-messages-over-hc-send-results-folder-inbox.md)).

### Consequences

- Good, because reporting does not depend on the child remembering.
- Bad, because the hook ships with crook and must be installed for children.

### Confirmation

Spike 1 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): a turn end sends no note.
Spike 14: a throwaway version of this hook reported a changed result, stayed silent on an unchanged turn,
and held a result while the coordinator was blocked.

## Pros and cons of the options

### `hc wait --children any`

- Bad, because the coordinator's shell call blocks it: no talking to the maintainer while it waits.

## More information

Accepted for now, with a known weak spot: a child doing several things in one brief. The hook reports when
the result file changes, so a child that rewrites it after each step pings the coordinator each time, and
one that writes it once only at the end reports nothing in between. Nothing checks the work against a
checklist. Revisit once real flocks show which way it breaks.
