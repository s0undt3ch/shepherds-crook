---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# Keep child traffic out of the notes root

## Context and problem statement

With `notes_root` set ([0004](0004-effort-storage-location.md)), the notes root may be a git repository.
Briefs and results change on every child turn and can hold raw logs and command output.

## Considered options

- Split by reader: curated documents in the notes root, child traffic always in the default location.
- Everything in the notes root.

## Decision outcome

Chosen option: split by reader.

| Where | What |
| --- | --- |
| Notes root (or the default location when unset) | `README.md` (plan, status, what each child does and why, links to PRs and tickets), `notes/` (investigations, decisions, internal docs) |
| Default location, always | `briefs/`, `results/` |

When a result matters, the coordinator writes it up in `notes/`.

### Consequences

- Good, because the notes repository's history stays readable and free of raw logs.
- Bad, because an effort spans two folders when `notes_root` is set; `README.md` must not link into
  `briefs/` or `results/`, since those links break on other machines.
- `notes/` fills only when the maintainer asks: nobody is forced to keep a diary.
