---
number: 4
title: "Flock storage: a default location, an optional notes root"
status: accepted
date: 2026-11-26
decision-makers:
- maintainer
---

# Flock storage: a default location, an optional notes root

## Context and problem statement

The coordinator writes internal docs, investigations and plans. Some maintainers keep their notes in a
Markdown repository; others must not be forced to keep a Markdown diary at all.

## Decision outcome

- Each flock has a folder at a default location that needs no setup:
  `${XDG_DATA_HOME:-~/.local/share}/crook/flocks/<yyyy-mm-dd>-<slug>/`. Data, not state or cache, because it
  is user content worth keeping.
- An optional setting, `notes_root`, points the human-facing documents at another folder, for example a
  notes repository.
- The user owns the notes repository and everything that lands in it. crook never runs git there: no
  staging, no commits, no pushes.

### Consequences

- Good, because crook works with no configuration.
- Good, because notes can live beside the maintainer's own notes.
- Bad, because a flock can span two folders; see [0012](0012-split-child-traffic-from-curated-notes.md).
