---
status: Proposed
date: 2026-11-26
decision-makers: maintainer
---

# Messages over `hc send`; the results folder is the inbox

## Context and problem statement

Parent and children need to message each other. The question was whether to build an inbox.

## Considered options

- `hc send` for delivery, files for anything long, the results folder as the durable inbox.
- A custom inbox (database or mailbox files) that sessions poll.

## Decision outcome

Chosen option: `hc send` plus files.

- `hc send <t> <text>` types the text in and presses Enter. It waits for a person's unfinished draft, and
  the receiver sees it tagged, for example `<hc from="t7 review">…</hc>`.
- Long content goes in a file; the message carries its path.
- A child writes its result to `results/<child>.md` before sending. A "delivered" checksum per result
  records what the coordinator has seen. If the send fails, the mark stays old, and a UserPromptSubmit hook
  in the coordinator reports every result whose checksum differs from its mark, at start and on every turn.

### Consequences

- Good, because nothing polls and there is no store to keep consistent.
- Good, because failed sends are loud: exit 9 (blocked), 11 (finished), 3 (not found). Nothing is silently
  queued or lost.
- Bad, because a message sent while the coordinator is blocked is refused, not held; the per-turn rescan
  covers that.

### Confirmation

Spikes 2 and 14 in the [spike report](../research/2026-11-26-hc-herdr-app-spikes.md).

## Pros and cons of the options

### A custom inbox

- Bad, because sessions cannot poll: a shell tool call blocks the session until it returns.
- Bad, because it duplicates delivery that hc already does.
