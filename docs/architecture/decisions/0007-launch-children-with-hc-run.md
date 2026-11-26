---
status: Proposed
date: 2026-11-26
decision-makers: maintainer
---

# Launch children with `hc run`, templates and brief files

## Context and problem statement

The coordinator must start children with a role, a model, a task and enough context to work without asking
back ([0003](0003-coordinator-session-delegates-to-children.md)).

## Considered options

- `hc run --bg` with one hc template per role, and a brief file per child.
- `hc run --bg claude ...` with everything in the task text.
- Claude Code's own subagents.

## Decision outcome

Chosen option: hc templates plus brief files.

- One hc template per role in `~/.config/herdr-core/templates/<name>.md`. Its frontmatter sets the command,
  including the model.
- The coordinator writes a brief per child: goal, why, scope, references, links, and where to report.
- It starts the child with
  `hc run --bg --name <child> --label crook.effort=<id> <template> "Read <brief>. Report to <coordinator>."`.
  The task becomes the child's first message.
- It passes `claude --session-id <uuid>` so the child's transcript is known before it starts. How a
  session id gets through a template's fixed command is untested.
- It never passes `--no-notify`: parent notes are how the coordinator hears that a child blocked or exited.
- It does not pass `--outlive-parent`; see [0011](0011-shell-hosted-coordinator.md).

### Consequences

- Good, because the brief is the record of what each child was asked to do and why.
- Good, because children appear in herdr-app under the coordinator's tab.
- Bad, because templates live in the user's hc config, outside this repository; packaging is an open
  question.

### Confirmation

Fixed session ids were confirmed in spike 6 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).
Templates have not been tried yet (`hc templates` listed none).

## Pros and cons of the options

### Claude Code subagents

- Bad, because they are not resumable sessions the maintainer can open, read and talk to in herdr-app.
