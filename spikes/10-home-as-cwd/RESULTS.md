# Spike 10: the project home as the orchestrator's cwd

Claude Code 2.1.329, Haiku, run 2026-11-21 from inside a herdr pane. The probe
folder was `/private/tmp/crook-spike10-probe/demo`: new, empty, not a git
repository (`git rev-parse` fails), standing in for `<home>/<slug>`.

Trust state checked first in `~/.claude.json` (`projects` keys): none of the
folder, `/private/tmp/crook-spike10-probe`, `/private/tmp`, `/private`, `/tmp`,
`/`, `$HOME` or `$HOME/projects` had an entry, so no ancestor trusted it. The
probe was made outside every tree the user already runs Claude in, because a
home under a trusted tree would hide the question.

## Results

| Question | Answer |
| --- | --- |
| `claude --bg --model haiku "<prompt>"` in the new folder | Refused before starting: `Workspace not trusted. Run \`claude\` in <dir> once and accept the trust prompt, then retry.` No session was created. |
| Interactive `claude --model haiku` in the same folder | Trust dialog: "Accessing workspace: <dir> ... Is this a project you created or one you trust?" with `No, exit` / `Yes, I trust this folder`. Escape printed "Claude Code didn't start because this folder isn't trusted." |
| Auto memory location | Not measured: the session never started. |
| Hook parent pid is the `claude` process | Not measured: no session started. |
| Cleanup | Pane closed, probe folder removed, no `crook-spike10` entry written to `~/.claude.json`, no background session left. |

## Second run: a trusted tree (controller ruling, 2026-11-21)

The controller ruled the dialog a one-time accept per project, not a blocker:
`crook open` is interactive, and children run in worktrees under the user's
repos. The two items above were then measured with the real `crook open`
(Task 13, debug build) with `home` set in `config.toml` to
`<worktree>/spikes/10-home-as-cwd/run/home`, a folder inside this worktree's
git tree, which is already trusted. A throwaway slug, temp
`XDG_STATE_HOME`/`XDG_DATA_HOME`/`XDG_CONFIG_HOME` and `ANTHROPIC_MODEL=haiku`.
No trust prompt was accepted and `~/.claude.json` was not edited.

| Question | Answer |
| --- | --- |
| Trust dialog for `<home>/spike10` there | None: the session started straight away, so trust is inherited from the git tree above it. This does **not** test a fresh folder outside every trusted tree; that case is the dialog above. |
| Hook's parent pid is the `claude` process | Yes. `orchestrator.pid` written by `crook hook SessionStart` was 40726; `ps -o command= -p 40726` showed `/opt/homebrew/bin/claude -n crook-spike10 --session-id 870137c7-... --permission-mode auto --plugin-dir ...`, the session's own process. After `/exit` and a second `crook open` (which passed `--resume` with the same id) the row held the new claude's pid (46677, also `claude`). |
| Auto memory location | The model's memory write went to `~/.claude/projects/-Users-<user>-projects-me-shepherds-crook/memory/`, the main checkout's project folder, not a folder named for the cwd (the transcript went to `...-shepherds-crook--worktrees-crook-v1-spikes-10-home-as-cwd-run-home-spike10/`). So memory is keyed to the git root's main repository. Because the probe home sat inside a git tree, this is **not** the answer for a non-git folder outside any repo: that case is still unmeasured, and needs the trust accept. |
| Card at session start | Present: the model quoted `What: Spike 10 probe` and the private locations (diary and notes folder). |
| Status line | `crook: orchestrator` (the UI shows it as `⚠ crook: crook: orchestrator`; the doubled prefix is the UI's own source label plus the mod's text). |
| `Read` on the state dir | Denied: `File is in a directory that is denied by your permission settings.` The model first declined to try, citing the orchestrator prompt; asked to test it, it got that error. |
| `crook open` again while live | Printed `crook: the orchestrator for spike10 is already running (pid 46677, session 870137c7-...); not starting a second one`, exit 0. |
| `--permission-mode` | `auto`: the user's `permissions.defaultMode` is `auto`, which `settings::permission_mode` keeps. |
| Cleanup | Both sessions exited, the pane closed, the run folder and its transcripts removed, the probe note written to the user's real auto memory deleted and its line removed from `MEMORY.md` (restored to its one original line). |

## Gate

A trust dialog blocks the session from starting in a new non-git
`<home>/<slug>` until the user accepts it once for that folder, and a `--bg`
start there is refused outright. Every new project gets a new folder, so the
dialog returns once per project. The task is stopped at this point as the brief
requires; product behaviour was not changed. The controller takes it to the user.
