---
number: 28
title: The coordinator's hc calls and brief writes under default mode
status: accepted
date: 2026-11-28
decision-makers:
- maintainer
links:
- target: 27
  kind: amends
- target: 20
  kind: amends
---

# The coordinator's hc calls and brief writes under default mode

## Context and problem statement

[0027](0027-the-coordinator-calls-crook-by-a-literal-path.md) lets the coordinator call crook with no prompt under
`default` permission mode. The coordinator does more than call crook. It runs `hc read <terminal>` to look at a
blocked child, and `hc send <terminal> "<text>"` for follow-ups. It writes `briefs/<child>.md` in the flock folder,
and `README.md` and `notes/` in the notes folder. None of these match 0027's one rule, so under `default` each one
asks the maintainer. The maintainer runs in `auto` mode and does not see this. Anyone in `default` mode does.

## Decision drivers

- In `default` mode, the routine steps of a flock should not ask the maintainer each time.
- A rule must not let the coordinator do what its skill forbids. `hc send --force` types into a blocked program's
  dialog (hc's own help: "it may answer its dialog"), so it could approve a child's permission prompt that belongs
  to the maintainer (0010). `hc send` and `hc read` also reach any terminal in the core, the maintainer's own
  sessions and shells included. A line typed into a shell runs with no Claude permission check at all.
- Writes stay inside the flock and notes folders.

## Considered options

- Option A: flat `crook send <child> <text>` and `crook read <child>` subcommands, plus `Edit` and `Write` allow
  rules for the flock's folders.
- Option B: allow rules for `hc send`, `hc read` and the writes, with no new subcommands.
- Option C: change nothing. Document that `default` mode asks for these steps, and that `auto` or `acceptEdits`
  avoids it.
- Option D: Option A, with the writes also done through crook subcommands.

## Decision outcome

Proposed: Option A, because the coordinator then makes no raw `hc` call at all, so 0027's one rule covers every
send and read, and neither can reach past the flock or use `--force`.

- `crook send <child> <text>` resolves the child's terminal from the roster and runs `hc send` without `--force`.
  It refuses a name that is not a live child of this flock, and keeps `hc send`'s exit codes (9 blocked, 11
  ended) so the skill's handling stays.
- `crook read <child>` resolves the terminal the same way and prints `hc read`'s output. It refuses the same names.
- Both are flat, like `spawn`, `stop`, `resume` and `status`. 0027's rule `Bash("<path>" *)` already covers them.
- `crook open` adds `Edit` and `Write` allow rules for `<flock>/briefs/**`, `<notes>/README.md` and
  `<notes>/notes/**`, next to 0027's rule.
- The coordinator skill drops `hc send` and `hc read` for `crook send` and `crook read`, and makes no `hc` call.

### Consequences

- Good, because a flock runs in `default` mode with no prompt for its routine steps.
- Good, because `--force` and sends or reads outside the flock become impossible, not just forbidden by the skill.
- Good, because no `hc` rule is needed, so no rule can match a call to another terminal.
- Bad, because crook gains two subcommands that wrap `hc send` and `hc read`.
- Bad, because the rule syntax for absolute paths in `Edit` and `Write` rules is not checked yet.

### Confirmation

- A spike, as for 0027: which `Edit` and `Write` rule spelling matches an absolute path.
- Unit tests: `crook send` and `crook read` refuse an unknown, ended or other flock's child, and `crook send` never
  passes `--force`.
- Tier 3 S1, or a new scenario, under `--permission-mode default`: a brief write, a `crook read` and a `crook send`
  run with no prompt.

## Pros and cons of the options

### Option B

- Good, because nothing new is built.
- Bad, because `Bash(hc send *)` also allows `hc send --force` and sends to any terminal in the core, and
  `Bash(hc read *)` reads any terminal in it.

### Option C

- Good, because nothing changes.
- Bad, because a `default`-mode maintainer approves every brief, every look at a child and every follow-up.

### Option D

- Good, because 0027's one rule would cover the writes too, with no `Edit` or `Write` rule.
- Bad, because the text has no clean way in. A long argument has to survive shell quoting, and a `$` in it is a
  variable, which no rule matches (spike 27). A pipe is a compound command. Whether a heredoc's body passes the
  variable check is not known.
- Bad, because the README and notes are edited a line at a time. A command that rewrites the whole file is
  clumsier, and a bad call loses text.
- Bad, because the one gain, checking a brief, can happen at `crook spawn` without crook owning the write.

## More information

- Raised on 2026-11-28 while 0027 was confirmed. Tier 3 S1 checks only a crook call (build log D120 to D123).
- Spike 27 in [the spikes](../research/2026-11-26-hc-herdr-app-spikes.md) showed how Bash rules match.
- `hc send --help` on `--force`: "Type even if the program is blocked (it may answer its dialog)".
- As built, `crook open` also allows reads of the plugin's own files, which the skill sends the coordinator to
  (build log D126 and D127). The rule spelling is spike 28.
