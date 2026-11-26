---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# A coordinator session delegates to child sessions

## Context and problem statement

The maintainer wants one place to talk about a problem (a bug, a feature, an investigation), plan it, and
send work out, without driving each worker session by hand.

## Decision outcome

The workflow:

1. In herdr-app, a dialog starts a new effort.
2. The effort gets a main Claude Code session: the **coordinator**. The maintainer talks to it, plans with
   it, and has it send work to child sessions.
3. When the coordinator delegates, it explains to the child what to do and why, with references and links.
4. Parent and children can message each other.
5. When a child finishes, the coordinator is told.
6. The coordinator can explain what each child is doing and why.

How each step works is decided in the Proposed ADRs 0007 to 0015.

### Consequences

- Good, because the maintainer has one conversation per effort.
- Bad, because the coordinator's context grows with every child report; reports are file paths, not bodies,
  to limit that ([0008](0008-messages-over-hc-send-results-folder-inbox.md)).
