# Architecture

crook is being redesigned on top of herdr-core (`hc`) and herdr-app. The v1 code (a Rust CLI driving herdr
0.9.5) lives on the [`backup_crook_v1`](https://github.com/s0undt3ch/shepherds-crook/tree/backup_crook_v1)
branch.

Decisions are recorded as [MADR](https://adr.github.io/madr/) files in [`decisions/`](decisions/). Evidence
that decisions cite lives in [`research/`](research/).

## Statuses

- **Accepted**: decided by the maintainer.
- **Proposed**: a design call inferred from evidence, waiting for the maintainer to accept or reject it.
- **Rejected**: turned down by the maintainer; kept as a record.
- **Superseded by NNNN**: replaced; kept as a record.

## Decisions

| ADR | Title | Status |
| --- | --- | --- |
| [0001](decisions/0001-record-architecture-decisions.md) | Record architecture decisions with MADR | Accepted |
| [0002](decisions/0002-build-on-herdr-core-and-herdr-app.md) | Build on herdr-core and herdr-app | Accepted |
| [0003](decisions/0003-coordinator-session-delegates-to-children.md) | A coordinator session delegates to child sessions | Accepted |
| [0004](decisions/0004-effort-storage-location.md) | Effort storage: a default location, an optional notes root | Accepted |
| [0005](decisions/0005-markdown-for-herdr-md.md) | Write plain GitHub Markdown for herdr-md | Accepted |
| [0006](decisions/0006-no-child-restrictions-initially.md) | No access restrictions for children, initially | Accepted |
| [0007](decisions/0007-launch-children-with-hc-run.md) | Launch children with `hc run`, crook's agents and brief files | Accepted |
| [0008](decisions/0008-messages-over-hc-send-results-folder-inbox.md) | Messages over `hc send`; the results folder is the inbox | Accepted |
| [0009](decisions/0009-child-stop-hook-reports-completion.md) | A child Stop hook reports completion | Accepted |
| [0010](decisions/0010-human-input-through-hc-blocked.md) | Human input goes through hc's `blocked` status | Accepted |
| [0011](decisions/0011-shell-hosted-coordinator.md) | Host the coordinator in a shell | Rejected |
| [0012](decisions/0012-split-child-traffic-from-curated-notes.md) | Keep child traffic out of the notes root | Accepted |
| [0013](decisions/0013-directory-access-through-trusted-roots.md) | Directory access through trusted stable roots | Accepted |
| [0014](decisions/0014-state-in-files-and-labels.md) | State in files and hc labels, no database | Accepted |
| [0015](decisions/0015-spaces-from-herdr-labels.md) | Create herdr-app spaces with `herdr.space` labels | Accepted |
| [0016](decisions/0016-hc-preflight.md) | Preflight checks before using hc | Accepted |
| [0017](decisions/0017-resume-children-from-a-roster.md) | Resume children from a roster after a restart | Accepted |
| [0018](decisions/0018-coordinator-runs-claude-directly.md) | The coordinator terminal runs `claude` directly | Accepted |
| [0019](decisions/0019-ship-crook-as-a-binary.md) | Ship crook as a binary, installed by mise, that installs its herdr mod | Accepted |
| [0020](decisions/0020-mechanical-steps-run-through-crook.md) | Mechanical steps run through `crook` subcommands | Proposed |
| [0021](decisions/0021-show-efforts-with-herdr-apps-own-tree.md) | Show an effort with herdr-app's own space, tab and agent tree | Proposed |

## Open questions

- **Names.** "crook" stays. "Effort" is still the working name for one piece of work (a bug, a feature, an
  investigation); "project" and "space" are taken by Claude Code and herdr-app.
- **herdr-app with the window closed.** Do labelled terminals show up when the app opens later? See
  [research](research/2026-11-26-hc-herdr-app-spikes.md).
- **Packaging.** Settled by [0019](decisions/0019-ship-crook-as-a-binary.md). A mise shim hands over to the
  install folder, checked with another tool; the first release confirms it for crook.

## Writing a new ADR

Copy [`decisions/adr-template.md`](decisions/adr-template.md) to the next free number, fill it in, and add a
row above.
