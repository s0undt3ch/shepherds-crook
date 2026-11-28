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

One row per ADR. Each cell lists the tasks in that plan that implement or test the ADR, taken from the ADRs line of
each task. Binary ids are tasks (`5.1`), plugin ids start with `P`, mod ids start with `M` and are tasks in the mod
plan, and release ids are milestone tasks (`M4.1`) or e2e scenarios (`S3`) in the release plan. "A to B" is a run of
tasks. A dash means the plan has no task for that ADR. "Process" and "context" mark an ADR with nothing to build.

| ADR | binary | plugin | mod | release |
| --- | --- | --- | --- | --- |
| [0001](../architecture/decisions/0001-record-architecture-decisions.md) Record architecture decisions with MADR | process | – | – | – |
| [0002](../architecture/decisions/0002-build-on-herdr-core-and-herdr-app.md) Build on herdr-core and herdr-app | context | – | – | – |
| [0003](../architecture/decisions/0003-coordinator-session-delegates-to-children.md) A coordinator session delegates to child sessions | – | P3.1a, P3.1b | M3.1, M6.1 | – |
| [0004](../architecture/decisions/0004-flock-storage-location.md) Flock storage: a default location, an optional notes root | 1.1, 1.2, 2.4, 5.4 | P3.1a, P3.1c | M3.1 | S2 |
| [0005](../architecture/decisions/0005-markdown-for-herdr-md.md) Write plain GitHub Markdown for herdr-md | – | P3.1c | – | M2.5 |
| [0006](../architecture/decisions/0006-no-child-restrictions-initially.md) No access restrictions for children, initially | context | – | – | – |
| [0007](../architecture/decisions/0007-launch-children-with-hc-run.md) Launch children with `hc run`, crook's agents and brief files | 4.2, 5.1, 6.1 | P1.2, P3.1a, P3.2 | – | M2.1, S3 |
| [0008](../architecture/decisions/0008-messages-over-hc-send-results-folder-inbox.md) Messages over `hc send`; the results folder is the inbox | 2.3, 4.2, 8.3 to 8.5 | P1.3, P3.1b, P6.1 | – | S4, S5 |
| [0009](../architecture/decisions/0009-child-stop-hook-reports-completion.md) A child Stop hook reports completion | 4.2, 8.4 | P1.3, P2.1, P3.1b, P6.1 | – | S4 |
| [0010](../architecture/decisions/0010-human-input-through-hc-blocked.md) Human input goes through hc's `blocked` status | – | P2.1, P3.1b, P6.1 | – | S5, S11 |
| [0011](../architecture/decisions/0011-shell-hosted-coordinator.md) Host the coordinator in a shell (Rejected) | Rejected | – | – | – |
| [0012](../architecture/decisions/0012-split-child-traffic-from-curated-notes.md) Keep child traffic out of the notes root | 2.4, 5.4 | P3.1a, P3.1c | – | – |
| [0013](../architecture/decisions/0013-directory-access-through-trusted-roots.md) Directory access through trusted stable roots | 5.1, 9.1 | P3.1a | – | M5.5, S3 |
| [0014](../architecture/decisions/0014-state-in-files-and-labels.md) State in files and hc labels, no database | 4.2, 7.1 | – | – | M5.6 |
| [0015](../architecture/decisions/0015-spaces-from-herdr-labels.md) Create herdr-app spaces with `herdr.space` labels | 1.3, 5.3 | – | M3.2 | S2 |
| [0016](../architecture/decisions/0016-hc-preflight.md) Preflight checks before using hc | 4.1, 4.5, 9.2 | – | – | M5.1, M5.2, S1 |
| [0017](../architecture/decisions/0017-resume-children-from-a-roster.md) Resume children from a roster after a restart | 2.2, 5.1, 5.4, 6.1, 6.3, 8.5 | P2.1, P3.4, P6.1 | – | S6 |
| [0018](../architecture/decisions/0018-coordinator-runs-claude-directly.md) The coordinator terminal runs `claude` directly | 5.1, 5.3, 5.5, 8.4 | P3.1c, P3.4, P6.1 | M3.1, M4.1, M4.2, M6.1 | S2, S6, S10 |
| [0019](../architecture/decisions/0019-ship-crook-as-a-binary.md) Ship crook as a binary, installed by mise, that installs its herdr mod | 0.1, 1.1, 3.1 to 3.4, 5.1, 8.2 to 8.5, 9.1, 9.2 | P1.1 to P1.3, P4.1 to P4.3, P5.1, P6.1 | M0.1, M1.1, M1.2, M2.1, M2.3, M5.1 to M5.3, M6.1 | M2.1 to M2.4, M3.1 to M3.5, M4.1 to M4.5, M5.2, M5.3, M7.1 to M7.4, M8.1, S7, S13 |
| [0020](../architecture/decisions/0020-mechanical-steps-run-through-crook.md) Mechanical steps run through `crook` subcommands | 0.1 to 0.3, 4.1, 4.3 to 4.5, 5.3 to 5.5, 6.1 to 6.3, 7.1, 7.2, 8.1, 8.3 to 8.5, 10.1, 11.1 | P1.3, P3.1a, P3.1c, P3.4, P4.2, P6.1 | M1.2, M2.1, M2.2, M3.2, M4.1, M4.2, M6.1 | M0.6, M2.2, M4.4, M4.5, M5.1, M5.2, M5.4, M6.3, M7.3, S1, S2, S3, S6, S10 |
| [0021](../architecture/decisions/0021-show-flocks-with-herdr-apps-own-tree.md) Show a flock with herdr-app's own space, tab and agent tree | 1.3, 5.2, 5.3 | P3.1b, P6.1 | M0.1, M1.1, M2.2, M3.2, M4.1, M6.1 | S2, S3 |
| [0022](../architecture/decisions/0022-rust-fresh-code-v1-tooling.md) Rust, fresh code on main, v1's tooling reused | 0.1, 0.2 | P4.1 | M1.3 | M0.1 to M0.6, M1.1 to M1.5, M2.6, M3.6, M4.1, M6.1, M7.0, M7.1, M8.2 |
| [0023](../architecture/decisions/0023-crook-manages-child-worktrees.md) crook creates and removes child worktrees | 1.3, 2.5, 5.5, 6.1, 6.2, 9.2 | P2.1, P3.1a, P3.1c, P6.1 | M4.2 | M5.5, S8, S10 |
| [0024](../architecture/decisions/0024-flock-file-formats.md) File formats for the roster, briefs, results and delivery marks | 2.1 to 2.3, 2.6, 5.4, 6.1, 7.1, 7.2 | P2.1, P3.1a, P3.1b, P3.2, P4.2, P5.1 | M1.2, M2.2 | M1.6, S2 |
| [0025](../architecture/decisions/0025-roles-and-child-models.md) Two roles; the coordinator picks each child's model | 1.2, 2.2, 5.1, 6.1, 6.3 | P1.2, P3.3, P5.1, P6.1 | – | M2.1, M2.3, S3, S9, S12 |
| [0026](../architecture/decisions/0026-config-file.md) crook's settings live in an XDG config file | 1.1, 1.2, 9.2 | P1.2, P3.3 | – | M5.1 |
| [0027](../architecture/decisions/0027-the-coordinator-calls-crook-by-a-literal-path.md) The coordinator calls crook by a literal path | 5.1, 5.3, 8.5 | P3.1a, P5.1 | – | S3 |

Every accepted ADR with something to build has at least one task. 0011 is Rejected; the plans cite it only as
history.
