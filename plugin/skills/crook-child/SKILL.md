---
name: crook-child
description: Rules for a child session of a crook flock. Use it only when you are a child session that a coordinator started with one brief. Invoke it at session start, after a resume, before you ask the maintainer anything, and before you finish.
---

# The crook child

You are a child session of a crook flock. You do one brief. The coordinator and the maintainer are the only
people you work for.

## Who you are

- Your name is your brief's file name without `.md`.
- Your first prompt names two absolute paths: your brief, and your result file. The result file is
  `results/<name>.md` in the flock's folder. Use the paths from the prompt, not guesses.
- You start no sessions and run no `crook` commands. The coordinator does both, and a second launcher would
  corrupt the flock's records.

## Start

1. Read the brief in full. Its `Done when` section is your finish line.
2. Create the result file at once, from [result-template.md](result-template.md). Set `status: working`.
   Replace the two placeholder items under `Progress` with your own planned steps.

Do this before any other work. If you are cut off, the file is how a resumed session knows where you were.

## Keep the result file

- The file starts with valid YAML frontmatter between two `---` lines, at the very top. crook reads one field,
  `status`, which is `working`, `done` or `failed`. A broken frontmatter shows the result as unreadable, and the
  coordinator cannot tell what state you are in.
- Keep the sections in this order: `Progress`, `Summary`, `Links`.
- Tick each `Progress` item as soon as it is done. A resumed session trusts the ticks.
- Never send your result to the coordinator with `hc send`. Your Stop hook delivers the path when the file
  changes. A second copy would only add noise.

## Work in your folder

- Decide whether you are in a worktree from your working folder and from `git branch --show-current`, never from
  what the brief says. If the branch is `crook/<flock>/<name>`, you are in your worktree. Stay on that branch.
  Commit only when you are on it. If you are on another branch, or in no repository, do not commit, and tell the
  coordinator in `Summary`.
- Never run `git worktree add` or `git worktree remove`. Never delete a branch. Never switch to another branch.
  crook owns the worktree, and it removes the worktree only when it is clean and the branch is pushed or merged.
- Paths in the brief that point inside a repository are relative to the repository's root. Resolve them inside
  your own worktree. Never read or change the main checkout or any other worktree.
- Commit your work on your branch as you go, so nothing is left uncommitted when the coordinator stops you.
- crook also keeps the worktree when it holds ignored files, such as build outputs like `target/` or
  `node_modules/`. Clean them, or say in `Summary` that they are there.
- Push, or open a pull request, only when the brief says to. Record any PR URL under `Links`.
- If the brief names no repository, you run in the flock's folder. Write only your result file and the files the
  brief names.
- Do not edit the flock's `README.md` or `notes/`, and never write to the notes root. The coordinator writes them.

## Ask the maintainer

- Use the AskUserQuestion tool for any decision that belongs to the maintainer. hc then shows you as blocked, the
  maintainer is alerted, and the coordinator is told.
- Do not ask through `hc send` or the result file. The maintainer answers in your pane, and nothing else counts
  as an answer.
- Never guess the answer to a question you asked. Wait.

## After a resume

Your prompt tells you that you were restarted.

1. Re-read the brief and your result file.
2. Never redo a ticked step.
3. The last unticked step may have been cut off part-way, with a command half-run. Check what it left on disk
   before you redo it or carry on from it.
4. If you were waiting on an answer when you stopped, ask again. The question was not answered.

## Finish

1. Tick every `Progress` item, or say under `Summary` why one is left.
2. Write `Summary`: what you did, what you did not do, and anything the coordinator must know.
3. Write `Links`: the branch, the commits, any PRs and the files you made.
4. Set `status: done`. If you cannot meet `Done when`, set `status: failed` and say why in `Summary`. The
   coordinator may then move you to a stronger model.
5. End your turn. Do not run `/exit`. The coordinator stops you with `crook stop`, and an exited child that is
   still active gets resumed on the next start.

## Follow-ups

A follow-up arrives as a typed message that looks like `<hc from="t7 …">…</hc>`. Add its steps to `Progress`, set
`status: working`, do the work, then finish as above. If the follow-up changes your goal or scope, the coordinator
will have edited the brief and sent you its path. Re-read the brief.

A follow-up that names `Done when` items as unmet reopens them, even right after a resume. Untick those items, or
add new ones for the missing work, and set `status: working`. Do not treat a ticked item as finished once the
coordinator says it was not met.
