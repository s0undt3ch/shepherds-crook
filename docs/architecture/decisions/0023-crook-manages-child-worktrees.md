---
number: 23
title: "crook creates and removes child worktrees"
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
---

# crook creates and removes child worktrees

## Context and problem statement

A child that changes code needs its own checkout, so two children never edit the same files in one working
tree. [0013](0013-directory-access-through-trusted-roots.md) assumes children run in a repository worktree, but
nothing creates or removes one. A model that runs `git worktree add` itself can pick a different path or branch
each time, and can delete work when it cleans up.

## Considered options

- `crook spawn` creates the worktree; `crook stop` and `crook close` remove it when that is safe.
- The coordinator runs `git worktree` itself, guided by its skill.
- Children share the repository's main working tree.

## Decision outcome

Chosen option: crook creates and removes worktrees.

- `crook spawn <role> <child> <brief> --repo <path>` runs `git worktree add -b crook/<flock>/<child>
  <repo>/.worktrees/<flock>-<child>` from the repository's current `HEAD`, then starts the child there. The
  roster records the repository, the worktree path and the branch.
- Without `--repo`, the child runs in the flock's folder: for research or writing that touches no repository.
- `crook stop <child>` and `crook close <flock>` remove a child's worktree only when it has no uncommitted
  changes and its branch is pushed or merged. Otherwise they leave it, keep the roster entry, and say why.
  The branch itself is never deleted. The one exception: when `spawn` fails to launch the child, it deletes
  the empty branch it has just made (`git branch -d`), so a retry with the same name works (D51 in
  [`docs/plans/build-log.md`](../../plans/build-log.md)).
- The repository must already be trusted in Claude Code. A worktree inside a trusted repository needs no trust
  step of its own (spike 26).

### Consequences

- Good, because every child gets the same layout, and branch names show which flock and child made them.
- Good, because cleanup can never lose uncommitted or unpushed work.
- Bad, because a repository gains a `.worktrees/` folder; it must be in the repository's `.gitignore` or the
  maintainer's global ignore file. `crook doctor` reports a repository where it is not ignored.
- Bad, because worktrees that could not be removed pile up until the maintainer deals with them.

### Confirmation

Spike 26 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): a Claude session in a new
worktree under a trusted repository started with no trust dialog.

## Pros and cons of the options

### The coordinator runs `git worktree` itself

- Good, because there is nothing to build.
- Bad, because paths, branches and cleanup depend on the model getting them right each time
  ([0020](0020-mechanical-steps-run-through-crook.md)).

### Children share the main working tree

- Bad, because two children editing one tree overwrite each other's changes and mix their commits.
