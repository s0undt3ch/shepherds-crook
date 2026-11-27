---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Ship crook from one repo: a Claude Code plugin and a herdr mod

## Context and problem statement

crook has two parts that install in different places:

- A Claude Code plugin: the coordinator skill, the role agents
  ([0007](0007-launch-children-with-hc-run.md)) and the hooks ([0009](0009-child-stop-hook-reports-completion.md)).
- A herdr-app mod: the "New effort" dialog and the effort panel.

herdr's docs cover sharing a mod, but not shipping one next to a Claude Code plugin. herdr-app's
[`defaults/AGENTS.md`](https://github.com/herdrdev/herdr-app/blob/HEAD/defaults/AGENTS.md) ("Layouts and mods
other people made") says:

- A shared mod is a folder laid out like `~/.config/herdr-app`: `mods/<name>/`, an optional `keys.json`, a
  `RECIPE.md` and a `preview.png`.
- It is published from a public GitHub repo with the topic `herdr-mod` and listed at herdr.dev/mods.
- It is installed by copying `mods/<name>/` into `~/.config/herdr-app/mods/`: never overwrite, rename on a
  clash, merge `keys.json`, run `herdr-app check`.
- Before installing, the agent must tell the person if the mod runs programs (`exec`, `hc`).

There is no installer, no versioning and no update path for mods.

## Considered options

- One repo that is both a Claude Code plugin and a herdr mod source; the plugin offers a command that copies
  the mod in.
- Two repos, one per part.
- Only the plugin; no mod.

## Decision outcome

Chosen option: one repo, both shapes.

- The repo root is a Claude Code plugin, installed from a marketplace.
- `mods/crook/` holds the mod, with a `RECIPE.md` and a `preview.png`. The repo carries the `herdr-mod` topic,
  so the mod is listed at herdr.dev/mods.
- A plugin command installs the mod the way herdr's docs ask: it states that the mod runs `hc`, never
  overwrites, merges `keys.json`, and runs `herdr-app check`.
- The mod stays thin: UI only. Effort logic lives in the plugin, so a plugin upgrade changes behaviour
  without a mod reinstall.
- The mod checks a version marker it shares with the plugin and shows "reinstall the crook mod" when they
  differ.

### Consequences

- Good, because one version number and one changelog cover both parts.
- Good, because users who find crook at herdr.dev/mods and users who find the plugin get the same thing.
- Bad, because the mod is a copy: it does not follow plugin upgrades. The version check only tells the
  person; it does not fix it.
- Bad, because every install shows herdr's "runs programs" warning.

### Confirmation

Not tested:

- how the mod finds the plugin's files: Claude Code's plugin install path may change with each version;
- whether a repo that is both a plugin and a mod source passes both installers.

A spike should answer both before this is accepted.

## Pros and cons of the options

### Two repos

- Good, because each repo has one shape and one installer.
- Bad, because the two parts must be versioned and released in step by hand.

### Plugin only

- Bad, because the "New effort" dialog and the effort panel ([0003](0003-coordinator-session-delegates-to-children.md))
  need herdr-app.
