# Implementation plans

One plan per area. Each task in a plan cites the ADRs it implements
([`docs/architecture`](../architecture/README.md)).

Questions the plans raise, and the maintainer's answers, are in [open-questions.md](open-questions.md).

| Plan | Area |
| --- | --- |
| [binary](2026-11-27-crook-binary.md) | The `crook` Rust binary |
| [plugin](2026-11-27-crook-plugin.md) | The Claude Code plugin: agents, skills, hooks |
| [mod](2026-11-27-crook-mod.md) | The herdr-app mod |
| [release-ci-e2e](2026-11-27-crook-release-ci-e2e.md) | Release, CI and the e2e harness |

## ADR coverage

Which plans cite each ADR. A tick means the plan cites the ADR, not that it implements all of it.

| ADR | binary | plugin | mod | release, CI, e2e |
| --- | --- | --- | --- | --- |
| [0001](../architecture/decisions/0001-record-architecture-decisions.md) Record architecture decisions with MADR | ✓ | ✓ |  | ✓ |
| [0002](../architecture/decisions/0002-build-on-herdr-core-and-herdr-app.md) Build on herdr-core and herdr-app | ✓ |  |  | ✓ |
| [0003](../architecture/decisions/0003-coordinator-session-delegates-to-children.md) A coordinator session delegates to child sessions |  | ✓ | ✓ |  |
| [0004](../architecture/decisions/0004-flock-storage-location.md) Flock storage: a default location, an optional notes root | ✓ | ✓ | ✓ | ✓ |
| [0005](../architecture/decisions/0005-markdown-for-herdr-md.md) Write plain GitHub Markdown for herdr-md |  | ✓ |  | ✓ |
| [0006](../architecture/decisions/0006-no-child-restrictions-initially.md) No access restrictions for children, initially | ✓ | ✓ |  |  |
| [0007](../architecture/decisions/0007-launch-children-with-hc-run.md) Launch children with `hc run`, crook's agents and brief files | ✓ | ✓ | ✓ | ✓ |
| [0008](../architecture/decisions/0008-messages-over-hc-send-results-folder-inbox.md) Messages over `hc send`; the results folder is the inbox | ✓ | ✓ |  | ✓ |
| [0009](../architecture/decisions/0009-child-stop-hook-reports-completion.md) A child Stop hook reports completion | ✓ | ✓ |  | ✓ |
| [0010](../architecture/decisions/0010-human-input-through-hc-blocked.md) Human input goes through hc's `blocked` status |  | ✓ |  | ✓ |
| [0011](../architecture/decisions/0011-shell-hosted-coordinator.md) Host the coordinator in a shell (Rejected) | ✓ | ✓ |  |  |
| [0012](../architecture/decisions/0012-split-child-traffic-from-curated-notes.md) Keep child traffic out of the notes root | ✓ | ✓ |  |  |
| [0013](../architecture/decisions/0013-directory-access-through-trusted-roots.md) Directory access through trusted stable roots | ✓ | ✓ |  | ✓ |
| [0014](../architecture/decisions/0014-state-in-files-and-labels.md) State in files and hc labels, no database | ✓ |  |  | ✓ |
| [0015](../architecture/decisions/0015-spaces-from-herdr-labels.md) Create herdr-app spaces with `herdr.space` labels | ✓ |  | ✓ | ✓ |
| [0016](../architecture/decisions/0016-hc-preflight.md) Preflight checks before using hc | ✓ |  |  | ✓ |
| [0017](../architecture/decisions/0017-resume-children-from-a-roster.md) Resume children from a roster after a restart | ✓ | ✓ |  | ✓ |
| [0018](../architecture/decisions/0018-coordinator-runs-claude-directly.md) The coordinator terminal runs `claude` directly | ✓ | ✓ | ✓ | ✓ |
| [0019](../architecture/decisions/0019-ship-crook-as-a-binary.md) Ship crook as a binary, installed by mise, that installs its herdr mod | ✓ | ✓ | ✓ | ✓ |
| [0020](../architecture/decisions/0020-mechanical-steps-run-through-crook.md) Mechanical steps run through `crook` subcommands | ✓ | ✓ | ✓ | ✓ |
| [0021](../architecture/decisions/0021-show-flocks-with-herdr-apps-own-tree.md) Show a flock with herdr-app's own space, tab and agent tree | ✓ | ✓ | ✓ | ✓ |
| [0022](../architecture/decisions/0022-rust-fresh-code-v1-tooling.md) Rust, fresh code on main, v1's tooling reused | ✓ | ✓ | ✓ | ✓ |
| [0023](../architecture/decisions/0023-crook-manages-child-worktrees.md) crook creates and removes child worktrees | ✓ | ✓ | ✓ | ✓ |
| [0024](../architecture/decisions/0024-flock-file-formats.md) File formats for the roster, briefs, results and delivery marks | ✓ | ✓ | ✓ | ✓ |
| [0025](../architecture/decisions/0025-roles-and-child-models.md) Two roles; the coordinator picks each child's model | ✓ | ✓ |  | ✓ |
| [0026](../architecture/decisions/0026-config-file.md) crook's settings live in an XDG config file | ✓ | ✓ | ✓ | ✓ |

Every accepted ADR is cited by at least one plan. 0011 is Rejected; the plans cite it only as history.
