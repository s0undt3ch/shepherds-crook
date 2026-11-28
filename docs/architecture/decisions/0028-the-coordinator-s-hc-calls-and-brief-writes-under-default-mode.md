---
number: 28
title: "The coordinator's hc calls and brief writes under default mode"
status: proposed
date: 2026-11-28
decision-makers:
- maintainer
---

# The coordinator's hc calls and brief writes under default mode

## Context and problem statement

[0027](0027-the-coordinator-calls-crook-by-a-literal-path.md) lets the coordinator call crook with no prompt under
`default` permission mode. The coordinator does more than call crook. It writes `briefs/<child>.md` in the flock
folder, and `README.md` and `notes/` in the notes folder. It runs `hc read <terminal>` to look at a blocked child,
and `hc send <terminal> "<text>"` for follow-ups. None of these match 0027's one rule, so under `default` each one
asks the maintainer. The maintainer runs in `auto` mode and does not see this. Anyone in `default` mode does.

## Decision drivers

- In `default` mode, the routine steps of a flock should not ask the maintainer each time.
- A rule must not let the coordinator do what its skill forbids. The skill forbids `hc send --force`, which would
  answer a dialog that belongs to the maintainer, and sends to terminals outside the flock.
- Writes stay inside the flock and notes folders.

## Considered options

- Option A: a `crook send <child> <text>` subcommand, plus allow rules for `hc read` and for writes in the flock's
  folders.
- Option B: allow rules for `hc send`, `hc read` and the writes, with no new subcommand.
- Option C: change nothing. Document that `default` mode asks for these steps, and that `auto` or `acceptEdits`
  avoids it.

## Decision outcome

Proposed: Option A, because it is the only one that removes the prompts without also allowing `hc send --force`
or a send outside the flock.

- `crook send <child> <text>` resolves the child's terminal from the roster and runs `hc send` without `--force`.
  It refuses a name that is not a live child of this flock. 0027's rule already covers it, so it needs no new rule.
- `crook open` adds, next to 0027's rule:
  - `Bash(hc read *)`. Reading a screen changes nothing.
  - `Edit` and `Write` rules for `<flock>/briefs/**`, `<notes>/README.md` and `<notes>/notes/**`.
- The coordinator skill uses `crook send` for follow-ups and keeps `hc read`.

### Consequences

- Good, because a flock runs in `default` mode with no prompt for its routine steps.
- Good, because `--force` and sends outside the flock stay impossible, not just forbidden by the skill.
- Bad, because crook gains a subcommand that wraps `hc send`.
- Bad, because the rule syntax for absolute paths in `Edit` and `Write` rules is not checked yet.

### Confirmation

- A spike, as for 0027: which `Edit` and `Write` rule spelling matches an absolute path, and whether `hc read *`
  matches the skill's calls.
- Unit tests: `crook send` refuses an unknown or ended child and never passes `--force`.
- Tier 3 S1, or a new scenario, under `--permission-mode default`: a brief write, an `hc read` and a `crook send`
  run with no prompt.

## Pros and cons of the options

### Option B

- Good, because nothing new is built.
- Bad, because `Bash(hc send *)` also allows `hc send --force` and sends to any terminal in the core, including
  the maintainer's own sessions.

### Option C

- Good, because nothing changes.
- Bad, because a `default`-mode maintainer approves every brief, every look at a child and every follow-up.

## More information

- Raised on 2026-11-28 while 0027 was confirmed. Tier 3 S1 checks only a crook call (build log D120 to D123).
- Spike 27 in [the spikes](../research/2026-11-26-hc-herdr-app-spikes.md) showed how Bash rules match.
