---
number: 29
title: A Flocks panel, and each flock's task, links and repositories
status: proposed
date: 2026-11-28
decision-makers:
- maintainer
links:
- target: 21
  kind: amends
- target: 24
  kind: amends
- target: 20
  kind: amends
---

# A Flocks panel, and each flock's task, links and repositories

## Context and problem statement

A flock today is a slug, a folder and a roster. Its task exists only in the coordinator's first prompt
(`src/launch.rs:250`), so nobody can see it or change it later. The flock has no record of the tickets, documents
and pull requests behind the work, or of the repositories the work touches. The coordinator has to be told these
things again in conversation. The only crook surface in herdr-app is the Flocks dialog. The Spaces sidebar shows
only live coordinators, so a paused flock vanishes from it.

The maintainer tried the Flocks dialog and then a throwaway sidebar panel in a sandbox herdr-app window (spike 29).
They asked for a Flocks panel in the sidebar, and for each flock to carry a task, links and repositories that they
can edit and that the coordinator acts on.

[0021](0021-show-flocks-with-herdr-apps-own-tree.md) left "what the tree cannot show" to a later decision. This is
that decision. The mod plan's non-goals ("no flock panel") follow it.

## Decision drivers

- The maintainer sees every flock, paused ones included, in one place, and reaches any flock in one click.
- The coordinator knows the flock's task, links and repositories at every session start, and learns about a change
  while it runs.
- A repository named in conversation leads to a child working in that repository without the maintainer giving a
  path.
- Only crook writes `roster.json` ([0020](0020-mechanical-steps-run-through-crook.md)). The mod runs crook
  commands and nothing else.
- herdr-app's own sidebars stay unforked (0021).

## Considered options

- A Flocks panel in the sidebar of a layout that crook installs, with properties stored in the roster and edited
  through a new `crook edit`.
- The same properties, edited in the existing Flocks dialog, with no panel.
- A panel docked into herdr-app's stock `main` layout through `mod.json`'s `place`.

## Decision outcome

Chosen option: a panel in a layout crook installs, with properties in the roster, because it is the only option
that shows paused flocks beside Spaces and Agents and works with Herdr's read-only `main` layout.

### The panel

- A second mod, `crook-flocks`, next to the dialog mod `crook`. It lists every flock from `crook status --json`:
  slug, date, state and child count, active flocks first. A "+" opens the New flock dialog.
- **Click** a flock to switch to its space. A flock with no live coordinator is opened first, with `crook open`.
- **Right-click** a flock for a menu: Properties…, and Close flock….
- A mod with `"place": "left"` did not dock into the stock `main` layout, live or after a restart (spike 29).
  `crook setup` therefore builds a layout `crook`: a clone of the installed `main` (`layout.clone`) with the panel's
  block added above Spaces, and a `layout:crook` key that opens the dialog. Setup rebuilds it on every run, so it
  follows Herdr's `main` across upgrades. The maintainer switches to it once. `main` itself is never changed.

### Properties

- **Task**: free text. `crook new --task` now stores it as well as passing it in the first prompt.
- **Links**: a label and a URL each. Clicking one in the panel opens it in the browser (`open` on macOS,
  `xdg-open` on Linux, through `exec`). crook does not copy links into the README; the coordinator decides that.
- **Repositories**: local folders, each the top of a git working tree. The dialog adds one with `pickFolder`.
  crook refuses a folder that is not a git working tree. Trust stays as
  [0013](0013-directory-access-through-trusted-roots.md) has it: a repository the maintainer has not trusted still
  asks once, in the child.
- The Properties dialog edits all three. The New flock dialog gets collapsed "Links" and "Repositories" sections
  under the task box, and grows taller to hold them.

### Storage and the command

- `roster.json` gains `task` (string or null), `links` (a list of `{label, url}`) and `repos` (a list of absolute
  paths). This amends [0024](0024-flock-file-formats.md): the roster's `version` becomes 2, and crook reads a
  version 1 roster as having no task, links or repositories.
- A new command writes them: `crook edit <slug> [--task <text>] [--add-link <label> <url>] [--remove-link <url>]
  [--add-repo <path>] [--remove-repo <path>]`. `crook new` takes `--link` and `--repo`, repeatable. `crook status
  --json` reports all three. This amends 0020's command list.

### The coordinator

- The SessionStart hook adds the flock's current task, links and repositories to the coordinator's context at every
  session start: a first start, a resume, `/clear` and compaction.
- When `crook edit` changes anything while the coordinator is live, crook sends it one line: the flock's details
  changed, and it should read them with `crook status <slug> --json`. A blocked coordinator gets no line; it reads
  the change at its next session start. The skill treats this line as information, never as the maintainer's
  consent (build log D35).
- When the maintainer asks for work in a repository the flock lists, the coordinator spawns the child with
  `--repo` set to that repository, without asking for the path.
- When the maintainer names a repository the flock does not list, and the coordinator finds it on disk, it asks
  the maintainer whether to add it. Only on a yes typed in its own pane does it run `crook edit --add-repo`.

### Consequences

- Good, because paused flocks stay visible, and every flock is one click away.
- Good, because the task, links and repositories survive `/clear`, compaction and resume, as roster data and
  through the hook.
- Good, because a repository becomes a name in conversation instead of a path the maintainer types.
- Bad, because the panel needs a layout crook owns. Between Herdr's upgrade and the next `crook setup`, the `crook`
  layout can lag behind `main`.
- Bad, because it adds a second mod, a command and a roster version for crook to maintain.
- Bad, because a nudge typed into the coordinator's pane lands in the middle of whatever the maintainer is typing
  there. Not checked yet.
- Bad, because the coordinator needs read access outside its folders to find a repository on disk. Under `default`
  permission mode each search may ask the maintainer.

### Confirmation

- Spike 29: a mod with `place` did not dock into the stock `main`. A throwaway panel with a Properties overlay
  rendered in a `main` clone, and `herdr-app check` passed it.
- Unit tests: the roster's version 2 fields, reading version 1, `crook edit` refusing a non-repository and an
  unknown flock, and the hook's context listing the three properties.
- Tier 2 e2e: `crook setup` builds the `crook` layout with the panel. A click switches to the flock, a right-click
  opens Properties, and an edit reaches `crook status --json`.
- Tier 3 e2e: an edit while the coordinator is live delivers the nudge. A coordinator asked to work in a listed
  repository spawns a child with that `--repo`.
- To check before building: what the nudge does while the maintainer is typing in the coordinator's pane.

## Pros and cons of the options

### Properties in the Flocks dialog, no panel

- Good, because it needs no layout and no second mod.
- Bad, because paused flocks stay invisible until the maintainer opens the dialog.
- Bad, because reaching a flock takes a key, a choice and Enter instead of one click.

### A panel docked into the stock `main` layout

- Good, because the maintainer would need no other layout.
- Bad, because it did not work: Herdr never docked the mod into `main` (spike 29). `main` is read-only and replaced
  on every start.

## More information

- [Spike 29](../research/2026-11-26-hc-herdr-app-spikes.md): the mod's dependencies in the app, the docking test and
  the throwaway panel.
- The mod plan's non-goals (`docs/plans/2026-11-27-crook-mod.md`) change only once this ADR is accepted.
