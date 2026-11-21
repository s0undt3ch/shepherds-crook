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

## Gate

A trust dialog blocks the session from starting in a new non-git
`<home>/<slug>` until the user accepts it once for that folder, and a `--bg`
start there is refused outright. Every new project gets a new folder, so the
dialog returns once per project. The task is stopped at this point as the brief
requires; product behaviour was not changed. The controller takes it to the user.
