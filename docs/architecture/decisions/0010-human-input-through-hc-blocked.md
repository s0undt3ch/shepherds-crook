---
status: Proposed
date: 2026-11-26
decision-makers: maintainer
---

# Human input goes through hc's `blocked` status

## Context and problem statement

A child sometimes needs the maintainer. v1 had its own need and park flow for this.

## Decision outcome

Drop need and park. A child asks with AskUserQuestion (or hits a permission prompt). hc reports it as
`blocked`; herdr-app shows that on the space and tab and plays its sound; hc types a note into the
coordinator. The maintainer answers in the child's pane.

### Consequences

- Good, because there is no crook code for this at all.
- Bad, because the coordinator cannot answer for the maintainer: `hc send` to a blocked child is refused
  unless `--force`, which would answer the dialog.

### Confirmation

Spike 3 (S1) in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).
