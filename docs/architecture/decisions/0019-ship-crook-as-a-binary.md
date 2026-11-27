---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Ship crook as a binary that installs its plugin and herdr mod

## Context and problem statement

crook has two parts:

- A Claude Code plugin: the coordinator skill, the role agents
  ([0007](0007-launch-children-with-hc-run.md)) and the hooks ([0009](0009-child-stop-hook-reports-completion.md)).
- A herdr-app mod: the "New effort" dialog and the effort panel.

v1 never installed its plugin globally: every crook session got `--plugin-dir <crook's folder>`
(`CONTRIBUTING.md` on `backup_crook_v1`). So the plugin needs a stable folder on disk, not a marketplace
install.

herdr installs shared mods by copying a folder into `~/.config/herdr-app/mods/`, never overwriting and
renaming on a clash ([`defaults/AGENTS.md`](https://github.com/herdrdev/herdr-app/blob/HEAD/defaults/AGENTS.md),
"Layouts and mods other people made"). There is no versioning or update path. A mod cannot learn its own
folder name at run time, so a renamed copy cannot find files next to it (spike 17).

## Considered options

- A `crook` binary that carries the plugin and the mod, unpacks them per version, and installs the mod.
- One herdr mod folder that carries the plugin, installed by copying.
- A marketplace plugin plus a separately copied mod.

## Decision outcome

Chosen option: a `crook` binary.

Layout on disk:

- `~/.local/share/crook/<version>/plugin/` holds the plugin. The binary unpacks it on first run of that
  version.
- `~/.local/share/crook/current` is a link to the newest unpacked version.
- Every crook session starts with `--plugin-dir ~/.local/share/crook/current/plugin`, and its terminal gets the
  label `crook.version=<version>`.

`crook setup`:

- installs the mod into `<appHome>/mods/crook/` with a `.crook-version` marker;
- overwrites that folder only when the marker is there; a `mods/crook/` without the marker belongs to someone
  else, so it stops and says so;
- points `current` at its own version;
- removes an older version's folder only when no live terminal carries its `crook.version` label (`hc ls`).

The mod stays thin: UI only. It runs `exec("crook", [...])` to start or resume an effort, so it never needs a
plugin path.

`crook doctor` checks, and `crook doctor --fix` repairs what it can:

- hc's version and schema ([0016](0016-hc-preflight.md));
- herdr-app is installed;
- the mod is installed and its marker matches the binary's version;
- `current` points at the binary's version;
- old versions no live terminal uses, which can go.

Children on another machine (`hc run --on`) need crook there too: install the binary on that machine and run
`crook setup`. crook does not copy itself across machines.

### Consequences

- Good, because the plugin and the mod come from one artefact and cannot drift apart.
- Good, because crook owns `mods/crook/` and never needs herdr's rename on a clash.
- Good, because live sessions keep their plugin files: old versions go only when nothing uses them.
- Good, because the mod needs no paths, so a renamed or moved mod still works.
- Bad, because installing from herdr.dev/mods by copying the folder is no longer enough; the mod's `RECIPE.md`
  points to the binary instead.
- Bad, because a binary has to be built and released per platform.
- Bad, because a live session that started on an old version follows `current` on its next resume. Old and new
  plugins must stay compatible across a resume.

### Confirmation

From spike 17 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): `herdr-app check` passes
with extra folders in a mod, `appHome()` is `~/.config/herdr-app` or `$HERDR_APP_HOME`, and Claude loads a
role from `--plugin-dir`.

Not tested, for a spike before this is accepted:

- a mod's `exec("crook", ...)` finds `crook` on PATH when herdr-app is started from the Finder;
- `--plugin-dir` through a symlinked `current`;
- removing a version folder while no terminal uses it, then resuming a session that started on it.

## Pros and cons of the options

### One mod folder that carries the plugin

- Bad, because a herdr rename on a clash breaks the plugin path unless the installing agent rewrites it.
- Bad, because there is no version marker or update path.

### Marketplace plugin plus a copied mod

- Bad, because the copied mod does not follow plugin upgrades.
- Bad, because crook's skills and hooks load in every Claude session, not just crook's.
