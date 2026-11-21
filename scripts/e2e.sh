#!/usr/bin/env bash
# Manual end-to-end checklist for crook. It starts real Claude Code sessions, so it needs a
# logged-in `claude` and your attention. It prints each step and what to look for, then waits.
# Run it once without herdr, then once from inside a herdr pane. CI does not run it.
set -euo pipefail

crook="${CROOK:-crook}"
slug="${SLUG:-e2e}"
repo="${REPO:-$(mktemp -d "${TMPDIR:-/tmp}/crook-e2e-repo.XXXXXX")}"
export XDG_STATE_HOME="${XDG_STATE_HOME:-$(mktemp -d "${TMPDIR:-/tmp}/crook-e2e-state.XXXXXX")}"
export XDG_DATA_HOME="${XDG_DATA_HOME:-$(mktemp -d "${TMPDIR:-/tmp}/crook-e2e-data.XXXXXX")}"

command -v "$crook" >/dev/null || { echo "crook not found; set CROOK=/path/to/crook" >&2; exit 1; }
command -v claude >/dev/null || { echo "claude not found" >&2; exit 1; }

if [ -n "${HERDR_ENV:-}" ]; then mode="with herdr"; else mode="without herdr"; fi
echo "crook e2e checklist, $mode"
echo "  repo:  $repo"
echo "  state: $XDG_STATE_HOME"
echo "  data:  $XDG_DATA_HOME"

if [ ! -d "$repo/.git" ]; then
  git -C "$repo" init -q
  git -C "$repo" commit -q --allow-empty -m "init"
fi

n=0
step() {
  n=$((n + 1))
  printf '\n== Step %d: %s\n' "$n" "$1"
  printf 'Look for: %s\n' "$2"
  if [ -n "${3:-}" ]; then printf 'Run:      %s\n' "$3"; fi
  read -r -p "Press Enter when done (Ctrl-C to stop) " _
}

"$crook" init "$slug"
"$crook" project "$slug" add-repo "$repo"

step "Open the orchestrator" \
  "a Claude Code session named crook-$slug, in the project home; with herdr, a crook-$slug workspace with a dog-labelled tab" \
  "$crook open $slug   (in another terminal or pane; accept the trust prompt once)"
step "Spawn two children" \
  "ask the orchestrator to spawn children 'one' and 'two' in $repo; each gets a worktree under $repo/.worktrees/$slug/ on branch crook/$slug/<child>; with herdr, a tab each" \
  "git -C $repo worktree list"
step "Task a child" \
  "ask the orchestrator to task 'one' with a small job; the child receives 'crook: you have tasks waiting', calls take_task and starts" ""
step "A child asks: need" \
  "tell 'one' to ask you a question with need; it ends its turn; the orchestrator's status line shows 1 waiting on you; with herdr, a notification" ""
step "Jump to the waiting child" \
  "crook next focuses the waiting child's pane, or runs claude attach without herdr" \
  "$crook next"
step "Answer in the child" \
  "type your answer in the child's session; it continues and the park lifts. A slash command typed first must not lift it" ""
step "Finish: done" \
  "'one' calls done(ok); the orchestrator wakes with 'crook: children are done. Call status().' and reports it" ""
step "Clear a child" \
  "run /clear in 'two' while it has a current task; the project card returns with the task" ""
step "Privacy check" \
  "ask a child to run a Bash command that names a path inside a notes folder; it asks you. Read of the same note must not ask" \
  "$crook project $slug add-notes <a notes folder>   (before spawning)"
step "Resume after a kill" \
  "kill a child's claude process, then resume it; it restarts with the same session id" \
  "$crook resume $slug/two"
step "Stop a child" \
  "the child is archived and its worktree removed; a worktree with uncommitted changes is refused, naming it" \
  "$crook stop $slug/one"
step "List" \
  "one line for $slug with the right counts" \
  "$crook list"
step "Clean up" \
  "stop the remaining children, then wipe the project; branches stay and are listed" \
  "$crook stop $slug/two && $crook wipe $slug"

echo
echo "Checklist finished. Throwaway directories: $repo $XDG_STATE_HOME $XDG_DATA_HOME"
