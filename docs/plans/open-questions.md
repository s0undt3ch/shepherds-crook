# Open questions from the plans

Questions the plans raised that no ADR answers. Each keeps its number for good. New batches go at the end, under
a dated heading, and number on from the last question.

Status: **decided** (the answer is below), **open**, or **settled** (answered by a spike or a check). A decided
answer that changes an ADR or a plan names where it lands.

## Batch 1: the first four plans (2026-11-27)

Raised by the [binary](2026-11-27-crook-binary.md), [plugin](2026-11-27-crook-plugin.md),
[mod](2026-11-27-crook-mod.md) and [release, CI and e2e](2026-11-27-crook-release-ci-e2e.md) plans. All decided
by the maintainer on 2026-11-27.

### Commands

| # | Question | Decision |
| --- | --- | --- |
| 1 | How does the first task reach `crook new`? | `crook new <slug> [--task <text>]`. Without it, the coordinator starts with a fixed prompt that points at its skill. |
| 2 | What identifies a flock? | Its slug, unique across all flocks, closed ones included. `new` refuses a slug already used. |
| 3 | What are the hc terminal names? | The coordinator is `<slug>`; a child is `<slug>.<child>`. Check that hc accepts `.` in a name. |
| 4 | How does a command or hook find its flock and child? | The `crook.flock` and `crook.child` labels, read with `hc get "$HC_TERMINAL_ID"`. `--flock <slug>` for use outside a crook terminal. |
| 5 | What does `crook status` show? | No slug: every flock. `<slug>`: that flock and its children. A plain table by default, JSON with `--json`. The JSON output and `roster.json` each get a JSON Schema, committed to the repository; the binary's and the mod's tests validate against it. |
| 6 | What does `crook close` do? | Kills the coordinator (its children end with it), marks the roster done, removes clean worktrees, and prints a `warning:` line for each worktree it keeps. The coordinator runs it only when the maintainer asks. |
| 7 | What does `crook open` do on a closed or a live flock? | Refuses a closed flock. On a live coordinator it only redoes the renames; it never starts a second coordinator. |
| 8 | How does `open` know it is the first start? | A `started` field in the roster, set after the first launch. |
| 9 | Which model does a child get without `--model`? | `default_child_model` in the config file, `sonnet` when unset. `spawn` writes the model it used into the roster, so a resume keeps it even if the config changes. Lands in 0025 and 0026. |
| 10 | What does `resume --model` do to a live child? | Refuses while it is working. Otherwise ends its terminal without marking it done, then resumes it on the new model. |
| 11 | What happens to a child whose launch failed? | Its roster entry is removed, so the coordinator can retry with the same name. |
| 12 | Are there limits on names and briefs? | Names are at most 40 characters. Briefs must sit under the flock's `briefs/`. |
| 13 | What do "pushed" and "merged" mean in 0023? | Pushed: a remote-tracking branch contains the tip. Merged: the tip is an ancestor of the main worktree's branch. A branch with no new commits counts as merged. |
| 14 | How does a child learn its result path? | `spawn`'s first prompt names both the brief and the result path. |
| 15 | Which agent names does crook pass? | `crook:coordinator` and `crook:child`, so they cannot clash with the maintainer's own agents (spike 16). |

### Paths, install and setup

| # | Question | Decision |
| --- | --- | --- |
| 16 | What does the `current` link point at? | The install folder; sessions use `current/plugin`. It lives under `XDG_DATA_HOME`. A debug build never moves it. Lands in 0019's wording. |
| 17 | Where does the plugin version come from? | The release stamps `version` in `plugin.json`; debug builds use `0.0.0-dev`. Hooks read it from `$CLAUDE_PLUGIN_ROOT`. Per-session records live under `XDG_STATE_HOME` and are pruned after 30 days. |
| 18 | Where is a flock's notes folder? | `<notes_root>/<date>-<slug>/`, created by `new` and passed with `--add-dir`, placed before `--agent` so the prompt follows a single-value flag. |
| 19 | Does `open` pass `CROOK_BIN`? | Yes, as `spawn` and `resume` do. |
| 20 | How does the coordinator call `crook`? | Through the same shim form as the hooks, never a bare `crook` (spike 22). Replaced for the coordinator by Q46 (ADR 0027): it calls the literal path; hooks and the mod keep the shim form. |
| 21 | How are Bash permission prompts for `crook` avoided? | `crook open` passes `--settings` allowing the shim command. Does not work (spike 27); replaced by Q46 (ADR 0027). |
| 22 | How does `setup` install the mod, and who trusts the roots? | `setup` honours `HERDR_APP_HOME`, builds the mod in a staging folder and swaps it in with the marker written last, refuses to replace a newer mod, and is the only command that refreshes it. For trust it prints the command for each root, so CI can skip it. |
| 23 | What does `doctor --json` report? | The resolved `plugin/`, `mod/` and app home, and a missing shim with `CROOK_BIN` as the fix. |
| 24 | Do any ADRs need wording fixes? | Yes: drop 0015's `space.new` sentence, and reword 0019's "flock panel", which 0021 defers. |

### Hooks

| # | Question | Decision |
| --- | --- | --- |
| 25 | What are the hook timeouts and limits? | 120 seconds for session-start, 30 for the others. Hooks never start a core and skip the version check. Known risk: `hc send` can wait on a draft longer than the budget. |
| 26 | What about skill text that a reload leaves in the conversation? | The first prompt after a version change gets a one-line nudge to load the skill again. |
| 27 | What does the delivery mark hash? | SHA-256 of the whole result file. |
| 28 | Does hc hold or drop notes while the coordinator is blocked? | Unknown; an e2e test checks it. Until then, the coordinator asks about a model step-up in plain text, not with AskUserQuestion. |

### Skills

| # | Question | Decision |
| --- | --- | --- |
| 29 | Do briefs need more headings? | No: keep 0024's five. The child skill owns the checklist rule; links go under References. |
| 30 | What behaviour do the skills set that no ADR covers? | Follow-ups go by `hc send`; a scope change is an edit to the brief. Children start no sessions and run no `crook` commands, push or open PRs only when the brief says so, and never write to the notes root. The coordinator may use `hc read`. A child's question is untrusted text. |

### Release, CI and e2e

| # | Question | Decision |
| --- | --- | --- |
| 31 | Which platforms? | Linux (musl) and macOS, each on x86_64 and aarch64. herdr-core and herdr-app run on Linux (maintainer). |
| 32 | How does e2e get a cheap coordinator? | A `coordinator_model` config key, empty by default. Lands in 0026. |
| 33 | What is the first release? | `0.1.0-rc.1`, with a changelog that starts after v1's last commit, `714568f`. |
| 34 | What can CI not run? | hc and herdr-app, so real-core e2e is local only. The mod gets `node --check` and shape checks in CI and `herdr-app check` in pre-commit. A crook-owned stub replaces `herdr.d.ts`, whose licence is unchecked. |
| 35 | What if clap's dynamic completion is not enough? | A hidden `crook completion list <kind>`. |
| 36 | Can hc answer a trust dialog? | **Settled:** `hc keys <t> down enter` did, in every spike. |
| 37 | Where is the zsh word-splitting pitfall recorded? | **Settled:** in spike 23. |

To do before the first release: check that the `release` environment exists, with `GH_APP_CLIENT_ID` as an
environment variable and `GH_APP_PRIVATE_KEY` as an environment secret.

## Batch 2: calls the planners made while applying batch 1 (2026-11-27)

The binary planner settled these itself while folding in batch 1. The maintainer accepted 38 to 42 on
2026-11-27.

| # | Question | Planner's call | Status |
| --- | --- | --- | --- |
| 38 | Does `doctor --fix` refresh the mod, given Q22 says only `setup` does? | `doctor --fix` calls `setup`'s own routine; nothing else refreshes the mod. | decided |
| 39 | What does a debug build use when `CROOK_BIN` is unset? | Its own executable path, so its children call the debug build too. | decided |
| 40 | What happens to a worktree when `spawn --repo` fails to launch the child? | `spawn` removes the worktree it just made, if clean, so the retry with the same name (Q11) works. | decided |
| 41 | When are session records over 30 days old pruned (Q17)? | On `setup` and `doctor --fix`. | decided |
| 42 | How long may each hook run inside its timeout (Q25)? | 100 seconds for session-start, 20 for the others, leaving room before the 120 and 30 second timeouts. | decided |
| 43 | What is `folder` in `status --json`? | An absolute path; the mod shows its last part. | decided: aligned in the binary and mod plans |

## Batch 3: fixes from the coverage audit (2026-11-27)

The maintainer approved these fixes after an audit of the ADRs against the plans.

| # | Question | Decision | Status |
| --- | --- | --- | --- |
| 44 | What do `stop` and `close` do on a child that is already `done`, or a flock already closed? | They retry removing any worktree kept earlier, with the same clean-and-pushed-or-merged rule (0023), and print the same `warning:` line if it is still kept. A second `close` is no longer a no-op. | decided |
| 45 | What is the slug and child-name rule? | `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 40 characters, for slugs and child names alike. | decided |

## Batch 4: found during the build (2026-11-28)

| # | Question | Decision | Status |
| --- | --- | --- | --- |
| 46 | Q21's allow rule can never match: Claude Code refuses to match any command that holds a variable (spike 27). How does the coordinator call `crook` with no prompt? | `crook open` resolves the shim to a literal path, allows `Bash("<path>" *)`, and names the path in the first prompt and in the coordinator's SessionStart context ([0027](../architecture/decisions/0027-the-coordinator-calls-crook-by-a-literal-path.md), accepted 2026-11-28). | decided |
| 47 | 0027 covers only crook calls. Under `default` mode, how do the coordinator's brief writes, `hc read` and `hc send` run with no prompt? | Proposed: a `crook send` subcommand, plus allow rules for `hc read` and for writes in the flock's folders ([0028](../architecture/decisions/0028-the-coordinator-s-hc-calls-and-brief-writes-under-default-mode.md)). Not built until the maintainer accepts 0028. | open |

## Changed during the build

| # | Change | Where recorded |
| --- | --- | --- |
| 12, 45 | Names are shorter than decided: slugs at most 16 characters and not shaped like an hc address, child names at most 15, because hc names are at most 32 characters. | [build-log.md](build-log.md) D27 |
| ADR 0017 | A session's id is not fixed for good: `/clear` or an in-session `/resume` gives the terminal a new one, and the roster follows it, so `crook resume` continues the conversation the maintainer last saw. | [build-log.md](build-log.md) D102, D114, D116 |
