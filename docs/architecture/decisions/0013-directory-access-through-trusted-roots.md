---
number: 13
title: "Directory access through trusted stable roots"
status: accepted
date: 2026-11-26
decision-makers:
- maintainer
---

# Directory access through trusted stable roots

## Context and problem statement

Children run in a repository worktree but must reach the flock folders. A plain `--add-dir` prompts in
every new session, which is unusable.

## Considered options

- Trust two stable roots once (the flocks root and the notes root) and pass them with `--add-dir`.
- `permissions.additionalDirectories` in settings.
- `--add-dir` per flock folder.

## Decision outcome

Chosen option: trust the stable roots once, by starting Claude in each and accepting. After that,
`--add-dir <root>` and `--add-dir <root>/<new flock>` start with no prompt. crook's setup step does this
trust once.

### Consequences

- Good, because new flocks never prompt.
- Bad, because the first trust is a manual step.

### Confirmation

Spike 10 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).

## Pros and cons of the options

### `permissions.additionalDirectories`

- Good, because it never prompted, even for a folder never trusted.
- Bad, because it is unclear whether that folder is actually accessible; not checked. If it is, this option
  is simpler, and this ADR should switch to it.

### `--add-dir` per flock folder

- Bad, because every flock is a new path, so it prompts every time.
