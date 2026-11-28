---
name: crook-e2e
description: Use when asked to run crook's end-to-end tests, the e2e checklist or a smoke test against a real hc core, to check a crook change in real Claude Code sessions before it is pushed, or to reproduce a bug that only shows across the coordinator, its children, hc and the flock's files.
---

# crook e2e

You drive the run yourself. The maintainer pastes nothing. Every session, prompt and answer goes through the
scripts in `scripts/e2e/`, which call `hc` and `crook` on a scratch core of their own. A step passes on what files,
`hc`, `git`, `ps` and Claude's transcripts show, never on what a model says: a coordinator has reported children
"running" while they sat at a trust dialog.

macOS or Linux with `lsof`, `jq`, `git`, `shasum`, `hc` and `toolr`. The release plan's "E2E harness" section is
the spec; the scenario numbers below (S1 to S13) are its.

## Limits (non-negotiable)

- Every `hc` and `crook` call goes through `scripts/e2e/hc.sh` or `scripts/e2e/crook.sh` (from here on, `hc` and
  `crook` below mean `bash scripts/e2e/hc.sh $ENVF` and `bash scripts/e2e/crook.sh $ENVF`). They give the call the
  run's own `HC_SESSION`, sockets and folders, remove the variables of ADR 0020 and refuse to run when the result
  does not point at the run. Never type a bare `hc` or `crook`: this repository's own session may sit inside the
  maintainer's real core.
- Never touch the maintainer's real hc core, `~/.config/herdr-app`, `~/.local/share/crook` or
  `~/.local/state/herdr-core`. The real `herdr-app` is never on a run's `PATH`: a stub is always first. The real one
  is called only by S13 step 6, on a copy in a throwaway home.
- Trust and add-dir dialogs are answered only by `trust.sh`, with `hc keys <terminal> down enter`. It refuses any
  path outside the run. On `REFUSED`, leave the run as it is, tell the maintainer, and tear down after they answer.
- Kill a process only with `kill_in_run` (in `lib.sh`), which refuses a pid whose working folder is outside the run.
  `teardown.sh` does it for you.
- Never print an environment value, only names: tokens leaked into a transcript that way once. The scripts print
  names only.
- Never answer a dialog by keying into a session, except a step below that says to. Reject when in doubt. S5 presses
  Enter only when `hc get` reports the blocked kind as `question`, never on a permission dialog.
- A `REFUSED` from `trust.sh` makes the runner keep the run, print it and exit non-zero: do not skip it.
- Never stage, commit or push from a run. Never run `git add -A`.
- End every run with `teardown.sh`, pass or fail. It stops the run's core only, ends what still runs in the run and
  removes the run, or keeps it and exits non-zero when something is left.

## Tiers

| Tier | Command | Uses | Covers |
| --- | --- | --- | --- |
| 1 | `cargo nextest run` (CI) | Stub `hc` and `claude` | Argument order, roster writes, hooks' exit codes |
| 2 | `bash scripts/e2e/tier2.sh` | Real scratch core, stub `claude` | S1 to S3, S6, S8 to S10, S13 |
| 3 | `bash scripts/e2e/tier3.sh` | Real scratch core, real `claude` on Haiku | S1, S4, S5, S6, S7, S9, S11, S12 |

Tiers 2 and 3 are local only: hc cannot run on CI (Q34). Run tier 2 first; it is free. Tier 3 spends model tokens, so
run only the scenarios the change touches (`--only S4,S5`; the runner refuses a name outside S1 S4 S5 S6 S7 S9 S11 S12,
in that spelling), and never loop a failing one.

`cargo build --release` first: both use `target/release/crook`, never your installed one (`--binary <path>` picks
another). The scenarios that tier 2 and tier 3 share (S1 and S9) are checked twice, on purpose: tier 2 for argv,
tier 3 for what a live session does.

## Setup and teardown by hand

Shell variables do not survive between Bash calls, and the Bash tool's shell may be zsh: write scripts in files, or
start every call with `bash -c`. A `-u NAME` list kept in a zsh string removes nothing (Q37), so never type a scrub
inline.

1. `ENVF=$(bash scripts/e2e/setup.sh --tier 3 [slug])` makes `$TMPDIR/crook-e2e-run.*` and `/tmp/crook-e2e-rt.*`
   and prints the run's env file. `--tier 3` records the caller's `HOME` and `CLAUDE_CONFIG_DIR` (paths, in the env
   file only) so Claude can log in; everything else stays the run's. Tier 2 uses a scratch `HOME`.
2. `bash scripts/e2e/install.sh $ENVF <version> target/release/crook` stages a release-shaped install under
   `$RUN/install/<version>/` and activates it behind the fake shim `$RUN/shims/crook`. `--no-activate` keeps the
   active version.
3. `crook new <slug> --task "..."` starts a flock on the scratch core; then `trust.sh $ENVF <slug> 120`.
4. `bash scripts/e2e/teardown.sh $ENVF [--keep]`.

The scripts `tier2.sh` and `tier3.sh` do all of this themselves and end with teardown. `--keep` leaves the run for
inspection; remember to tear it down.

## The tier 3 runner

`bash scripts/e2e/tier3.sh [--keep] [--only S4,S5,...] [--binary <crook>] [--permission-mode auto|default]`. The
Bash tool stops a call at ten minutes, and a model turn takes 5 to 60 s: start it with `run_in_background` and read
the log.

The run has its own `HOME` and Claude config folder. `login.sh` copies the maintainer's Claude login into the run
without its refresh token, and teardown deletes the copy (D122). It refuses when the access token has under an hour
left: say so to the maintainer, who refreshes it by using Claude Code once. `auto`, the default, is the mode the
maintainer's own sessions run in. `--permission-mode default` is the check that the coordinator's crook calls need
no approval (ADR 0027), nor do its brief write, `crook read` and `crook send` (ADR 0028). S1 alone covers it, as in
`--only S1 --permission-mode default`. Under `default` the children's own tool calls would wait for approval, so run
no other scenario in that mode.

What it does, per scenario, each on its own flock:

| # | Does | Passes when |
| --- | --- | --- |
| S1 | Starts the core through `crook status`, probes a terminal's `env`, starts a real coordinator | `core_env.sh` ok; the coordinator's screen has no "Transcript saving is off"; its argv has `--model haiku` |
| S4 | A child writes `done`; then a no-change turn; then a change; then `/clear` | `.delivered.json` equals the SHA-256 of the file; the coordinator's screen has an `<hc from=…>` line `Result updated: <path>`; a no-change turn sends nothing |
| S5 | Blocks the coordinator on an AskUserQuestion, updates a child, makes the child ask too, answers | The mark matches what was delivered; after the next prompt the rescan names the result; Q28's two outcomes are printed as notes |
| S6 | `hc server stop`, then `crook open` | The coordinator and the active child restart with `--resume <id>` under their own names, the done child does not, the roster is unchanged, one core socket |
| S7 | Installs two versions, starts on A, moves `active` to B, runs B once | `current` moves to B; at the next turn end the record moves to B and `/reload-plugins` lands; the next prompt has the nudge; a marker in the skill answers B; A again leaves `current` on B |
| S9 | Tells a child a word, `crook resume --model sonnet`, asks for it back | argv has `--model sonnet --resume <id>`; the roster says sonnet; the word is in the result file |
| S11 | A child briefed to ask with AskUserQuestion | `hc get` shows it blocked; the coordinator has hc's note and names child and question; no `hc send` to the child |
| S12 | A child whose result is `failed`; then the maintainer's "yes" | No AskUserQuestion and no resume before the yes; after it the roster says a new model |

Facts the runner depends on, each seen in a real run (see build-log D98 and following):

- `hc send <name> "<text>"` from the harness arrives untagged, so a session reads it as the maintainer typing. That
  is how the harness gives "consent". A child's Stop hook sends tagged `<hc from="t2 <slug>.<child>">`, which a
  coordinator must never take as consent.
- `hc get --json` has `.status.app == "claude-code"` and `.status.state` (`done`, `working`, `blocked`); `hc ls --json`
  has `.agent == "claude-code"` (tier 2's stub reports `claude`).
- Every child launch ends at an "Adding a new directory" dialog for the notes folder: `trust.sh` answers it. A
  resumed session shows it again.
- A transcript is `<claude config>/projects/<mangled cwd>/<session id>.jsonl`; find it by session id, never compute
  the folder name. `tool_use` entries, `hook_additional_context` entries and `<hc …>` user lines are the evidence for
  what a coordinator did and was told.
- Sessions start in whatever permission mode the maintainer's settings give (it was `auto`): a run that passes "no
  Bash prompt" proves nothing for Q21 under `auto`. The runner prints the mode.

## If a run was killed

The runner prints `run: <RUN>` first. If it died without tearing down (or tore down and said it left the run), run
`bash scripts/e2e/teardown.sh <RUN>/run.env`. It stops the run's core, ends processes whose working folder is in the
run, and also reports processes that carry the run's `HC_SESSION` in their environment but work elsewhere (it leaves
those alone and keeps the run). That last check reads `ps ewwax`: macOS hides the environment of its own system
binaries from `ps`, so it sees only ordinary programs such as `claude` and `hc`; it is a net, not a proof.
`MISE_DATA_DIR`, if set, passes the scrub; it does no harm while `CROOK_BIN` is set.

## Reading a result

One line per scenario step: PASS, FAIL or NOT EXERCISED, with the evidence. NOT EXERCISED means the model would not
do what the step's prompt asked (no AskUserQuestion, no resume); send the prompt again once by hand, and if it still
does not, report it as that, not as a crook failure. Each FAIL is: what you saw against what the step expects, and
whether it is a crook bug, a design gap or the run.

Open the failing step's evidence first: the coordinator's transcript (`tool_use` lines), `roster.json`,
`results/.delivered.json`, `hc read <terminal> --lines 200`, `ps -axww -o command=`.

## What the checks read

If a check starts failing for no reason, look for a wording change first:

- Claude Code dialogs: `Accessing workspace:`, `hasn't worked here before`, `Transcript saving is off`.
- crook's hooks: `Result updated: <path>`, `Undelivered child result: <path>`, `the plugin was reloaded to a newer
  version` (`src/hook/`).
- hc's own notes: `<hc>t2 <name> is blocked (question)…</hc>`.
- crook's CLI: `warning: kept worktree <path>: <why>`, `opened flock <slug>`, `spawned <slug>.<child>`.

## Mistakes that cost runs

| Symptom | Cause |
| --- | --- |
| A session exits right after start | Enter on a trust dialog whose default is "No, exit". Use `trust.sh` |
| A child sits at `blocked` and never works | The add-dir dialog for the notes folder: run `trust.sh` on its terminal |
| A harness `hc send` fails with exit 9 | The terminal is blocked; nothing is queued. Answer its dialog, or `crook stop` it |
| A check passes on an empty `hc` failure | `! hc … \| jq` and `test -z "$(hc …)"` pass when hc fails. Read first, then search |
| `crook` refuses "user verbs cannot run inside Claude Code" | A bare `crook` from your own session. Use `crook.sh` |
| `hc` finds the maintainer's core | A bare `hc` inherited `HC_TERMINAL_SOCKET`. Use `hc.sh` |
| A flock slug is refused | At most 16 characters of `a-z`, `0-9`, `-`, and not `t<digit>…` (it looks like an address) |
| A transcript is not where you computed it | Claude mangles `.` too, and the run path has one. Search by session id |

## Report

Say, once, in the report: tier 3 runs in its own `HOME` and Claude config folder with a copy of the maintainer's
login that cannot refresh, and teardown deletes the copy. Its sessions load none of the maintainer's Claude settings,
plugins or hooks. Quote the snapshot result and the two checks that the maintainer's Claude folder got no project
folder and no `.claude.json` entry from the run. The runner compares names and mtimes of `~/.config/herdr-app/mods`,
`~/.local/share/crook`, `~/.local/state/crook`, `~/.config/crook`, `~/.config/herdr-core` and
`~/.local/state/herdr-core` before and after; any change is a failure, except the maintainer's own core's registry
(`~/.local/state/herdr-core/default/terminals.json` and its folder), which moves with their own terminals. For that
file the runner fails if it is missing or names this run (the run's path or `HC_SESSION`).

## Local checks in the real app

S2's space and tab names, the Agents tree (0021) and the mod's dialog need the maintainer's real herdr-app, because
only the core it watches is shown. They stay manual and run only when the maintainer says so, with throwaway labels
and `space.close` afterwards. The runs above never touch that app.
