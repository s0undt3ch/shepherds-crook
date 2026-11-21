# crook

crook runs one orchestrator Claude Code session per project. The orchestrator plans work and starts child sessions in
other repositories, each in its own git worktree. A child's questions come to you, and crook keeps private notes out of
public places. It is one Rust binary, plus a Claude Code mod that the binary carries and unpacks at launch.

Design: [specs/2026-11-19-crook-design.md](specs/2026-11-19-crook-design.md).

## Install

crook is released through [packslip](https://github.com/jdx/packslip) and installed with [mise](https://mise.jdx.dev),
which verifies the release's Sigstore bundle:

```sh
mise use -g "packslip:github.com/s0undt3ch/shepherds-crook@<version>"
```

Use a version from the [releases page](https://github.com/s0undt3ch/shepherds-crook/releases). Shell completions come
with the package. To print one by hand: `crook completion print <bash|zsh|fish>`.

## Use

```sh
crook init myproject
crook project myproject add-repo /path/to/repo
crook project myproject add-notes /path/to/notes
crook open myproject
```

`crook open` starts the orchestrator in the project's home. Ask it to spawn children. Each child works in
`<repo>/.worktrees/<slug>/<child>` on branch `crook/<slug>/<child>`. When a child calls `need`, it parks until you answer
in its session. `crook next` takes you to the child that has waited longest.

The `crook-protocol` skill and the launch prompts tell the sessions their rules. The skill is in
[plugin/skills/crook-protocol/SKILL.md](plugin/skills/crook-protocol/SKILL.md).

## Commands

These are for you, run outside any session. A session's model cannot call them.

| Command | Does |
|---|---|
| `crook init <slug>` | Creates the project record, its home and its diary |
| `crook project <slug> <command>` | Edits the record: `add-repo`, `add-notes`, `add-private`, `add-allow`, `add-link`, `add-ticket`, `remove`, `set-what`, `set-why`, `set-status` |
| `crook open <slug>` | Starts the orchestrator, or attaches to the live one |
| `crook resume <child>` | Restarts a child's session with crook's flags |
| `crook unpark <child>` | Lifts a child's park by hand |
| `crook stop <child>` | Stops a child, archives it and removes its worktrees; refused if one has uncommitted changes |
| `crook wipe <slug> [--with-diary]` | Removes a finished project's worktrees, state and what crook made in its home; keeps the diary, the branches and anything else in the home, which it lists |
| `crook list` | One line per project |
| `crook status` | Live children across projects |
| `crook next` | Jumps to the child that has waited longest on you, across every project |
| `crook completion print <shell>` | Prints a shell completion script |

`resume`, `unpark` and `stop` take `<child>` or `<slug>/<child>`. A bare `<child>` that more than one project has is
refused, and the error lists the `<slug>/<child>` forms.

## herdr

herdr is optional. When crook runs inside a herdr pane:

- each project gets one workspace, `crook-<slug>`
- the orchestrator's tab is labelled `🐕`
- each child gets one tab, named after the child
- `need` and `done` raise a herdr notification
- `crook next` focuses the waiting child's pane

Without herdr, children start with `claude --bg`, and `crook next` runs `claude attach`.

To use plain text for the orchestrator's tab, set it in `~/.config/crook/config.toml`:

```toml
[herdr]
orchestrator_label = "o10r"
```

## Threat model

Every crook session runs as your OS user, so crook is not a security boundary. It does three narrower jobs: keep private
material out of public places, keep projects apart, and keep questions answered by you.

- **Natural tool calls** by a mistaken or prompt-injected model: private references are closed. A call that names a
  private path asks you first. Anything else is your own Claude Code configuration.
- **Evasion** by a model trying to get round the hooks: best effort. Paraphrase is not caught.
- **A hostile local process**: out of scope. That needs a separate OS user or a container per project.

The check looks for private paths in a tool call's input, not for the text of private files. Private paths are a
project's notes folders, its diary, its `add-private` entries, and the `private` list in the config file.

## Known gaps

The design lists them in full. The ones to know first:

- **Upgrading crook with mise** removes the binary that live sessions' absolute `CROOK_BIN` names. Every tool call in
  those sessions blocks until `crook resume <child>` or `crook open <slug>` relaunches it.
- **Quoted text is not checked.** The privacy check sees private paths, not quoted text, so a note pasted into a PR body
  does not ask. The protocol tells sessions not to do it.
- **Paraphrase and encoding** are not caught.
- **Some Bash spellings slip past the path check.** These include a `cd` inside a compound command, a bare glob, a
  backslash spelling and a hand-built expansion.
- **`git worktree remove` deletes ignored files** in the worktree, such as `.env` and build output. `crook stop` and
  `crook wipe` use it.
- **A project home that is not trusted yet** makes `claude --bg` refuse with `Workspace not trusted`, so spawning
  without herdr fails. Open the project once and accept the trust prompt first.
- **`--add-dir` trust is per session.**
- **Other mods.** A higher-tier mod can pass events past crook's hooks. `crook open` names such mods.

## Credits

crook borrows four ideas. It copies no code from any of them.

- The `need` relay, where a child flags a question and the orchestrator routes it to you, comes from
  [herdr-quartermaster](https://github.com/travisjeffery/herdr-quartermaster).
- Park and resume, where a waiting child parks and resumes on your answer, comes from
  [bermuda](https://github.com/bon5co/bermuda).
- In-session delivery by a mod, which polls a store and delivers with a prompt, comes from claude-snooze, a private
  Claude Code plugin.
- A record kept current, where a write fails if the record changed since it was read, comes from
  [loam](https://github.com/GregorMcC/loam).

## Licence

Apache-2.0. See [LICENSE](LICENSE).
