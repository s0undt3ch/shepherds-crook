---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# No access restrictions for children, initially

## Context and problem statement

Children need to read the flock's documents, including the notes root. Claude Code's `--add-dir` grants
read and write: its prompt says Claude "will be able to read, edit, and execute files here". A read-only
folder would need deny rules for `Edit` and `Write`; whether those also stop shell commands from writing has
not been checked.

## Decision outcome

No permission rules for children in the first version. Children get the same directory access as the
coordinator.

### Consequences

- Good, because there is nothing to configure or debug.
- Bad, because a confused child can edit the notes root. Revisit if that happens.
