---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Ship crook as a binary, installed by mise, that installs its herdr mod

## Context and problem statement

crook has two parts:

- A Claude Code plugin: the coordinator skill, the role agents
  ([0007](0007-launch-children-with-hc-run.md)) and the hooks ([0009](0009-child-stop-hook-reports-completion.md)).
- A herdr-app mod: the "New effort" dialog and the effort panel.

v1 never installed its plugin globally: every crook session got `--plugin-dir <crook's folder>`
(`CONTRIBUTING.md` on `backup_crook_v1`). So the plugin needs a folder on disk, not a marketplace install.

herdr installs shared mods by copying a folder into `~/.config/herdr-app/mods/`, never overwriting and
renaming on a clash ([`defaults/AGENTS.md`](https://github.com/herdrdev/herdr-app/blob/HEAD/defaults/AGENTS.md),
"Layouts and mods other people made"). A mod cannot learn its own folder name (spike 17).

The maintainer installs tools with mise. crook will be installed globally with mise's `packslip:` backend,
not per project. mise unpacks the whole release archive into one folder per version and removes the old
version 24 hours after an upgrade (spike 18).

## Considered options

- A `crook` binary released with packslip, its archive carrying the plugin and the mod; `crook setup` installs
  the mod.
- One herdr mod folder that carries the plugin, installed by copying.
- A marketplace plugin plus a separately copied mod.

## Decision outcome

Chosen option: a `crook` binary released with packslip.

The release:

- Each release archive holds the `crook` executable, `plugin/` and `mod/`. The packslip manifest declares
  `crook` as its executable.
- The maintainer installs it with `mise use -g packslip:github.com/<owner>/shepherds-crook`. Per-project
  installs are not supported.
- mise owns the version folders: installing, upgrading and pruning them. crook unpacks nothing itself.

Finding its files:

- The binary finds `plugin/` and `mod/` next to its own executable, in its mise install folder.
- On every run it points `~/.local/share/crook/current` at its own `plugin/`.
- Every crook session starts with `--plugin-dir ~/.local/share/crook/current/plugin`, never the versioned
  folder. When mise prunes the old version, the next crook run has already moved the link. A session started
  on a versioned path that is later removed gets `Stop hook failed` (spike 18).
- The terminal gets the label `crook.version=<version>`.

`crook setup`:

- installs `mod/` into `<appHome>/mods/crook/` with a `.crook-version` marker;
- overwrites that folder only when the marker is there; a `mods/crook/` without the marker belongs to someone
  else, so it stops and says so;
- moves `current`.

The mod stays thin: UI only. It runs `exec("crook", [...])` to start or resume an effort, so it never needs a
path. herdr-app's `exec` uses the login shell's PATH, which on the maintainer's machine includes mise's global
tools (spike 18).

`crook doctor` checks, and `crook doctor --fix` repairs what it can:

- hc's version and schema ([0016](0016-hc-preflight.md));
- herdr-app is installed;
- `crook` is on the login shell's PATH;
- the mod is installed and its marker matches the binary's version;
- `current` points at this binary's `plugin/`.

Children on another machine (`hc run --on`) need crook there too: install it with mise and run `crook setup`
on that machine. crook does not copy itself across machines.

### Consequences

- Good, because one signed release carries the binary, the plugin and the mod: they cannot drift apart.
- Good, because mise handles download, verification, versions and cleanup.
- Good, because crook owns `mods/crook/` and never needs herdr's rename on a clash.
- Good, because the mod needs no paths.
- Bad, because a live session reads its agent text at start but its hooks through `current`, so after an
  upgrade it runs an old role text with new hooks (spike 18). Hooks must stay compatible with the previous
  version's agents and skills.
- Bad, because installing from herdr.dev/mods by copying the folder is no longer enough; the mod's
  `RECIPE.md` points to mise instead.
- Bad, because the binary must be built and released per platform.

### Confirmation

Spikes 17 and 18 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).

Not tested:

- `exec("crook", …)` from a mod in the real herdr-app;
- the binary finding its own install folder when started through a mise shim.

## Pros and cons of the options

### One mod folder that carries the plugin

- Bad, because a herdr rename on a clash breaks the plugin path unless the installing agent rewrites it.
- Bad, because there is no version marker or update path.

### Marketplace plugin plus a copied mod

- Bad, because the copied mod does not follow plugin upgrades.
- Bad, because crook's skills and hooks load in every Claude session, not just crook's.
