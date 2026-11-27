---
status: Accepted
date: 2026-11-27
decision-makers: maintainer
---

# Launch children with `hc run`, crook's agents and brief files

Amended by [0020](0020-mechanical-steps-run-through-crook.md): `crook spawn` builds and runs the launch
command below; the coordinator no longer types it.

## Context and problem statement

The coordinator must start children with a role, a model, a task and enough context to work without asking
back ([0003](0003-coordinator-session-delegates-to-children.md)). The maintainer does not want crook's files
in herdr-core's folders.

## Considered options

- `hc run --bg claude` with a Claude Code agent from crook's own plugin as the role, and a brief file per
  child.
- `hc run --bg` with one hc template per role.
- `hc run --bg claude ...` with everything in the task text.
- Claude Code's own subagents.

## Decision outcome

Chosen option: crook's plugin agents plus brief files.

- Each role is a Claude Code agent, `agents/<role>.md` in crook's plugin. Its frontmatter sets the model; its
  body is the role's instructions.
- The coordinator picks the child's session id, writes the roster entry
  ([0017](0017-resume-children-from-a-roster.md)) and a brief: goal, why, scope, references, links, and
  where to report.
- It starts the child with the command below. The last argument is Claude's first prompt.
- The prompt must follow a flag that takes exactly one value. Flags that take several values, such as
  `--allowedTools`, swallow it.
- It never passes `--no-notify`: parent notes are how the coordinator hears that a child blocked or exited.
- It does not pass `--outlive-parent`; see [0018](0018-coordinator-runs-claude-directly.md).

```sh
hc run --bg --name <child> --label crook.flock=<id> \
  claude --plugin-dir <crook> --agent <role> --session-id <uuid> "Read <brief>. Report to <coordinator>."
```

### Consequences

- Good, because roles ship in crook's plugin; nothing goes in `~/.config/herdr-core`.
- Good, because the coordinator sets the session id itself, so the roster needs no hook to learn it.
- Good, because the brief is the record of what each child was asked to do and why.
- Good, because children appear in herdr-app under the coordinator's tab.
- Bad, because hc's template launch tracks whether the first message was sent and retries while the child
  starts. With a plain launch, the prompt rides on Claude's command line and hc reports nothing about it.
  Spike 16 saw it arrive in every case, including after a trust dialog.

### Confirmation

Spike 16 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): role prompt and model from
the agent, session id honoured, resume with a prompt, prompt kept through a trust dialog.

## Pros and cons of the options

### hc templates

- Bad, because hc reads templates only from `~/.config/herdr-core/templates`; `hc templates --help` names
  that folder and `hc run` has no option to change it.
- Bad, because hc drops extra arguments after a template name, so `--session-id` cannot be passed (spike 13).

### Claude Code subagents

- Bad, because they are not resumable sessions the maintainer can open, read and talk to in herdr-app.
