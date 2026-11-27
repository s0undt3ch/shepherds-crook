---
number: 14
title: "State in files and hc labels, no database"
status: accepted
date: 2026-11-26
decision-makers:
- maintainer
---

# State in files and hc labels, no database

## Context and problem statement

v1 kept flock and child state in SQLite and reconciled it with herdr. hc already knows every terminal, its
parent, status, labels and exit.

## Considered options

- Files (plan, briefs, results) plus `crook.*` labels on terminals; rebuild the live view from `hc ls`.
- SQLite, as in v1.

## Decision outcome

Chosen option: files plus labels. No database unless a gap shows up that files and `hc ls` cannot cover.

### Consequences

- Good, because there is one source of truth for live state (hc) and one for history (files).
- Bad, because finished terminals can be removed (`hc rm`, `run --rm`); history must be in the files, not in hc.
