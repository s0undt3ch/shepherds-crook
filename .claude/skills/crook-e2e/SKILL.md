---
name: crook-e2e
description: Use when asked to run crook's end-to-end test, e2e checklist or smoke test against real Claude Code sessions, to check a crook change in real sessions before a PR, or to reproduce a bug that only shows across the orchestrator, children, herdr panes and the crook database.
---

# crook e2e

You drive the run yourself through herdr; the user does not paste anything. Every session, prompt and answer goes
through `herdr pane` commands and the scripts in `scripts/e2e/`. A step passes on what the database, git and process
table show, never on what a model says: an orchestrator has reported children "running" while they sat at a trust
prompt.

## Limits

- Accept a trust or add-dir prompt only through `bash scripts/e2e/trust.sh`, which refuses any path outside
  `$TMPDIR/crook-e2e-*`. On `REFUSED: not a crook-e2e path`: leave the run as it is and ask the user; tear down after
  they answer.
- Permission prompts in the run's sessions: approve or reject as the step needs. Reject when in doubt.
- Touch only the run: its directory, its herdr workspaces, its processes (check a pid's cwd is under `$RUN` before
  killing it). Never the user's real crook state, workspaces or sessions. The one exception is setup's
  `cargo install`, which replaces the user's `crook` binary with this checkout's on purpose.
- End every run with `bash scripts/e2e/teardown.sh "$envf"`, pass or fail.

## Setup

1. `cargo install --locked --path .` so the run uses this checkout. The mod is embedded, so this covers it too.
2. `envf=$(bash scripts/e2e/setup.sh)`, then `source "$envf"`. It sets `RUN SLUG CROOK R S D N`. Check `$CROOK` is
   `$HOME/.cargo/bin/crook`; another `crook` earlier on `PATH` would test the wrong build.
3. A driver pane: `herdr workspace create --cwd "$R" --label e2e-driver --no-focus`; its
   `.result.root_pane.pane_id` is `$DRV`. A fresh pane has no `CLAUDECODE`, so crook runs there.

| Need | Run |
|---|---|
| crook on the run's state, from your shell | `bash scripts/e2e/crook.sh "$envf" <args>` (`NO_HERDR=1` hides herdr) |
| crook in the driver pane (needs a terminal) | `herdr pane run $DRV "env XDG_STATE_HOME=$S XDG_DATA_HOME=$D $CROOK <args>"` |
| query the database | `bash scripts/e2e/db.sh "$envf" "<sql>"` |
| type a prompt into a pane | `bash scripts/e2e/say.sh <pane> "<text>"` |
| accept trust prompts | `bash scripts/e2e/trust.sh <pane>`; it waits until the session is up |
| see a pane | `herdr pane read <pane> --source visible`; for scrollback or a dialog below the fold, `--source recent --lines 200` |
| find a pane | `herdr pane list \| jq -r '.result.panes[] \| "\(.pane_id) \(.workspace_id) \(.cwd)"'`, by cwd |
| a session's pid | `claude agents --json \| jq '.[] \| select(.cwd == "<worktree>") \| .pid'` |
| wait | 20–60 s after a prompt, then check; models take that long |

## Steps (with herdr)

Run each, check it, record pass or fail with the evidence, then go on. `O` is the orchestrator pane, `C1` and `C2`
the children's. `db` means `bash scripts/e2e/db.sh "$envf"`.

| # | Do | Pass when |
|---|---|---|
| 1 | `herdr pane run $DRV "env XDG_STATE_HOME=$S XDG_DATA_HOME=$D $CROOK open $SLUG"`. Poll `herdr pane list` (up to 20 s) for the pane whose cwd is `$D/crook/$SLUG`: that is `O`, and its `workspace_id` is `OW`. `trust.sh $O` | `db "select session_id, pid from orchestrator"` has a row; `herdr pane read $O --source visible \| grep -c '⚠ crook: 0 waiting on you · 0 working'` is 1 |
| 2 | `say.sh $O "Spawn two children, 'one' and 'two', in $R"`. `C1`, `C2` are the panes with cwd `$R/.worktrees/$SLUG/one` and `…/two`; `trust.sh` each | `db "select child, branch from worktrees"` has both on `crook/$SLUG/<child>`; `git -C $R worktree list` agrees; `herdr pane read $O --source recent --lines 200 \| grep -c project_add_repo` is 0 |
| 3 | `say.sh $O "Task 'one' with: create hello.txt containing hi and commit it. Task 'two' with: create bye.txt containing bye and commit it."` | `db "select child, taken from inbox"` all 1; `db "select child, state, outcome from status"` both `done`, `ok`; `git -C $R log --oneline crook/$SLUG/one` and `…/two` each have a commit above `init`; `herdr pane read $O --source recent --lines 200 \| grep -c 'Prompt from the crook plugin'` is at least 1 (the done wake-up) |
| 4 | `say.sh $C1 "Use need to ask the user what colour hello.txt should be. Don't guess, don't do anything else until they answer."` | `db "select state, question from status where child = 'one'"` is `parked` with the question; `herdr pane read $O --source visible \| grep -c '⚠ crook: 1 waiting on you'` is 1 |
| 5 | `herdr pane run $DRV "env XDG_STATE_HOME=$S XDG_DATA_HOME=$D $CROOK next"` | `$DRV` prints `$SLUG/one: focused pane $C1`; `herdr workspace get $OW \| jq -r .result.workspace.active_tab_id` equals `herdr pane get $C1 \| jq -r .result.pane.tab_id` |
| 6 | `say.sh $C1 "/help"`, then `herdr pane send-keys $C1 esc`, then the first check; then `say.sh $C1 "red"` and wait | after `/help`: `db "select state from status where child = 'one'"` is still `parked`; after `red`: state is no longer `parked` and `question` is empty |
| 7 | Note `db "select done_seq from status where child = 'two'"`. `say.sh $O "Task 'two' with: run sleep 120 in Bash first, then append *.tmp to a new .gitignore and commit it."`. Once `db "select taken from inbox where child = 'two' order by seq desc limit 1"` is 1: `say.sh $C2 "/clear"` | right after the clear, `db "select current_task from status where child = 'two'"` is still set; within 3 minutes `git -C $R show --name-only --format= crook/$SLUG/two` lists `.gitignore` and `status.done_seq` for two is above the noted value |
| 8 | `say.sh $C1 "First read $N/plan.md with the Read tool. Then run this exact Bash command and show me the output: cat $N/plan.md"` | `db "select tool, hit from asks"` has exactly one row, `Bash`, hit `$N/plan.md` (the `Read` raised none); `C1` shows `Waiting for your answer`: reject it with `herdr pane send-keys $C1 esc` |
| 9 | Kill two: `kill <pid>`, the pid from the "session's pid" helper, cwd checked under `$RUN`. Then `herdr pane run $DRV "env XDG_STATE_HOME=$S XDG_DATA_HOME=$D $CROOK resume $SLUG/two"`; `trust.sh $DRV` | the new pid (same helper) has `--resume <id>` in `ps -o command= -p <pid>`, where `db "select session_id from children where child = 'two'"` gives the id; `herdr pane read $DRV --source visible \| grep -c '⚠ crook: child'` is 1 |
| 10 | `echo x > $R/.worktrees/$SLUG/one/dirty.txt`; `bash scripts/e2e/crook.sh "$envf" stop $SLUG/one; echo $?`; `rm` that file; stop again. Your shell must have `HERDR_ENV=1` (it does inside a herdr pane), or crook will not close panes | first prints exit 1 naming the worktree and `db "select archived from children where child = 'one'"` is still 0; then archived 1, `C1` gone from `herdr pane list`, worktree gone from disk |
| 11 | `bash scripts/e2e/crook.sh "$envf" list`, then `bash scripts/e2e/crook.sh "$envf" stop $SLUG/two` | `list` prints one line for `$SLUG`; the stop prints `the pane it ran in is not one crook opened, so it stays open` and `herdr pane get $DRV` still succeeds |
| 12 | `bash scripts/e2e/teardown.sh "$envf"` | it prints `removed $RUN`, no `LEFT:` line, exit 0 |

## Without herdr

Herdr still drives the panes; only crook must not see it. Use `NO_HERDR=1` with `crook.sh`, and add `-u HERDR_ENV` to
every `env` in a `herdr pane run`. The orchestrator then runs in `$DRV`, children start with `claude --bg`, and you
reach a child by opening a pane with `herdr pane run <pane> "env -u CLAUDECODE claude attach <short id>"`. The short
id is the session id up to its first `-`; the full id is refused.

Before spawning: in a new pane, `herdr pane run <pane> "cd $N; env -u CLAUDECODE claude"`, `trust.sh <pane>`, end that
session, and the same for `$R`. `--bg` refuses a folder nobody has trusted.

Known blocker, as of 2026-11-22: a second `--bg` child can start inside a pre-warmed `claude bg-spare` that keeps the
first child's environment. Right after step 2, for each child take its pid from the "session's pid" helper and run
`ps eww -p <pid> | tr ' ' '\n' | grep '^CROOK_CHILD='`. Each must name its own child. If one names the other, record it
and stop.

## Mistakes that cost runs

| Symptom | Cause |
|---|---|
| A session exits right after start | Enter on a trust prompt whose default is "No, exit". Use `trust.sh` |
| `trust.sh` refuses "could not highlight" | The dialog was still drawing; run it again |
| A prompt typed with `say.sh` never arrives | A dialog is up below the fold, or a `※ recap` line was being drawn. Read `--source recent`; if the prompt is not in the pane, send it again |
| A `claude` you thought was stopped still runs | `pgrep -x claude` misses `claude bg-spare`; use `pgrep -f claude` and check cwd |
| `crook` refuses "user verbs cannot run inside Claude Code" | Run it through `scripts/e2e/crook.sh` or in a herdr pane, never bare from your own shell |
| A step "passed" but the database disagrees | You read the model's reply. Use the check in the Pass column |

## Report

If a model does not do what a step's prompt asks (no `need` call, no Bash command), send the prompt once more. If it
still does not, record the step as "not exercised: the model did not comply", not as a crook failure.

A run leaves trust entries for its `$RUN` paths in your Claude Code config, and each session loads your real
`~/.claude` settings, plugins and hooks (only the XDG dirs are throwaway). Say so in the report.

One line per step: number, pass or fail, and the evidence (the query result, commit or pane line). Then each
failure with what you saw against what the step expects, and whether it is a crook bug, a design gap or the run.
