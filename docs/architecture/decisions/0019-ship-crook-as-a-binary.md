---
number: 19
title: "Ship crook as a binary, installed by mise, that installs its herdr mod"
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
---

# Ship crook as a binary, installed by mise, that installs its herdr mod

## Context and problem statement

crook has two parts:

- A Claude Code plugin: the coordinator skill, the role agents
  ([0007](0007-launch-children-with-hc-run.md)) and the hooks ([0009](0009-child-stop-hook-reports-completion.md)).
- A herdr-app mod: the "New flock" dialog. A panel for flocks is deferred ([0021](0021-show-flocks-with-herdr-apps-own-tree.md)).

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
- A debug build (`cargo build`, `cargo run`) has nothing next to it in `target/`. When `plugin/` is not next
  to the executable, a debug build uses the repository's own `plugin/` and `mod/`, whose path is fixed at
  compile time. Skill edits then show up without a rebuild. Release builds never fall back; they stop and say
  the install is broken.
- On every run it points `${XDG_DATA_HOME:-~/.local/share}/crook/current` at its own install folder, but only
  forward: a binary older than the version `current` points at leaves it alone. A debug build never moves it.
- Everything crook starts calls the binary through mise's shim, in mise's data folder (`$MISE_DATA_DIR`, else
  `$XDG_DATA_HOME/mise`, else `~/.local/share/mise`) under `shims/crook`, never a
  versioned path and never a bare `crook` looked up on the `PATH`: the plugin's hook commands and the mod's
  `exec`. The shim always runs the version mise has active now. A bare `crook` in a terminal of a running hc
  core would find the old version's folder ahead of the shims for up to 24 hours after an upgrade (spike 22).
  The maintainer's dotfiles already run mempalace and qdrant services through their shims for the same reason.
- `CROOK_BIN` overrides the shim, for development. The hook commands read
  `"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}"`,
  and the mod does the same. A debug build is started with `CROOK_BIN` set to itself. A terminal gets the environment
  of the call that started its hc core, not
  of the caller of `hc run`, and `hc run` has no option to set one variable, so `crook spawn` and
  `crook resume` pass it on as `env CROOK_BIN=<path> claude …` when it is set (spike 23). `mise link` does not
  cover this: it registers a local build, but mise makes no shim for a linked packslip version (spike 23).
- Every crook session starts with `--plugin-dir <data home>/crook/current/plugin`, never the versioned
  folder. When mise prunes the old version, the next crook run has already moved the link. A session started
  on a versioned path that is later removed gets `Stop hook failed` (spike 18).
- The terminal gets the label `crook.version=<version>`.

Upgrades in live sessions:

- A session's role text (`--agent`) is fixed for its whole life, resumes included. `/reload-plugins` brings in
  new skills and hooks (spike 19).
- So each role's agent file stays thin and stable: who the session is, and "use the `crook-<role>` skill".
  Behaviour lives in skills and hooks.
- A crook hook in every session records the plugin version at SessionStart. Its Stop hook reads the version
  again through `current`; when it changed, it runs `hc send "$HC_TERMINAL_ID" "/reload-plugins"`, and the
  reload runs as the turn ends (spike 20). The coordinator does not have to tell its children: each session
  reloads itself. The turn running during the upgrade finishes on the old skills.
- A version never removes or renames a skill that the previous version's agent files name.

`crook setup`:

- installs `mod/` into `<appHome>/mods/crook/` with a `.crook-version` marker;
- overwrites that folder only when the marker is there; a `mods/crook/` without the marker belongs to someone
  else, so it stops and says so;
- moves `current`.

The mod stays thin: UI only. It runs `exec` on the shim to start or resume a flock. herdr-app's `exec`
also finds a bare `crook` through the login shell's PATH when the login shell activates mise (spikes 18 and
21), but the shim does not depend on that.

`crook doctor` checks, and `crook doctor --fix` repairs what it can:

- hc's version and schema ([0016](0016-hc-preflight.md));
- herdr-app is installed;
- `crook` is on the login shell's PATH;
- the mod is installed and its marker matches the binary's version;
- `current` points at this binary's install folder.

Children on another machine (`hc run --on`) need crook there too: install it with mise and run `crook setup`
on that machine. crook does not copy itself across machines.

### Consequences

- Good, because one signed release carries the binary, the plugin and the mod: they cannot drift apart.
- Good, because mise handles download, verification, versions and cleanup.
- Good, because crook owns `mods/crook/` and never needs herdr's rename on a clash.
- Good, because the mod needs no paths.
- Bad, because a live session keeps its old role text after an upgrade, next to new skills and hooks
  (spikes 18 and 19). Agent files must stay thin, and skills must stay compatible with the previous
  version's agent files.
- Bad, because installing from herdr.dev/mods by copying the folder is no longer enough; the mod's
  `RECIPE.md` points to mise instead.
- Bad, because the binary must be built and released per platform.

### Confirmation

Spikes 17 to 21 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).

Started through a mise shim, a tool runs from its install folder: `python3` started as
`~/.local/share/mise/shims/python3` showed `~/.local/share/mise/installs/python/3.15.2/bin/python3` in `ps`
and its executable in `lsof` (2026-11-27). The shim hands over with `exec`, so crook's own executable path is
its install folder too. Not yet seen with crook itself; the first release confirms it.

## Pros and cons of the options

### One mod folder that carries the plugin

- Bad, because a herdr rename on a clash breaks the plugin path unless the installing agent rewrites it.
- Bad, because there is no version marker or update path.

### Marketplace plugin plus a copied mod

- Bad, because the copied mod does not follow plugin upgrades.
- Bad, because crook's skills and hooks load in every Claude session, not just crook's.
