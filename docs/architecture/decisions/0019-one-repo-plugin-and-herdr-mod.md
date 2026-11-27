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
- The mod writes its plugin path as a literal, `appHome() + "/mods/crook/plugin"`. A mod cannot read its own
  folder name at run time (spike 17), but herdr's install rules rewrite a renamed mod's own `mods/<name>/`
  paths, so the literal follows a rename.
- Children on another machine (`hc run --on`) need crook there too: that machine must have the crook mod
  installed, so the same plugin path exists on it. crook does not copy the plugin across machines.

### Consequences

- Good, because there is one copy and one version: the plugin and the mod cannot drift apart.
- Good, because crook leaves no trace in Claude Code's global setup.
- Good, because an upgrade is a re-copy of one folder.
- Bad, because the mod has no update path: herdr copies are never updated by themselves.
- Bad, because a rename on a name clash relies on the installing agent rewriting the mod's own paths.
- Bad, because every machine that runs crook children needs the mod installed.
- Bad, because every install shows herdr's "runs programs" warning.

### Confirmation

Spike 17 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): `herdr-app check` passes with
`plugin/` inside the mod, `appHome()` is `~/.config/herdr-app` or `$HERDR_APP_HOME`, and Claude loads the
role from `--plugin-dir <mod>/plugin`.

Not tested: the mod live in the real app; the rename rewrite; children on another machine.

## Pros and cons of the options

### Marketplace plugin plus a copied mod

- Bad, because the copied mod does not follow plugin upgrades, so the two can drift.
- Bad, because crook's skills and hooks load in every Claude session, not just crook's.

### Two repos

- Bad, because the two parts must be versioned and released in step by hand.
