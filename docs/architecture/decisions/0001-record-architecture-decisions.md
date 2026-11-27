---
number: 1
title: "Record architecture decisions with MADR"
status: accepted
date: 2026-11-26
decision-makers:
- maintainer
---

# Record architecture decisions with MADR

## Context and problem statement

crook v1 kept its design in long spec files under `specs/`. They drifted, batched up 28 pending edits, and
mixed decided calls with guesses. The redesign starts from scratch and needs a record that keeps each
decision small, dated, and marked as decided or not.

## Considered options

- MADR files in `docs/architecture/decisions/`.
- Nygard-style ADRs (title, status, context, decision, consequences).
- Long-form specs, as in v1.

## Decision outcome

Chosen option: MADR, because it lists the rejected options next to the chosen one, and the rejections are
half the value here.

### Consequences

- Good, because each call is one short file with a status.
- Good, because a Proposed status separates inferred design calls from the maintainer's decisions.
- Bad, because cross-cutting changes touch several files; the index in `docs/architecture/README.md` must
  stay current.

## More information

The v1 specs stay readable on the `backup_crook_v1` branch.
