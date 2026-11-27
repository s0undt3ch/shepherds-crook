---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# Create herdr-app spaces with `herdr.space` labels

## Context and problem statement

Each effort should be one herdr-app space. herdr-app shows any terminal labelled
`herdr.space=<id>` and `herdr.tab=<id>`, and an id it does not know becomes a new space or tab.

## Considered options

- Invent ids (`crook-<effort>`) and label terminals; rename with `space.rename`.
- Call `herdr-app do space.new` and read back the id it chose.

## Decision outcome

Chosen option: invented ids through labels. The "New effort" dialog may still use `space.new`; children and
restarts use labels.

### Consequences

- Good, because hc alone can place a terminal; no herdr-app call is needed on the hot path.
- Bad, because behaviour with the app closed is untested.

### Confirmation

Spike 7 (S3) in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).
