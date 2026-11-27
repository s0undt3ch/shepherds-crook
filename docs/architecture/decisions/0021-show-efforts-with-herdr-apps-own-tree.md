---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# Show an effort with herdr-app's own space, tab and agent tree

## Context and problem statement

The maintainer should see each effort in herdr-app as one unit: the effort, its coordinator on top, then the
coordinator's children, as a tree. A child should get its own tab, at least when the maintainer clicks it.

herdr-app's Agents sidebar already draws a tree. It lists the live terminals whose `parent` is an agent under
that agent, depth first, with tree lines (`mods/agents/main.js:173-183` in herdr-app's mods). Clicking a child
that has no tab opens a peek overlay with a "keep" button (`mods/agents/main.js:202`). `crook spawn` already
starts every child with `--parent` set to the coordinator ([0020](0020-mechanical-steps-run-through-crook.md)).

## Considered options

- herdr-app's own Spaces sidebar and Agents tree; children get a tab only when the maintainer keeps one.
- The same, with a `herdr.tab` label on every child, so each child gets a tab at once.
- A crook mod that replaces or forks the Spaces or Agents sidebar with an effort tree.

## Decision outcome

Chosen option: herdr-app's own sidebars, children without tabs until kept.

- **Effort.** One space, from the coordinator's `herdr.space` label ([0015](0015-spaces-from-herdr-labels.md)).
  `crook open` (and so `crook new`) renames the space to the effort's name with `space.rename` on every start:
  the space goes when its coordinator ends, and comes back named after the coordinator's folder (spike 25).
- **Coordinator.** The space's first tab, from its `herdr.tab` label. crook renames that tab with
  `tab.rename`; herdr-app otherwise names it from the session's title.
- **Children.** No `herdr.space` or `herdr.tab` label. This amends 0015, where children carried labels too.
  Each child shows under the coordinator in the Agents tree. Clicking it peeks; "keep" gives it its own tab in
  the effort's space, named after the child, and hc records the tab labels so the tab stays.
- **Order.** The coordinator is the tree's root; its children follow in the order herdr-app lists them.

crook adds no herdr-app code for this. What the tree cannot show (children in the roster with no live
terminal, each child's role, links to its brief and result) is left to a later decision.

### Consequences

- Good, because the tree, the status roll-up, the sounds and the click-to-tab come from herdr-app and follow
  its updates.
- Good, because the tab bar stays short: only the coordinator and the children the maintainer chose to keep.
- Bad, because a child's tree position follows herdr-app's ordering, which crook does not control.
- Bad, because a paused or not-yet-resumed child does not show: the tree lists live terminals only.
- Bad, because renaming the space and the coordinator's tab needs the app running; with the app closed, the
  space keeps the folder name until the next `crook open` (behaviour with the app closed is untested, 0015).

### Confirmation

Spike 24 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): in the maintainer's real
herdr-app, a coordinator with two children showed as a tree in the Agents sidebar, with and without a tab for
the child. Peek and keep gave the tab-less child its own tab.

Spike 25: `space.rename` and `tab.rename` by label id renamed the space and the coordinator's tab, and both
names held across turns. When the coordinator ended, the space went away; a new terminal with the same labels
came back with the default names. So `crook open` renames on every start, not only `crook new`.

## Pros and cons of the options

### A `herdr.tab` label on every child

- Good, because every child is one click away in the tab bar.
- Bad, because an effort with many children floods the tab bar, while the tree already gives one-click
  access.

### A crook mod that replaces the Spaces or Agents sidebar

- Good, because crook controls the order and can show roster-only children.
- Bad, because herdr-app's sidebars are ordinary mods that herdr updates; a fork drifts from them.
- Bad, because it duplicates a tree herdr-app already draws.

## More information

When this is accepted, add an "amended by 0021" note to 0015.
