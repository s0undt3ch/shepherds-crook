---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Ship crook as one herdr mod that carries its Claude Code plugin

## Context and problem statement

crook has two parts:

- A Claude Code plugin: the coordinator skill, the role agents
  ([0007](0007-launch-children-with-hc-run.md)) and the hooks ([0009](0009-child-stop-hook-reports-completion.md)).
- A herdr-app mod: the "New effort" dialog and the effort panel.

v1 never installed its plugin globally. Every crook session was started with `--plugin-dir <crook's folder>`
(`CONTRIBUTING.md` on `backup_crook_v1`), and 0007 already starts children the same way. So the plugin only
needs a stable folder on disk, not a marketplace install.

herdr's docs cover sharing a mod, but not shipping one next to a Claude Code plugin. herdr-app's
[`defaults/AGENTS.md`](https://github.com/herdrdev/herdr-app/blob/HEAD/defaults/AGENTS.md) ("Layouts and mods
other people made") says:

- A shared mod is a folder laid out like `~/.config/herdr-app`: `mods/<name>/`, an optional `keys.json`, a
  `RECIPE.md` and a `preview.png`.
- It is published from a public GitHub repo with the topic `herdr-mod` and listed at herdr.dev/mods.
- It is installed by copying `mods/<name>/` into `~/.config/herdr-app/mods/`: never overwrite, rename on a
  clash, merge `keys.json`, run `herdr-app check`.
- Before installing, the agent must tell the person if the mod runs programs (`exec`, `hc`).
- A mod can call `appHome()` and `exec(...)`.

## Considered options

- One herdr mod folder that carries the plugin inside it; every crook session gets `--plugin-dir` pointing
  there.
- A marketplace-installed plugin plus a separately copied mod, from one repo.
- Two repos, one per part.

## Decision outcome

Chosen option: the mod carries the plugin.

- The repo holds `mods/crook/`, with a `RECIPE.md` and a `preview.png`, and carries the `herdr-mod` topic, so
  it is listed at herdr.dev/mods.
- The plugin lives inside it, at `mods/crook/plugin/`. Nothing is installed in Claude Code's own config.
- Installing crook is installing the mod: one copy, the herdr way.
- The mod starts the coordinator with `--plugin-dir <appHome()>/mods/crook/plugin` and hands the same path
  to it (for example as `CROOK_PLUGIN_DIR`), so the coordinator passes it on to every child and every resume
  ([0017](0017-resume-children-from-a-roster.md)).
- Plain Claude sessions outside crook never see crook's skills, agents or hooks.

### Consequences

- Good, because there is one copy and one version: the plugin and the mod cannot drift apart.
- Good, because crook leaves no trace in Claude Code's global setup.
- Good, because an upgrade is a re-copy of one folder.
- Bad, because the mod has no update path: herdr copies are never updated by themselves.
- Bad, because a herdr rename on a name clash changes the folder, so the mod must build the path from its own
  folder name, not a fixed `crook`.
- Bad, because every install shows herdr's "runs programs" warning.

### Confirmation

Not tested:

- whether `herdr-app check` accepts a mod folder that also holds a `plugin/` folder;
- what `appHome()` returns, and whether a mod can learn its own folder name;
- children on another machine (`hc run --on`): the plugin folder is only on this one.

A spike should answer the first two before this is accepted.

## Pros and cons of the options

### Marketplace plugin plus a copied mod

- Bad, because the copied mod does not follow plugin upgrades, so the two can drift.
- Bad, because crook's skills and hooks load in every Claude session, not just crook's.

### Two repos

- Bad, because the two parts must be versioned and released in step by hand.
