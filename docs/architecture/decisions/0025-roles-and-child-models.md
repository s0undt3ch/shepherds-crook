---
number: 25
title: "Two roles; the coordinator picks each child's model"
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
---

# Two roles; the coordinator picks each child's model

## Context and problem statement

Roles are plugin agents, `agents/<role>.md` ([0007](0007-launch-children-with-hc-run.md)), kept thin
([0019](0019-ship-crook-as-a-binary.md)). Children do work of very different weight: a lookup, a refactor, a
hard bug. One model for every child either wastes money or fails on hard work.

## Considered options

- Two roles; the coordinator picks each child's model when it spawns it, cheapest first, and asks the
  maintainer before moving a child to a more expensive model.
- One fixed model per role, in the agent file.
- More roles, one per kind of work, each with its own model.

## Decision outcome

Chosen option: two roles, model chosen per child.

- Roles: `coordinator` and `child`. Each agent file says who the session is and which skill to use; the skills
  hold the behaviour. More roles are added later as more agent files.
- The coordinator runs on `coordinator_model` from the config file ([0026](0026-config-file.md)), or on the
  maintainer's default model when that is unset.
- `crook spawn` takes `--model <model>`. The coordinator picks the cheapest model it judges able to do the
  brief well; the skill gives the guidance (Haiku for mechanical work, Sonnet by default, Opus for hard
  reasoning). Without `--model`, `spawn` uses `default_child_model` from the config file, `sonnet` when unset.
  The roster records the model actually used, and `crook resume` uses it, even if the config changes later.
  `--model` overrides the agent file's `model` (spike 26).
- When a child's result shows the model was not enough (`status: failed`, or work the coordinator judges
  short of the brief), the coordinator asks the maintainer before moving it to a more expensive model. It
  names the child, the model it would move to, and why. On a yes, crook resumes the child on that model:
  `crook resume <child> --model <model>`, which updates the roster. The child keeps its conversation (spike
  26).

### Consequences

- Good, because most children run on the cheapest model that works.
- Good, because the maintainer decides every step up in cost.
- Good, because a moved child keeps its conversation and its worktree.
- Bad, because the coordinator's first pick can be wrong, which costs one failed attempt before the question.

### Confirmation

Spike 26 of the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md): `--model sonnet` beat an
agent file's `model: haiku`, and a resume with `--model opus` moved the child to Opus with its conversation.

## Pros and cons of the options

### One fixed model per role

- Bad, because every child pays for the hardest task's model, or the hardest tasks fail.

### More roles, one per kind of work

- Good, because each role can carry its own instructions.
- Bad, because the roles would mostly differ only in the model, which a flag already sets.
