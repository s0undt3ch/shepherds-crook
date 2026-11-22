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

- Accept a trust or add-dir prompt only through `trust.sh`, which refuses any path outside this run's `$RUN`. On
  `REFUSED`: leave the run as it is and ask the user; tear down after they answer.
- Permission prompts in the run's sessions: approve or reject with `keys.sh` as the step needs. Reject when in doubt.
- Touch only the run: its directory, its herdr workspaces, its processes (check a pid's cwd is under `$RUN` before
  killing it). Never the user's real crook state, workspaces, sessions or installed `crook`. `say.sh` and `trust.sh`
  refuse a pane whose cwd is outside the run, and every script refuses an env file that is not a run's.
- End every run with `teardown.sh`, pass or fail.

## Setup

1. `cargo build --release`. The run uses this build, `target/release/crook`, with its mod embedded; your installed
   `crook` is left alone.
2. `bash scripts/e2e/setup.sh` prints the run's env file. Shell variables do not survive between Bash calls, so start
   every later call with `source <that file>`. It sets `RUN ENVF E2E SLUG CROOK R S D N`.
3. A driver pane: `herdr workspace create --cwd "$R" --label e2e-driver --no-focus`; its
   `.result.root_pane.pane_id` is the driver. A fresh pane has no `CLAUDECODE`, so crook runs there.
4. Keep every pane id you find in the env file so later calls have it: `echo "DRV=<id>" >> "$ENVF"`, and the same for
   `O`, `OW`, `C1` and `C2` below.

Below, `say.sh`, `keys.sh`, `trust.sh`, `run_in.sh`, `kill_in_run.sh` and `db` mean `bash $E2E/<script> $ENVF`
(`db` is `db.sh`). Use them, never raw `herdr pane send-keys`, `herdr pane run` or `kill`: each refuses a pane or pid
outside the run.

| Need | Run |
|---|---|
| crook on the run's state, from your shell | `bash $E2E/crook.sh $ENVF <args>` (`NO_HERDR=1` hides herdr) |
| crook in the driver pane (needs a terminal) | `run_in.sh $DRV <args>` |
| keys into a pane (Esc, an answer) | `keys.sh <pane> <key>...` |
| end a process | `kill_in_run.sh <pid>` |
| query the database | `db "<sql>"` |
| type a prompt into a pane | `say.sh <pane> "<text>"` |
| accept trust prompts | `trust.sh <pane>`; it waits until the session is up |
| see a pane | `herdr pane read <pane> --source visible`; for scrollback or a dialog below the fold, `--source recent --lines 200` |
| find a pane | `herdr pane list \| jq -r '.result.panes[] \| "\(.pane_id) \(.workspace_id) \(.cwd)"'`, by cwd |
| a session's pid | `claude agents --json \| jq '.[] \| select(.cwd == "<worktree>") \| .pid'` |
| wait | 20–60 s after a prompt, then check; models take that long |

## Steps (with herdr)

Run each, check it, record pass or fail with the evidence, then go on. `O` is the orchestrator pane, `C1` and `C2`
the children's.

| # | Do | Pass when |
|---|---|---|
| 1 | `run_in.sh $DRV open $SLUG`. Poll `herdr pane list` (up to 20 s) for the pane whose cwd is `$D/crook/$SLUG`: that is `O`, and its `workspace_id` is `OW`. `trust.sh $O` | `db "select session_id, pid from orchestrator"` has a row; `herdr pane read $O --source visible \| grep -c '⚠ crook: 0 waiting on you · 0 working'` is 1 |
| 2 | `say.sh $O "Spawn two children, 'one' and 'two', in $R"`. `C1`, `C2` are the panes with cwd `$R/.worktrees/$SLUG/one` and `…/two`; `trust.sh` each | `db "select child, branch from worktrees"` has both on `crook/$SLUG/<child>`; `git -C $R worktree list` agrees; `herdr pane read $O --source recent --lines 200 \| grep -c project_add_repo` is 0 |
| 3 | `say.sh $O "Task 'one' with: create hello.txt containing hi and commit it. Task 'two' with: create bye.txt containing bye and commit it."` | `db "select child, taken from inbox"` all 1; `db "select child, state, outcome from status"` both `done`, `ok`; `git -C $R log --oneline crook/$SLUG/one` and `…/two` each have a commit above `init`; `herdr pane read $O --source recent --lines 200 \| grep -c 'Prompt from the crook plugin'` is at least 1 (the done wake-up) |
| 4 | `say.sh $C1 "Use need to ask the user what colour hello.txt should be. Don't guess, don't do anything else until they answer."` | `db "select state, question from status where child = 'one'"` is `parked` with the question; `herdr pane read $O --source visible \| grep -c '⚠ crook: 1 waiting on you'` is 1 |
| 5 | `run_in.sh $DRV next` | `$DRV` prints `$SLUG/one: focused pane $C1`; `herdr workspace get $OW \| jq -r .result.workspace.active_tab_id` equals `herdr pane get $C1 \| jq -r .result.pane.tab_id` |
| 6 | `say.sh $C1 "/help"`, then `keys.sh $C1 esc`, then the first check; then `say.sh $C1 "red"` and wait | after `/help`: `db "select state from status where child = 'one'"` is still `parked`; after `red`: state is no longer `parked` and `question` is empty |
| 7 | Note `db "select done_seq from status where child = 'two'"`. `say.sh $O "Task 'two' with: run sleep 120 in Bash first, then append *.tmp to a new .gitignore and commit it."`. Once `db "select taken from inbox where child = 'two' order by seq desc limit 1"` is 1: `say.sh $C2 "/clear"` | right after the clear, `db "select current_task from status where child = 'two'"` is still set; within 3 minutes `git -C $R show --name-only --format= crook/$SLUG/two` lists `.gitignore` and `status.done_seq` for two is above the noted value |
| 8 | `say.sh $C1 "First read $N/plan.md with the Read tool. Then run this exact Bash command and show me the output: cat $N/plan.md"` | `db "select tool, hit from asks"` has exactly one row, `Bash`, hit `$N/plan.md` (the `Read` raised none); `C1` shows `Waiting for your answer`: reject it with `keys.sh $C1 esc` |
| 9 | Kill two: `kill_in_run.sh <pid>`, the pid from the "session's pid" helper. Then `run_in.sh $DRV resume $SLUG/two`; `trust.sh $DRV` | the new pid (same helper) has `--resume <id>` in `ps -o command= -p <pid>`, where `db "select session_id from children where child = 'two'"` gives the id; `herdr pane read $DRV --source visible \| grep -c '⚠ crook: child'` is 1 |
| 10 | `echo x > $R/.worktrees/$SLUG/one/dirty.txt`; `bash $E2E/crook.sh $ENVF stop $SLUG/one; echo $?`; `rm` that file; stop again. Your shell must have `HERDR_ENV=1` (it does inside a herdr pane), or crook will not close panes | first prints exit 1 naming the worktree and `db "select archived from children where child = 'one'"` is still 0; then archived 1, `C1` gone from `herdr pane list`, worktree gone from disk |
| 11 | `bash $E2E/crook.sh $ENVF list`, then `bash $E2E/crook.sh $ENVF stop $SLUG/two` | `list` prints one line for `$SLUG`; the stop prints `the pane it ran in is not one crook opened, so it stays open` and `herdr pane get $DRV` still succeeds |
| 12 | `bash $E2E/teardown.sh $ENVF` | it prints `removed $RUN`, no `LEFT:` line, exit 0 |

## Without herdr

Herdr still drives the panes; only crook must not see it. Put `NO_HERDR=1` before every `crook.sh`, in your shell and
before `run_in.sh` (`NO_HERDR=1 bash $E2E/run_in.sh $ENVF $DRV open $SLUG`). The orchestrator then
runs in the driver, children start with `claude --bg`, and you reach a child in a new pane made with
`herdr workspace create --cwd $R`, running `env -u CLAUDECODE claude attach <short id>` there. The short id is the
session id up to its first `-`; the full id is refused.

Before spawning: in a new pane made with `--cwd $N`, run `env -u CLAUDECODE claude`, `trust.sh <pane>`, end that
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
| `crook` refuses "user verbs cannot run inside Claude Code" | Run it through `crook.sh` or in a herdr pane, never bare from your own shell |
| A step "passed" but the database disagrees | You read the model's reply. Use the check in the Pass column |

## Report

If a model does not do what a step's prompt asks (no `need` call, no Bash command), send the prompt once more. If it
still does not, record the step as "not exercised: the model did not comply", not as a crook failure.

A run leaves trust entries for its `$RUN` paths in your Claude Code config, and each session loads your real
`~/.claude` settings, plugins and hooks: crook's state, data and config dirs are throwaway, Claude Code's are not. Say
so in the report.

One line per step: number, pass or fail, and the evidence (the query result, commit or pane line). Then each
failure with what you saw against what the step expects, and whether it is a crook bug, a design gap or the run.
