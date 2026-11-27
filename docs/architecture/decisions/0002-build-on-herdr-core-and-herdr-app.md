---
number: 2
title: "Build on herdr-core and herdr-app"
status: accepted
date: 2026-11-26
decision-makers:
- maintainer
---

# Build on herdr-core and herdr-app

## Context and problem statement

crook v1 was a Rust CLI with its own SQLite database, supervising Claude Code sessions in herdr 0.9.5
panes, with herdr optional. herdr-core (`hc`) now runs terminals and agents as a service, and herdr-app is
its desktop UI. Much of what v1 built (launching, parent and child links, messaging, status, notification)
exists in hc.

herdr 0.9.5, herdr-core and herdr-app are three different things. This decision is about the last two.

## Decision drivers

- Stop maintaining a supervisor that duplicates hc.
- The maintainer works in herdr-app.
- Children must be resumable and must be able to ask a human for input.

## Considered options

- Build on herdr-core and herdr-app; both required.
- Keep crook v1 on herdr 0.9.5.
- Claude Code agent teams.
- JDonaghy/code-coordinator.

## Decision outcome

Chosen option: build on herdr-core and herdr-app, both required. This reverses v1's "herdr optional".

### Consequences

- Good, because hc already provides launching (`hc run`), messaging (`hc send`), parent notes, status and
  waiting.
- Good, because herdr-app shows agent status per space and tab, and plays sounds on done and blocked.
- Bad, because crook now depends on a young API (hc 0.1.x). [0016](0016-hc-preflight.md) adds a preflight.
- Bad, because the v1 Rust code is retired, not ported.

## Pros and cons of the options

### Keep crook v1 on herdr 0.9.5

- Good, because it exists and has tests.
- Bad, because it keeps a second supervisor, database and wake-up logic beside hc.

### Claude Code agent teams

- Bad, because it runs in tmux or iTerm only.
- Bad, because teammates cannot be resumed.
- Bad, because it has no channel for a teammate to ask a human.

### JDonaghy/code-coordinator

- Bad, because it runs on tmux and `claude -p`, and uses GitHub issues as its message bus.

## More information

petekp/coordinator is the closest reference for the intended workflow. Ideas borrowed from the reviewed
projects: mailbox write semantics, handing off a final answer when idle, rebuilding state from `hc ls`
labels, and stripping orchestrator environment from children.
