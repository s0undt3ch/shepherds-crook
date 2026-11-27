# Flocks

One dialog for crook flocks, opened from the command palette as "Flocks". It lists the active flocks, starts a
new one, reopens a paused one and closes one. It draws no flock tree: the app's own Spaces and Agents sidebars
show the flock.

## What you see

A dialog 560 px wide and 460 px high, in the look of the shipped `worktree` dialog. The list view has the title
"Flocks" with "Esc" on the right, a first row "+ New flock…", then one row per active flock with its slug, the
date from its folder name and a "running" (accent) or "paused" (dim) badge. The selected flock row shows two
buttons, "Open" and "Close…". A footer reads "↑↓ choose · ↵ open · esc close". The New flock view has a Name
field with a hint, an optional First task textarea, and Back and Create buttons. The close confirm names the
flock, says what closing does, and has Cancel and "Close flock".

## How it behaves

- On open it runs `crook status --json`. While that runs the list shows "…"; if it fails the list shows crook's
  message in the `blocked` colour and a Retry button.
- A flock is "running" when its coordinator terminal (labelled `crook.flock`, not `crook.child`) is live and
  the app sees an agent in it. A live bare shell counts as paused, so Open runs `crook open`. The list follows
  `terminals()` without polling.
- Open on a running flock selects its space. Open on a paused flock runs `crook open <slug>`, then selects the
  space once the app lists it (up to 10 s).
- Create runs `crook new <slug>`, with `--task=<text>` when the task is not empty. While crook runs the fields
  are read-only and Esc is ignored. A failure shows crook's first stderr line
  that is not a warning under the form.
- Close runs `crook close <slug>` after the confirm. The toast lists each
  `warning: kept worktree …` line crook printed on stderr; other warnings are ignored.
- The footer adds "Run `crook setup` to update this dialog." when `mods/crook/.crook-version` differs from the
  binary's version.

## Built from

- `mod.json`: `"place": "none"`, an overlay of kind `dialog`, and one palette entry running `mod.open` with
  `{ "name": "crook" }`.
- `main.js`: one `View`, with ids `crook-root` and `crook-new`. It uses `herdr`'s `exec`, `spaces`, `terminals`,
  `run`, `notify`, `readTextFile`, `appHome`, `home`, `checking`, `props` and `closeOverlay`, and `gpui-base`'s
  `InputState`, `Input`, `TextareaState` and `Textarea`.
- `lib.js`: pure functions, no imports.
- Every crook call is `exec("sh", ["-c", SCRIPT, "crook", ...args], { cwd: home(), timeout: 120000 })`, where
  `SCRIPT` is `exec "${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" "$@"`.
  The arguments are positional parameters, so a name or task never enters the script text.
- The `crook` binary, installed with mise. `crook setup` installs this mod into `mods/crook/`; nothing else
  refreshes it.

## Watch out

- A dialog cannot take props from the palette, so one dialog serves all three actions.
- `exec` does not start a shell, so `CROOK_BIN` only works inside the `sh -c` launcher. Whether the app passes
  `CROOK_BIN` to `exec` at all is not known.
- Slugs are at most 16 characters, lower-case words joined by single dashes, and must not start with `t` and a
  digit.
- `exec` runs during `herdr-app check`; the mod skips every crook call while `checking()` is true.

## Done when

- `herdr-app check` passes on a copy of the folder.
- The palette lists "Flocks" and opens the dialog.
- With crook installed, the list shows the active flocks, and Create, Open and Close each run the matching crook
  command and show its error text on failure.
