# crook: design

Status: draft 8.7, 2026-11-20. After seventeen adversarial reviews, two builder
checks and live tests. Draft 7 moved wake-ups, parking and the session verbs
into a crook mod. Draft 8 dropped the egress firewall at the user's call,
kept one privacy check, and gave every child a project card. Drafts 8.1 to
8.4 answer reviews 7 to 10. Review 10 signed it off as ready to plan from,
once its two last findings were applied. **Approved by the user on
2026-11-20.** Spikes 1–6 done (see Spikes). Draft 8.5 adds two user asks:
project-wide memory and keeping the card current. Draft 8.6 moves crook's
state into one SQLite database per project, and adds `crook list`, at the user's ask.
Reviews 11 and 12 tightened it, and review 12 signed it off: a `gen` file for the watcher, `BEGIN IMMEDIATE`, a
verb table for writers, entry-point gates, the index in its own file, and
no card refresh on machine-started turns. Draft 8.7 (2026-11-20) records
the user's build decisions (Rust, one binary, mise and packslip
distribution, no Bash exemption) and answers reviews 13 to 17: the hook
fails closed, path detection catches embedded paths and bare names in a
private working directory, and the CLI addresses children by
`<slug>/<child>` when needed.

## Intent

Big pieces of work span several repos. Today the user runs one Claude Code
session per repo and carries context between them by hand.

crook lets one **orchestrator** session per project start **child** sessions
in other repos, track them, give them tasks and route their questions to the
user. It should feel like Claude Code subagents, but each child is a full
session in its own repo, and the user can jump into any child to watch, steer
or answer it.

crook is a **single-developer local tool**. It is open source, and anyone with
`claude` can run it against any repos.

## The idea that holds it together

- **crook sessions are ordinary Claude Code sessions.** The user's own config
  and permission mode govern what they may do, publishing and steering
  included, exactly as in any other session. crook doesn't second-guess it.
- **crook adds two things the user's config can't know:**
  - **the project**: every session gets a card saying what the project is,
    where it stands and who its children are
  - **where private material lives**: anything from those locations leaving
    through a tool call raises a prompt
- **The user answers a child's question by typing in that child.** The crook
  mod tells typing apart from every machine source by the prompt's origin.
- **Sessions never message each other.** They coordinate through a
  per-project database that only crook writes. Each session's crook mod watches its own inbox and
  submits a fixed-text prompt when there's work. Nothing types into a
  terminal.

## Threat model

Every crook session runs as the same OS user, so crook is not a security
boundary. crook's job is narrower than a firewall: keep private material out
of public places, keep projects apart, and keep questions answered by the
user.

| Tier | Example | crook's stance |
|---|---|---|
| **A. Natural tool calls** by a mistaken or prompt-injected model | a PR body quoting a vault note, written to a file and passed with `--body-file`. A Linear comment with a local path. A child calling `need` as a sibling. | **Private references are closed:** the call that carries them out asks. **Everything else is the user's config**, as in any session. |
| **B. Evasion** by a model trying to get round the hooks | paraphrasing a private note, encoding it, a script that reads and posts it | **Best-effort.** Paraphrase is not caught. The gaps are listed. |
| **C. A hostile local process** | malware running as the user | **Out of scope.** This needs a separate OS user or a container per project. |

Reading another project's files is unwanted because it contaminates context,
not because it's secret, so it's handled at tier A only.

## Goals

- One orchestrator per project, several projects at once, without them
  contaminating each other.
- The orchestrator can start, list, task and stop children, and route their
  questions to the user.
- The user can open a child's live session and get back to the orchestrator.
- Every session knows the project it works for and where private material
  lives.
- Interactive first. Unattended runs later.
- Relationships survive restarts, with the hooks intact.
- Low token cost. Checking on N children never means reading N transcripts.
- Each project holds its own record, as [loam](https://github.com/GregorMcC/loam) does: what and why, where it
  stands, repos, links, tickets, private locations, worktrees. It's
  independent of loam.

## Non-goals

- Policing publishing or steering beyond private references. The user's
  config and permission mode do that, as in any session.
- Sharing anything between developers. A future `crook export` might seed
  another developer with a project's record.
- Replacing herdr or writing a terminal multiplexer.
- Automating the PR lifecycle from open to merge.
- Cross-machine orchestration.
- Tier C protection.

## Anti-goals

These would make crook a failure even if it works:

- A crook session sends a private reference anywhere without the user being
  asked first (tier A).
- Anything other than the user's typing resolves a child's question.
- crook routes one project's content into another project's sessions.
- crook changes Claude Code sessions it didn't start.
- crook gets in the way of what the user's config already allows, beyond the
  privacy check.
- Copying code from herdr-quartermaster, bermuda, claude-snooze or loam.
  Ideas only, with credit.

## Constraints

- **Public vs private.**
  - Public means things already public: tracker tickets, PRs, doc URLs.
  - Private means anything under a declared private location: the project's
    notes folders, the project's `private` list, and the user-wide `private`
    list (for example the vault). Sessions may read and use it freely.
- **Full config.** Sessions get the user's full Claude Code config (plugins,
  skills, MCP servers, allow rules, permission mode), plus crook.
- **Investigations.** Children may read arbitrary websites without prompts,
  and they get the project's notes folders.
- **Permission mode.** Sessions run in the user's default mode, except
  `bypassPermissions` and `plan`, which become `manual`. A hook `ask` still
  prompts in `auto` mode, even over an explicit allow rule (tested). In
  `dontAsk` mode the ask becomes a silent deny (`permissions.md:80`), which
  fails safe.
- **Other mods can approve past the hook.** Without managed settings or a
  Team or Enterprise login, "the mod can approve a call that a `PreToolUse`
  hook blocked" and can skip a hook's ask (`permissions.md:659-661`).
  `crook open` lists installed mods that hook `tool.check` or
  `prompt.submit`, and says so.
- **Worktrees** live under `<repo>/.worktrees/<name>`.
- **herdr is optional.** crook counts herdr as present when it runs inside a
  herdr pane (`HERDR_ENV=1`); herdr installed but unused counts as absent.
  With herdr, each child gets a pane and crook raises
  herdr notifications. Without it, children start with `claude --bg`, and
  `crook next` runs `claude attach <id>`. Tested: an attached session shows
  the pending permission prompt, and text typed through `claude attach`
  reaches the mod as `composer`.
- **Mods must be allowed.** An organization can switch off a person's mods
  (`allowManagedModsOnly`) or a user-tier plugin's hooks
  (`allowManagedHooksOnly`). `crook open` refuses to start when either is
  set, because parking and wake-ups would silently stop.
- **Claude Code versions.** 2.1.329 is the tested version. The floor is
  2.1.318: mods arrived in 2.1.287, and from 2.1.318 "what a mod attaches at
  `prompt.submit` [stays] in the user's turn" (the changelog). `crook open`
  refuses older versions and warns on untested newer ones.

## Approaches considered

### Packaging

| Approach | Verdict |
|---|---|
| A marketplace plugin plus a herdr plugin | Dropped. It's loaded everywhere, and herdr plugin actions take no arguments. |
| Our own multiplexer binary | Dropped. Rebuilds herdr and Claude Code. |
| A `crook` launcher, gate hooks in generated `--settings`, and a plugin for skills (draft 6) | Superseded. It needed gate-injected `--as` identity, and herdr for wake-ups. |
| Everything as mods, the privacy check included | Not yet. In a test, a higher-tier plugin passed `tool.check` past crook's mod hook 21 times in one short session ("its tool.check hook was skipped"). The real call still asked, but it's unclear which dispatches skip. |
| **A `crook` CLI for the user, a crook mod loaded with `--plugin-dir` in every crook session, and one settings hook** | **Chosen** |

### How answers reach a child

| Approach | Verdict |
|---|---|
| The orchestrator relays the answer | Dropped. The model could invent it. |
| A hook records `answer N:` from the user's prompts in the orchestrator | Dropped. Settings hooks get scheduled prompts and herdr-typed text bare, exactly like typing (tested). |
| **The user types the answer in the child** | **Chosen.** `crook next` jumps to the oldest child waiting on the user. The mod's `prompt.submit` hook sees `origin.kind: "composer"` for typing, in a pane or through `claude attach`, and a different kind for every machine source. |

### How sessions coordinate and wake

| Approach | Verdict |
|---|---|
| `SendMessage` trusting any sender | Dropped |
| A per-child token, checked by the model or by a hook | Dropped. A hook can't strip it (`hooks.md:1230`). Blocking and re-injecting loses the message (tested). |
| Fixed-grammar `SendMessage` nudges, filtered on both ends | Dropped in review 4. Deadlocked with parking. |
| `herdr agent prompt` typing `crook: inbox <seq>` (draft 6) | Superseded. Made herdr mandatory, and its text is indistinguishable from the user's typing. |
| An `asyncRewake` waiter armed by a `Stop` hook | Dropped. Its timeout is enforced (`hooks.md:4201`), and `Stop` doesn't fire after an interrupt, so waiters die. |
| **The crook mod polls its own inbox and calls `$.prompt.submit` with fixed text** | **Chosen.** Tested: wakes an idle session, in a pane and in `--bg`, is held while a permission dialog is up and delivered after it, survives an interrupt, and reaches the model labelled "The crook plugin sent a message". Inbound messages stay refused. |

### Caller identity

| Approach | Verdict |
|---|---|
| Gate-injected `--as` on a Bash `crook` command (draft 6) | Superseded. Needed a Monitor deny, a `CLAUDECODE` check and an `auto`-mode caveat. |
| **Session verbs are mod tools. The mod reads its identity from `CROOK_PROJECT`, `CROOK_ROLE` and `CROOK_CHILD`, set by the launcher in the session's environment** | **Chosen.** Tested: the tool recorded `proj/child/c1` from the environment, and the model had no input to change it. |

### Publishing and steering

| Approach | Verdict |
|---|---|
| A denylist | Dropped |
| A network sandbox | Dropped. It prompts on every site visited and breaks dev tools. |
| An egress firewall: `ask` on every push, `gh` write, MCP write and steering command, payload first, SHAs pinned (drafts 3–7) | **Dropped by the user, 2026-11-20.** The user is responsible for every session's config and steering, as in ordinary sessions. It was also the heaviest, most fragile part of the design. |
| A privacy check on a named list of publishing tools (`gh`, Linear, Notion, Slack) | Dropped. A tool adopted next year would slip past. |
| A privacy check on every tool except the local file tools (draft 8) | Fixed in review 7. Exempting `Write` and `Edit` let private text into a repo file, then out through a commit or `--body-file`. |
| **A privacy check on every tool call's input, including file writes outside private locations, asking only when it finds a private reference** | **Chosen.** It names the one thing to protect, not the tools, so new tools are covered without changes. |

## Design

### Pieces

One repo holds:

- **`crook`**, one Rust binary with three entry points, each with its own
  gate:
  - **user verbs** (the CLI table below): refused when `CLAUDECODE=1` (set in
    Claude Code's tool shells, checked) or any `CROOK_ROLE` is set, so a
    session's model can't call them.
  - **mod verbs**, run by the mod through `$.process.run`: need
    `CROOK_ROLE`, refused when `CLAUDECODE=1`, and take the slug only from
    `CROOK_PROJECT`. They refuse user verbs and `list`. The model's Bash
    shell inherits `CROOK_*` but also has `CLAUDECODE=1`; unsetting it by
    hand is the tier B gap under Known gaps.
  - **the hook**, `crook hook <event>`: routed before argument parsing, so
    it can never exit with a usage error. It takes the event from stdin JSON
    and the identity from `CROOK_*`.
  Whether the mod's host process carries `CLAUDECODE` is not checked (spike
  9). If it does, the mod passes `env: { CLAUDECODE: "" }` on its own runs
  (`claude-code.d.ts:10251-10253`), the same strength as today. A user
  verb's refusal names the variable that caused it, so a pane shell left
  with `CROOK_*` set is easy to spot.
- **`plugin/`**, embedded in the binary and unpacked at launch into
  `${XDG_DATA_HOME:-~/.local/share}/crook-plugin/<version>-<hash>/`, outside
  the data dir, then loaded with `--plugin-dir` in every crook session:
  - **the crook mod** (`hooks/register.ts`): the session tools, the inbox
    watcher, the park filter and the orchestrator's status line
  - a small skill with the protocol text
- **The settings hook**, `crook hook <event>` wired in through generated
  `--settings` in exec form, with `onFailure: "block"` (`hooks.md:990`):
  - `PreToolUse` with matcher `*`: the privacy check and the park denial
  - `SessionStart`: records the current session id and injects the project
    card
  - The hook fails closed. It only runs in crook sessions, so a missing
    `CROOK_PROJECT`, a missing database or a schema it can't read means a
    broken session: it asks on every call except `Read`, `Glob` and `Grep`,
    with `crook: hook error: run crook open <slug>`.

### The project card

Every crook session gets a short card built from the project record, around 30
lines. It holds only facts that change slowly:

- what and why
- where it stands
- public links and tickets
- the child's own brief (children only)
- every child in the project, one line each: id, repo, goal. An infra child
  knows what the feature child is doing.
- the private locations, with the rule: "use freely for work; anything from
  these leaving through a tool call raises a prompt; never reference them in
  public without asking"
- for a parked child, its open question
- the child's current task: the last one taken with no `done` after it

No sibling transcripts, live states or orchestrator history go in it.

Delivery:

- The `SessionStart` hook adds it as context on `startup`, `resume`, `clear`
  and `compact` (`hooks.md:1293-1299`), so it survives a `/clear` and
  compaction.
- The `status()` tool returns the current card plus every child's live state.
- When the project record changes, the mod's next wake-up says "the project card
  changed: call status()". The mod can't attach context to its own prompt
  (`PromptSubmitArgs` omits `context`, `claude-code.d.ts:11565`), so the
  model fetches it.

**Keeping the card current.** A card nobody checks goes stale. This copies
loam's two habits, ideas only: its `set_where_it_stands` tool tells the
model "Call it when the work moves on", and a landed run's ship notes become
the new Where it stands, rewritten to "keep what is still true, and move it
on by this change" (strings in the loam v0.4.8 binary).

- Only the orchestrator writes the card, through `project_set_what_why` and
  `project_set_status`. Children never write it, so two children can't
  overwrite each other. The user edits it with `crook project`.
- Refresh on the user's turn, never on a machine-started one. `done` carries
  only a fixed outcome and a PR URL, so a refresh on the `done` wake-up
  could only summarise child-authored text (a PR body) into a card every
  sibling trusts. `project_set_*` and `project_add_*` refuse unless the current turn began
  with a `composer` or `bridge` prompt, saying so, so the model doesn't
  retry, which the mod already tracks for
  `status()`.
- Stale flag: after 3 `done`s or 7 days with no card update, the
  orchestrator's status line shows `card stale`. It's a flag, not a prompt,
  so it costs no machine-started turn. On the next `composer` or `bridge`
  prompt, the orchestrator's mod adds `the card is stale: move Where it
  stands on, keeping what is still true` to the context it already attaches.
- Stale-write check: `status()` returns the card's `version`. The
  `project_set_*` tools take it and refuse if the card changed since, as
  loam's writes do. The check is one `UPDATE … WHERE version = ?` and a
  rows-changed test. Every project write, `project_add_*` included, bumps
  the version and returns the new one, so a second set in the same turn
  works. The append-only `project_add_*` take no version. The CLI checks
  the same way, so the project record has two writers that can't overwrite
  each other.

### Private locations and the privacy check

**Declaring them:**

- The project record has a `private` list of paths and globs. It always includes
  the project's notes folders.
- `~/.config/crook/config.toml` has a user-wide `private` list for every
  project, for example the vault, so it's declared once.
- The same files hold `allow` entries for known false positives.

**The check**, in the `PreToolUse` hook:

- **Which calls:** every tool call except:
  - `Read`, `Glob` and `Grep`, which only bring data in
  - `Write` and `Edit` whose target is inside a private location
  - crook's own tools, `Agent`, `Skill`, `TodoWrite` and `ToolSearch`,
    whose inputs stay in this machine's sessions

  That covers every MCP tool, including ones the user adopts later, plus
  Bash, Monitor and `WebFetch`, and `Write` and `Edit` into a repo, so
  private text can't reach a commit or a `--body-file` unasked.

- **What it looks for** in the call's input (`content` and `new_string` for
  file writes):
  - a private path, absolute, `~`-relative, or relative to the hook's `cwd`
    and resolving into a private location, including one embedded in a
    longer string (`--body-file=<path>`, `file=@<path>`, a URL) and a
    private root containing spaces
  - a private path written with `$HOME` or `${HOME}` for the home directory
  - in a Bash or Monitor command, when the hook's `cwd` is inside a private
    location: a bare name of an existing entry there, `.`, a glob, or
    `$PWD`/`$(pwd)`. The shell's working directory
    persists between calls, so `--body-file plan.md` from inside a notes
    folder names a private file without a slash. Whether the hook's `cwd`
    follows the shell is checked in spike 9.
  - a verbatim chunk of a private file's text, at least a minimum length.
    A chunk doesn't count when it also appears in the record's public links
    and tickets (notes quote ticket titles and PR URLs), in the edited file's
    current content or `old_string`, or in a project repo's tracked files
    outside every private location, because notes quote code, diffs and
    error text too. The repo search excludes private paths and returns the
    whole matching lines (`git grep -h -F -f <chunks> -- .
    ':(exclude)<private path>'`); only chunks found in those lines are
    dropped, overlapping ones included. A repo that sits wholly
    inside a private location isn't searched. Otherwise notes tracked in a
    repo would cancel themselves out. Chunks are compared line by line after
    whitespace normalisation.

- **No Bash exemption.** Bash and Monitor are checked like every other
  tool, so a command that names a private path or quotes private text asks,
  even a plain `cat` of a note. Reading notes needs no Bash: `Read`, `Grep`
  and `Glob` are exempt, and `Write` and `Edit` into a private location are
  exempt, so the protocol tells sessions to use those tools for notes. A
  per-command exemption read by a shell lexer (drafts 8 to 8.7) leaked in
  review after review (`cd`, `sed -n w`, `git -c`, `$'…'`, `..`) and was
  dropped at the user's call on 2026-11-20: the goal is that private
  details don't leave unasked, and an extra prompt for Bash work in notes
  is the price.

- **On a hit:** `ask`, with the reason `crook: private reference: <first
  hit>` and the number of others. The dialog shows the call itself.

- **Otherwise:** no decision. The user's config and mode handle the call as
  usual.

- Every hit goes to the `asks` table. The user's answer isn't logged: a
  denied call never reaches a later hook, so it could only ever say "allowed".

The verbatim index lives in its own files under `<slug>/index/`, not in
`crook.db`, keyed by file and mtime: the tested shape (spike 5). It's the
biggest and most-churned data, and in `crook.db` its writes would hold the
project's one write lock and wake every watcher. Each hook run refreshes
the entries for files that changed, so a note written ten minutes ago is
covered. Index writes are write-then-atomic-rename, with no lock, so a crashed hook
leaves nothing held.

### Session tools

The mod registers its tools at `session.start` with `$.tool.register`, with
`isDeferred: false` so they're always in the tool list. The model sees them as
`mcp__crook__<name>`. Each one reads the caller's identity from the session
environment, never from its input. A session registers only its own role's
tools.

| Role | Tools |
|---|---|
| child | `take_task()`, `need(question)`, `done(outcome, pr_url?)`, `status()` |
| orchestrator | `status()`, `spawn(child, repo, brief_text, branch?)`, `task(child, text)`, `stop(child)`, `project_set_what_why(what, why, version)`, `project_set_status(text, version)`, `project_add_link`, `project_add_ticket`, `project_add_repo`, `project_add_notes` |

- `take_task` returns the oldest untaken task's text and marks it taken.
  This is the acknowledgement, and it means the model never reads crook's
  data dir. A taken task without a `done` after it is the child's current
  task: `status()` returns it, and the card carries it after `/clear` or
  compaction.
- `status()` labels open questions "waiting on the user: don't answer or
  work around it".
- `spawn` creates `<repo>/.worktrees/<child>` on branch `crook/<child>` (or
  `branch`), then starts the child there.
- `task` adds a row to the child's inbox. Text goes to `crook` on stdin,
  never in argv.
- `stop` stops a child and marks it archived.
- `done` URLs must match a PR URL pattern.
- The generated `--settings` allow `take_task`, `need`, `done` and
  `status`, so a child never stops at a permission prompt just to take its
  task or ask its question. Whether the orchestrator's tools prompt is the
  user's config, like any MCP tool, with two exceptions. The settings hook
  asks on `stop` when the target child is parked, and on `spawn` when a
  parked child exists in the same repo, so stopping or duplicating a child
  can't route around the user's answer.
- `take_task` refuses while the child has a current task open. The child
  calls `done` before taking the next, so the card always holds the whole
  current task.
- Every crook tool refuses, with a message, in a session that isn't the one
  recorded in the `children` table (see Forks).

Every crook tool is served at `tool.run`, which fires only after the
permission check and the settings hook have allowed the call. An answer at
`tool.call` would skip the permission check (plugin-authoring reference).

### The CLI

Everything here is for the user, outside any session.

| Command | Does |
|---|---|
| `crook init <slug>` | creates the project record |
| `crook list` | one line per project, found by globbing `crook/*/crook.db`: slug, what, children working, parked and done, card stale, orchestrator live or not. It opens each database read-only, one at a time, reads older schema versions without migrating them, and prints a broken database as an error line without stopping. |
| `crook project <slug> …` | edits the record: `add-repo`, `add-notes`, `add-private`, `add-allow`, `add-link`, `add-ticket`, `remove`, `set-what`, `set-why`, `set-status` |
| `crook open <slug>` | starts or attaches to the orchestrator |
| `crook status [<slug>]` | one line per child, merging crook state, `claude agents --json` and herdr when present |
| `crook next` | jumps to the oldest child waiting on the user, across every project, either parked on a question or at a permission prompt: focuses its herdr pane, or runs `claude attach` |
| `crook unpark <child>` | lifts a park by hand, for when the mod's filter is skipped or broken |
| `crook resume <child>` | resumes the child's current session id with crook's flags |
| `crook stop <child>` | as the tool, plus worktree removal, refused if the worktree has uncommitted changes |
| `crook launch <slug>/<child>` | hidden; what a herdr pane runs to start a spawned child: builds its environment and argv from the rows and execs `claude`. Refuses a child whose session is already running |
| `crook completion print <shell>` | prints shell completions; it touches no state, so it skips the user-verb gate, as `--version` and `--help` do |

`unpark`, `resume` and `stop` take `<child>` or `<slug>/<child>`. A bare
`<child>` that more than one project has is refused, listing the
`<slug>/<child>` forms.

Sessions are addressed by session id, never by name, because names match on
partial text (`help.txt:266`). herdr panes are looked up each time, never
stored.

### Launching

```text
crook open <slug>
  if the `orchestrator` row names a live orchestrator for <slug>: start
    nothing; focus its herdr pane, or print its pid and session id
  refuse if allowManagedModsOnly or allowManagedHooksOnly is set
  unpack the embedded mod if its directory is missing
  CROOK_PROJECT=<slug> CROOK_ROLE=orchestrator CROOK_BIN=<abs crook> \
  claude -n crook-<slug> \
         --session-id <pre-assigned>, or --resume <id> after a reboot \
         --permission-mode <user default; manual instead of bypass or plan> \
         --plugin-dir <unpacked mod dir> \
         --append-system-prompt "<orchestrator protocol>" \
         --settings <generated> \
         --add-dir <each notes folder>
  cwd: the first repo in the record

spawn (the orchestrator's tool)
  checks repo against the project record, the brief's length and the child cap
  writes the child's rows first, then launches; a crash between leaves rows
    that `crook open` reconciles
  sets CROOK_PROJECT, CROOK_ROLE=child, CROOK_CHILD=<id>; scrubs any other
    CROOK_* and CLAUDE_CODE_MESSAGING_* from the environment
  same flags, with a new --session-id
  with herdr, the pane runs only `<CROOK_BIN> launch <slug>/<child>` (with
    `CLAUDECODE` and `CROOK_*` unset), a user verb that builds the
    environment and argv and execs claude, so no environment value or
    prompt text is typed into the shell
  cwd: the new worktree
  with herdr: a new pane in the project's workspace; without: claude --bg
```

The generated `--settings` contain:

- **`disableAllHooks: false`.** Without it, a repo's `disableAllHooks: true`
  silently disabled crook's hook (tested). Command-line settings outrank
  project settings (`settings.md:212`).
- **`crossSessionInbound: "refuse"`.** The sender got "not accepting
  cross-session messages" (tested). Outbound `SendMessage` is left to the
  user's config: a deny rule takes only the bare tool name and would also
  remove messaging to subagents (`cross-session-messaging.md:291, 304`).
- **In children, a deny on `CronCreate`, `ScheduleWakeup`, `RemoteTrigger` and
  `/loop`.** Tasks come from the inbox only.
- **Read deny rules** on the whole crook data dir, this project's included,
  and an **Edit/Write deny** on it. Sessions reach crook state only through
  crook's tools, so `status()` hiding question text can't be sidestepped by
  reading `crook.db`.
- **The crook hook**, with `onFailure: "block"`.

### State

One SQLite database per project, in WAL mode, under
`${XDG_DATA_HOME:-~/.local/share}/crook/`:

```text
<slug>/
  crook.db         schema version in PRAGMA user_version
    project        the record: what/why, where it stands, repos, links, tickets, notes folders, private, allow; card version, time and done count at the last update
    orchestrator   current session id, pid; `crook open` refuses while it names a live process
    children       per child: repo, worktree, branch, brief, current session id, archived
    inbox          tasks: child, seq, text, taken
    status         per child: state (working|parked|done), seq, question, outcome, pr_url
    forks          session ids of each child's forks
    seen           last status seq shown in the orchestrator, per child
    notified       last done seq the orchestrator was woken for, per child
    asks           every privacy hit: time, session, tool, hit
  gen              rewritten after each coordination commit; the watchers stat it
  index/           the verbatim index of private files, by file and mtime
```

Slugs match `[a-z0-9-]+`, because the slug is a path component.

Only `crook open` and `crook resume` migrate the schema. Mod verbs and the
hook work against any version they can read, and otherwise fail with `run
crook open <slug>`.

User-wide settings live in `~/.config/crook/config.toml`: the `private` and
`allow` lists.

- **Only the `crook` binary opens `crook.db`.** The mod API has no SQLite,
  only `$.fs` and `$.process` (`claude-code.d.ts:3882, 4195`), so the mod
  runs `crook`'s mod verbs through `$.process.run`. The launcher exports an
  absolute `CROOK_BIN`, because `$.process.run` takes `argv[0]` with no
  shell and a pane's or `--bg` session's `PATH` isn't checked. The mod runs
  it once at `session.start`; on failure it puts `crook: binary not found`
  on the status line and its tools refuse with that message.
- **Writers, by verb**, enforced in crook's code from the caller's entry
  point and identity (`CROOK_ROLE`, `CROOK_CHILD`):

| Verb | Caller | Writes |
|---|---|---|
| `init`, `project …` | user | `project` |
| `open` | user | `orchestrator`, `children` (reconcile) |
| `unpark` | user | that child's `status` |
| `stop` (CLI or tool) | user, orchestrator | sets `archived` on that child's `children` row; rows stay, so counts don't move |
| `spawn` | orchestrator | new `children`, `status` rows |
| `task` | orchestrator | `inbox` insert |
| `project_set_*`, `project_add_*` | orchestrator, user turn only | `project` |
| wake-up bookkeeping | orchestrator's mod | `seen`, `notified` |
| `take_task` | that child | its `inbox` rows (taken) and its current task |
| `need`, `done`, park lift | that child, its mod | its own `status` row |
| `SessionStart` | the hook | the session id in `children` or `orchestrator`, `forks` |
| `/crook-adopt` | that child's mod, `composer` only | the session id in `children`, `forks` |
| privacy hit | the hook, any session | `asks` |

- **The `project` row has two writers**, the CLI and the orchestrator's
  tools. Its version check stops one overwriting the other. A transaction
  can't replace the check: the orchestrator reads the card in one turn and
  writes it in a later one.
- **Every write transaction is `BEGIN IMMEDIATE`.** A deferred transaction
  that reads and then writes fails at once with `database is locked` when
  another writer committed in between, whatever `busy_timeout` says (tested
  in review 11). `take_task` and the version check both read then write.
- **Concurrent writers wait** on a `busy_timeout` of 5 s, well under
  `$.process.run`'s 30 s default and the hook's timeout. WAL lets readers
  run during a write, so `crook status` never blocks a child, and the
  settings hook's park check never waits on a writer.
- **A crash mid-transaction rolls back**, including when `$.process.run`
  kills `crook` on timeout. `spawn` and `stop` also touch a worktree and a
  session, which aren't atomic with the rows: rows first, then the launch
  or teardown, and `crook open` reconciles the rest.
- **An `asks` write that times out** doesn't change the hook's decision. The
  hook returns its `ask` regardless and reports the lost log line on
  stderr.
- **Not "only the orchestrator writes".** A child's park and `done` would
  then have to reach the orchestrator first, which is messaging between
  sessions, and would stall while the orchestrator is busy, compacting or
  gone.
- **Mod calls:** text (questions, tasks) goes to `crook` on stdin, never in
  argv. The park filter awaits the lift write before `next(e)`, so the
  model's first tool call doesn't race the settings hook; `$` call time
  doesn't count against the hook's budget (`claude-code.d.ts:6167-6169`). A
  failed lift shows on the status line. The status line renders from the
  mod's cached state, never from a `crook` run inside `ui.render`.
- The `SessionStart` hook records the current session id on `startup`,
  `resume`, `clear` and `compact`, so `crook resume` resumes the right one.
  It doesn't record on `fork`.

**Forks.** A fork (`/branch`, the `/fork` background copy, moving a
conversation to the background, `--fork-session`, `hooks.md:1299`) is never
the child. Its id isn't in the `children` table, so its watcher stays passive and its
crook tools refuse with "this is a fork; the child is session <id>". The
original stays the child, with one exception: `/branch` moves the user into
the fork in the same process ("You're now in the new one", tested), leaving
the original on disk. So when `source` is `fork` and the hook's Claude Code
process is the one recorded with the child's session id, the hook records
the fork as the child instead. Otherwise, to carry on in a fork, the user
types the mod's
slash command `/crook-adopt` there. It's accepted only from a `composer`
origin, and it records the fork's id in the `children` table. The `SessionStart` hook
records every fork id in the child's `forks` rows when its `source` is
`fork`. The settings hook applies the park denial to every session in the
child's worktree except recorded forks, so a fork of a parked child isn't
locked, and a stale id never unlocks the child itself. The id in
`children` is always denied while parked, even if it is also in `forks`. Adopting a fork
removes its id from `forks`, so the adopted session is park-denied like
any child.

- The park state lives in the `status` table, not in the mod's memory, so it
  survives a mod reload, `/clear` and a resume.

### Protocol

| Flow | How |
|---|---|
| start | launch prompt: `You are child c1 of crook-foo. Your project card is above.` |
| task | `task` tool → inbox row → the child's watcher sees it and submits `crook: you have tasks waiting. Call take_task.` The child takes them one by one. |
| task while the child is busy, parked or at a dialog | the watcher waits. It submits only when the session is idle and not parked. A submit that was already queued is held through a dialog (tested). |
| question | `need("keep the old column?")`, then the child ends its turn |
| answer | the user types in the child (`crook next`) |
| before opening a PR | the child calls `need` first. Protocol only, not enforced: whether the PR call itself prompts is the user's config. |
| done | `done(ok, url)`, or `done(failed)` or `done(dropped)`. The outcome is a fixed value, never free text, so no child-authored text reaches the orchestrator. |

**The watcher**, in the child's mod, every 2 s:

- It stats `<slug>/gen` with `$.fs.stat` and runs `crook` only when its
  mtime moved, plus once every 60 s as a net. crook rewrites `gen` after
  each commit that touches `inbox`, `status`, `children` or `project`, so
  reads, `crook list` and `asks` writes wake nobody. Watching `crook.db-wal`
  doesn't work: it's missing after a lone writer closes, and a read-only
  open leaves a fresh empty one (tested in review 11).
- It skips a tick while its last `crook` run is still going.
- It compares the `gen` mtime taken before its `crook` read, so a commit
  that lands during the read is seen on the next tick.
- It reads and caches whenever `gen` moves, busy or not. Only the submit
  waits: it sends nothing unless the session is idle, the child's `status`
  row isn't parked, and its session id is the one in the `children` table.
- **Idle** comes from the mod's `turn.start` and `turn.complete` events,
  counting only the main loop (`agentId` unset), because both also fire for
  subagents (`claude-code.d.ts:5409-5411`). At `session.start` the watcher
  assumes idle. A wrong guess is harmless: a plugin prompt only runs "once
  the session is idle" (`claude-code.d.ts:11567-11569`).
- It sends **one** fixed-text wake-up for the whole inbox, never one per
  task. The text never includes task content, so nothing from the inbox
  reaches the model except through `take_task`.
- A task stays in the inbox until `take_task` moves it. The watcher never
  offers while a task is open, because `take_task` would refuse.
- It offers **once per inbox state**: the newest task's seq plus the number
  taken. A new task or a `take_task` (and so the next task after a `done`)
  makes a new state and a new offer. A child that ends its turn without
  taking a task isn't offered again until the inbox changes, a park lifts,
  or the mod restarts, so wake-ups can't loop. The orchestrator's status
  line shows each child's untaken count (`c1: 3 untaken`), so a child that
  ignores its inbox is visible. Miss counting, a 1/5/15-minute backoff and
  an `ignoring` flag (reviews 8 and 9) were dropped for simplicity on
  2026-11-20; this rule bounds wake-ups without them.
- A wake-up that slips into a parked child anyway (a race with `need`) does
  no harm: `take_task` is denied while parked, so the task stays untaken and
  is offered again after the answer.

**Parking:**

- From `need` until the user answers in that child:
  - the mod's `prompt.submit` hook lets every prompt through, but only a
    `composer` or `bridge` (Remote Control, which is the user too) prompt
    lifts the park. Dropping machine prompts would lose results: a
    background task's `task-notification` and a subagent's report (origin
    `peer`) both arrive this way (tested). A machine-started turn can't do
    anything while parked, because the tools are denied, and the model sees
    the result. Scheduled prompts can't occur in children, which deny the
    scheduling tools. A mod's own `prompt.submit` hook doesn't hear its own
    submissions (tested), which is why the watcher checks the park itself.
  - a slash command (`e.command` set, `claude-code.d.ts:11645-11656`) from
    `composer` or `bridge` passes but doesn't lift the park. The check runs
    before the origin check. After `/clear`, the card brings the open
    question back.
  - the settings hook denies every tool except `mcp__crook__status`. It reads
    the `status` table, so it doesn't depend on the mod.
- Any other `composer` or `bridge` prompt lifts the park (tested for
  `composer`; `/loop` typed while parked passed without lifting, tested).
- If the filter throws, its `.catch` lets a `composer` prompt through and
  writes the lift to the `status` table, so the user's answer still works. If a
  higher-tier mod skips the filter, `crook unpark <child>` lifts the park by
  hand.
- **Residual:** text typed into the terminal by another program, such as
  herdr, tmux or osascript, is also `composer` (tested with herdr). A session
  that types into a parked child's pane could answer for the user. Whether
  that call is allowed is the user's config. The protocol forbids it.

**Keeping the orchestrator current:**

- When children call `done`, the orchestrator's mod submits one fixed text,
  `crook: children are done. Call status().`,
  once the orchestrator is idle.
  Several `done`s while it's busy make one wake-up.
  This is the same mechanism as a child's wake-up, not a session message,
  and it lets the orchestrator carry on as it would after a subagent.
  - It records the child's done seq in the `notified` table, so a reload or
    restart doesn't wake it twice.
  - It stops after five machine-started turns in a row with no typed prompt
    between them, and falls back to the status line. A `done` → `task` →
    `done` loop can't run without the user.
  - Child-authored text never rides on these turns. Question text reaches
    the model only on a turn the user started, as below, and `status()`
    omits question text unless the current turn began with a `composer` or
    `bridge` prompt.
- `need` doesn't wake the orchestrator, because only the user can answer.
- The orchestrator's mod keeps one status line with a summary across
  children, for example `crook: 1 waiting on you · 2 working`, plus each
  child's untaken count when non-zero (`c1: 3 untaken`) and `card stale`
  when the card is overdue. With herdr,
  `need` and `done` also raise a herdr notification (`herdr notification
  show <title> --body <child> --sound request`).
- On every `composer` or `bridge` prompt in the orchestrator, its mod's
  `prompt.submit` hook attaches `context` (`next({ ...e, context })`): one
  line per child whose seq moved since the `seen` table, with the child's
  question verbatim, marked untrusted and capped at 300 characters. Machine
  prompts get nothing: scheduled prompts, task notifications and crook's own
  wake-ups, which the mod doesn't hear anyway.

**The orchestrator never relays an answer.** It says who's waiting.

Borrowed ideas, credited:

- **needs_me relay**, from
  [herdr-quartermaster](https://github.com/travisjeffery/herdr-quartermaster):
  the child flags a question, and the orchestrator routes it to the user.
- **park-and-resume**, from [bermuda](https://github.com/bon5co/bermuda): a
  waiting child parks and resumes on the user's answer.
- **in-session delivery by a mod**, from claude-snooze, a private Claude
  Code plugin: a mod polls a store and delivers with `$.prompt.submit`.
- **a record kept current**, from [loam](https://github.com/GregorMcC/loam): Where it stands moves on when work
  lands, and a write fails if the record changed since it was read.

### Known gaps, accepted

- **Paraphrase.** The check catches paths and verbatim text, not a summary of
  a private note. The project card's rule is the only defence.
- **Indirection.** A script that reads a private file and posts it shows the
  script's path, not the content. If the script lives in a private location,
  its path is a hit; otherwise it isn't.
- **Paths built from other variables**, such as `$NOTES/plan.md` or a
  path assembled in a script. Only `~`, `$HOME`, `${HOME}`, `$PWD` and
  `$(pwd)` are resolved.
- **Encoded content**, such as base64 of a private file.
- **Short fragments** below the minimum chunk length.
- **A private repo pushed to its own remote.** With the shell inside a
  private git repo, a bare `git push` has no path operand and no verbatim
  text, so it doesn't ask. That's the repo going where it already goes (for
  example the vault to its own remote), which is the user's setup, so it's
  intended.
- **Forged hook input through Bash.** The model can pipe made-up
  `SessionStart` JSON into `crook hook SessionStart`, for example to record a fork.
  The `children` id stays park-denied whatever `forks` holds, so this can't
  lift a park. Tier B, like the next item.
- **A script writing straight into `crook.db`**, another child's inbox
  for example, with `sqlite3`. The Edit/Write deny covers the file tools,
  not Bash.
- **Other mods.** A higher-tier mod can pass events past crook's mod hooks
  (observed with `tool.check`), and on setups without managed settings a mod
  can approve past the settings hook (`permissions.md:659-661`). `crook open`
  names such mods.
- **Upgrading crook under live sessions.** Sessions carry an absolute
  `CROOK_BIN`. When mise replaces that binary, every tool call there blocks
  (the hook fails closed) until `crook resume <child>` or `crook open
  <slug>` relaunches it.

### Isolation between projects

| Path | Closed by |
|---|---|
| Messages | none sent by crook, inbound refused |
| crook tools | scoped by the session's `CROOK_*` environment |
| The crook CLI | user verbs refuse with `CLAUDECODE=1` or any `CROOK_ROLE`; mod verbs refuse with `CLAUDECODE=1` and take the slug only from `CROOK_PROJECT` |
| The Read tool on crook's data dir | Read deny rules, generated at every launch |
| Other projects' notes folders | only this project's folders are passed with `--add-dir` |
| crook's data | Edit/Write denied, and only crook writes it |
| Other projects' state | one `crook.db` per project; crook opens only the one named by `CROOK_PROJECT` |
| Auto memory | Per repo, as in the user's other sessions: crook sets no `autoMemoryDirectory`. It has been "shared across git worktrees of the same repository" since 2.1.63 (the changelog), so a child in a worktree shares its repo's memory, with the user's own sessions there too. That keeps the full config. A memory shared across a project's repos (draft 8.5's `memory = "project"`) is deferred: see Later. |
| `cat` through Bash | not closed (tier B). The protocol says don't. |
| MCP tools spanning projects (for example a loam plot reader on the allow list) | not closed. `crook open` notes it. |

### Restarts

- `crook open` reconciles the `children` table against `claude agents --json` and herdr.
- `crook resume <child>` relaunches the child's current session id with
  crook's flags and environment. It's in v1 because reboots are ordinary.
- After a restart, the watcher offers every untaken task again.
- A plain `claude --resume` drops the hook and the mod. `crook status` flags
  a live session in a crook worktree that has no crook mod.

## v1 scope

**In:**

- the CLI: `init`, `list`, `project`, `open`, `status`, `next`, `unpark`,
  `resume`, `stop`
- the mod: session tools, inbox watcher, park filter, orchestrator wake-up
  on `done` and status line, the stale flag and the card nudge on user turns
- auto memory per repo (crook sets no `autoMemoryDirectory`)
- the settings hook: privacy check, park denial, project card, session ids,
  and `--settings` generation
- herdr panes and notifications when herdr is present; `--bg` and
  `claude attach` when it isn't

**Later:**

- `crook export`
- unattended runs (the `PreToolUse` `"defer"` decision would fit, but it
  needs `-p`, `hooks.md:2015`)
- moving the settings hook into the mod, once mod hooks can't be skipped
- project-wide memory: explore whether a child's memories should reach the
  orchestrator, or the whole project, rather than staying per repo
  (`autoMemoryDirectory` from `--settings` is honoured, spike 6)

## Evidence

Tested with throwaway Haiku sessions, each cleaned up afterwards: on
2026-11-19 with Claude Code 2.1.328 and herdr 0.9.5, and on 2026-11-20 (mod
rows) with 2.1.329.

| Claim | Result |
|---|---|
| A child at a permission prompt is visible | Pass: `"status":"waiting"` in `claude agents --json`, `blocked` in herdr, `"state":"blocked"` for `--bg` |
| Text typed into a busy session is submitted at once, mid-turn | Observed (`UserPromptSubmit` fired during the first of three tool calls) |
| Settings hooks get herdr-typed text, scheduled prompts and mod prompts bare | Observed |
| `crossSessionInbound: "refuse"` through `--settings` | Pass. The sender was told the session "is not accepting cross-session messages". |
| `/clear` changes the session id | Observed |
| A repo's `disableAllHooks: true` disables `--settings` hooks unless `--settings` sets `false` | Observed, in `-p` mode |
| Hooks load through `--settings` and `--plugin-dir`, and plugin skills load | Pass |
| A `PreToolUse` `ask` shows a custom reason, cut off after about 10 lines | Pass / observed |
| A settings hook `ask` beats an explicit allow rule in `auto` mode | Pass |
| `claude attach` shows a `--bg` child's pending prompt | Pass |
| `CLAUDECODE=1` is set in Claude Code's Bash tool shells | Checked |
| A mod loads through `--plugin-dir`, two at once | Pass |
| A mod loads and its watcher runs in a `--bg` session | Pass. `session.start` fired, and an inbox file was submitted. |
| A mod's `$.prompt.submit` wakes an idle session | Pass. Delivered about 1 s after the inbox file appeared; the model replied. |
| The model reads it labelled | "The crookspike plugin sent a message: …", with `origin: {kind: plugin}` in the transcript |
| `$.prompt.submit` while a permission dialog is up | Held. The dialog and its command were untouched. Delivered after the dialog was dismissed. |
| A mod's `prompt.submit` hook sees the origin | `composer` for herdr-typed text in a pane, `composer` for text typed through `claude attach`, `plugin` for another mod's prompt |
| A mod's `prompt.submit` hook drops a prompt | Pass. The submitter got `{drop}`, and the model never saw it. |
| A mod tool takes identity from the environment | Pass. `need` recorded `proj/child/c1` from `CROOK_ROLE`. |
| A mod `tool.check` `ask` beats an allow rule in `auto` mode | Pass, with the reason and `[plugin:crookchild]` in the dialog. The command didn't run after Esc. |
| A higher-tier plugin can skip a mod's hook | Observed: 21 "its tool.check hook was skipped" notices from another installed plugin in one session |
| A mod's own `prompt.submit` hook hears its own submissions | No. Only other plugins' hooks see them. |

## Review history

Twelve adversarial reviews and two builder checks. Rounds 1–3 are summarised.

| Round | Finding | Answer |
|---|---|---|
| 1–3 | Token leaks; denylist egress; abusable spawn; ungated orchestrator; answers relied on an undocumented distinction; payload not shown; hooks fail open; waiters die; ungated resume; parked child answerable by task; nested `claude` | No tokens, no messaging. Typed answers. `onFailure: "block"`. No waiters. `crook resume`. Park enforced. The egress and steering findings were answered by a firewall until draft 8 dropped it. |
| 4 | Park deadlocked the nudge | No nudges |
| 4 | Over-built | Messaging, filters, Stop hook, heartbeat, runtime copy and `doctor` cut |
| 5 | crook's typing could land on a dialog or on the user's typing | Draft 7: nothing types; mod prompts are held during dialogs. |
| 5 | Park lifts on any bare prompt | Draft 7: lift only on `origin.kind: composer`. |
| 5 | The script had no caller identity | Draft 7: mod tools with environment identity. |
| 5 | Resume after `/clear` used a stale id | `SessionStart` records the current id. Panes looked up live. |
| 6 | `crook` through Monitor skipped identity | Draft 7: the CLI isn't callable from sessions at all. |
| Builder | Editing the record, worktrees, lifecycle, `next` scope, notes for children | `crook project`. Worktree per child. `done` → idle and taskable. `next` covers prompts too. Children get notes. |
| User, 2026-11-20 | herdr had become mandatory | Draft 7: mod wake-ups, herdr optional |
| User, 2026-11-20 | The egress firewall gets in the way of the user's own config | Draft 8: dropped. One privacy check, project card. |
| 7 | Private text written with `Write`/`Edit`, then committed or passed with `--body-file`, skipped the check | Writes outside private locations are checked. Relative paths resolved against `cwd`. |
| 7 | The park filter can't see crook's own wake-ups, so a queued task fires in a parked child and is stranded | The watcher checks the park and idleness itself. Tasks stay until `take_task`, which is denied while parked, and are offered again. |
| 7 | Typing through `claude attach` might not be `composer` | Tested: it is. `bridge` also lifts. |
| 7 | No escape if the filter breaks; slash commands lift the park; forks share one identity | `crook unpark`, `.catch` passes `composer`. Slash commands don't lift. Watcher passive unless its session is current. |
| 7 | False positives: crook's tools, bare file names, quoted ticket titles, readers in pipelines | Those tools exempt. File-name matching dropped. Public strings subtracted, minimum chunk length. Pipelines exempt only when every segment is a reader. |
| 7 | Stale index and card; auto memory may mix projects | mtime index. Card holds slow facts only; live state via `status()`. Per-project `autoMemoryDirectory`. |
| 7 | The orchestrator didn't carry on after `done` | Fixed-text wake-up on `done`. |
| 7 | Simplify: Bash `crook` deny, `SendMessage` deny, the lock, the toast | All cut. |
| 8 | A reader with a redirect (`cat note > /tmp/body.md`) was exempt, reopening the file route | Exempt only with no redirect or `tee`, every command in the whole list a reader, no writing flags; `awk` dropped; unparsable means not exempt. |
| 8 | Wake-ups could loop forever, one per task, and `take_task` could stop at a prompt | One wake-up per inbox, three backed-off re-offers, then the status line. Child tools allowed in `--settings`. `take_task()` takes the oldest. |
| 8 | The `done` wake-up carried child text into unprompted turns, and could loop | The hook skips fixed wake-up text. `status()` labels questions. `stop` on a parked child asks. `notified.json`. Five machine turns in a row, then the status line. |
| 8 | The per-project memory folder clashed with the data-dir deny and the full config | Dropped from v1. Spike 6 decides. |
| 8 | A taken task was lost after `/clear`; verbatim hits on ordinary code; `/fork` made the wrong session the child; idle undefined after reload | Current task in the card and `status()`. Chunks found in the edited file or tracked repo files don't count. No recording on `fork`; forks are passive and their tools refuse. Idle assumed at start, main loop only. |
| 8 | Simplify: `.catch` should lift the park; slash rule for `bridge`; index writers | All applied. |
| 9 | The `git grep` subtraction cancelled every chunk when notes are tracked in a project repo | Private paths excluded from the search; repos inside a private location not searched; line-wise normalisation, batched with `-f`. |
| 9 | The backoff flagged a child working through a queue, and never reset | Re-offer at once after a turn that took a task or called `done`; a miss is a whole turn with no `take_task`; any progress resets. |
| 9 | Question text reached orchestrator turns started by scheduled prompts or notifications | Injection moved to the orchestrator mod's `prompt.submit` hook, `composer` and `bridge` only. `status()` hides question text on machine-started turns. Orchestrator `UserPromptSubmit` hook removed. |
| 9 | The fork rule stranded the session the user continues in, and locked a parked child's fork | `/crook-adopt` from the fork (`composer` only). Park denial applies to the recorded session only. |
| 9 | The Bash exemption asked on routine work in private folders; some readers still wrote | Commands whose every path is private are exempt. `cd`, `pwd`, `test`, read-only `git` added. Per-reader flag allowlists. |
| 9 | Minor: coalesce `done` wake-ups; stacked tasks; `spawn` workaround | One `done` wake-up. `take_task` refuses while a task is open. `spawn` asks when a parked child shares the repo. |
| 10 | The private-locations Bash exemption let any command send a private file (`gh … --body-file <note>`, `curl -F`, `git push`) | Only readers and local filesystem mutators, with the same splitting, flag and redirect rules. |
| 10 | The watcher re-offered while a task was open, then flagged the child | Offers only with no task open; at once after `done` only. |
| User, 2026-11-20 | Can all of a project's sessions share one memory, across repos? | Draft 8.5: `memory = "own"` renamed `memory = "project"`; `"repo"` stays the default. Concurrent writes go to spike 8. |
| User, 2026-11-20 | loam keeps its what/why and where-it-stands current; crook should too | Draft 8.5: the orchestrator refreshes the card on `done`, a `card stale` flag, a version check on writes. |
| User, 2026-11-20 | One SQLite database per project instead of files, written only by the orchestrator? | Draft 8.6: one database, only the `crook` binary opens it, each row keeps its one writer. Only-the-orchestrator would need messaging between sessions. |
| 11 | The WAL mtime watch woke every watcher on reads; deferred transactions fail at once; the writer list broke five verbs; the `CLAUDECODE` gate clashed with mod and hook calls; `crook` off `PATH` failed silently; the index in `crook.db` held the write lock; refresh on `done` fed child text into the card | `gen` file plus a 60 s net. `BEGIN IMMEDIATE`, 5 s busy timeout. Verb table. Three entry points with gates. `CROOK_BIN`, checked at start. Index in its own files. Refresh only on user turns. |
| 11 | Minor: no spike for the subprocess path; version protocol; leftover file wording; memory folder unnamed; no schema version; mod-call details | Spike 9. Every write bumps and returns the version. Wording fixed, brief moved into `children`. Memory folder named and private. `user_version`, slug rule. Stdin, awaited lift, cached status line, skipped ticks. |
| 12 | Minor: forged hook input; `CLAUDECODE` fallback; memory false positives; verb table gaps; `project_add_*` on machine turns; index lock; migration owner; watcher mtime order. Signed off: "ready to plan from". | `children` id always park-denied, gap listed. `env` fallback. Spike 7 covers memory. `orchestrator` id, `ignoring`, `archived`. Both refuse off user turns. Atomic rename, no lock. `open`/`resume` migrate. mtime before read. |
| User, 2026-11-20 | One database for every project, so crook can list them? | No: one write lock for every session on the machine, isolation by query filter instead of by file, one corrupt file takes out every project. Draft 8.6 adds `crook list`, which globs the per-project databases. |
| 10 | Minor: park denial failed open on a stale id; Read on own state; `done` had no outcome; bare `git push` from a private repo | `forks.json`, denial for all but recorded forks. Read deny on the whole data dir. `done(outcome)` as a fixed enum. Push gap listed as intended. |
| User, 2026-11-20 | Build decisions: Rust; no full Bash parser; mise and packslip | Draft 8.7: one Rust binary with `crook hook` as an entry point; the Bash exemption uses a conservative lexer that refuses anything it can't read; the mod is embedded and unpacked at launch. |
| User, 2026-11-20 | Simplify the watcher | One offer per inbox state; untaken counts on the orchestrator's status line. Backoff and `ignoring` dropped for simplicity. |
| User, 2026-11-20 | Drop the `asks` answer; keep memory per repo | No `PostToolUse` hook; `asks` logs hits only. `memory = "project"` and spike 8 move to Later. |
| User, 2026-11-20 | Drop the Bash exemption | No shell lexer: Bash and Monitor are checked like every tool; notes are worked through `Read`, `Grep`, `Glob`, `Write` and `Edit`. Supersedes reviews 8–10's exemption rules and the `cd` row below. |
| User, 2026-11-20 | Drop `cd` tracking | Any `cd`, `pushd` or `popd` makes a command not exempt; `cd` leaves the readers list. Reverses part of review 9's answer. |
| 17 | A path written with `$HOME` wasn't detected; the spike 9 `cwd` probe used a directory Claude Code doesn't keep; the bare-name rule also fired on non-shell input | `$HOME`/`${HOME}` root forms, `$PWD`/`$(pwd)` in a private `cwd`; probe a kept directory; bare names for Bash and Monitor only. Other variables listed as a gap. |
| 16 | A bare file name, `.` or a glob in a private working directory (`--body-file plan.md` after `cd` into notes) was never checked; nothing confirmed the hook's `cwd` follows the shell | Bare names, `.` and globs count when `cwd` is private. Spike 9 checks `cwd` follows the shell. |
| 15 | A failed `cd` still leaked one separator further on (`cd x && true; cp …`); `ack` signature, hook store mode, turn latch and spike-9 fallback list left stale | First answered with an `&&`-only rule, then superseded by dropping `cd` tracking (the user row above). The rest fixed in the plan. |
| 14 | A `cd` that fails (`;`, `\|\|`) left operands resolved inside notes; the herdr pane typed every environment value; `git grep -o` drops overlapping shingles; "herdr present" undefined; `stop` could signal a stale pid; an `ack` by name could swallow a second `done`; lexical `..` through symlinks | `cd` only between `&&`, absolute targets; `crook launch`; whole-line `git grep`; `HERDR_ENV=1`; signal only a pid `claude agents` confirms; `ack` carries `done_seq`; any `..` not exempt. |
| 13 | The hook failed open outside a readable database; herdr children got no `CROOK_*`; embedded paths (`--x=<path>`, URLs, roots with spaces) slipped past; `cd` moved operands out from under the exemption; `sed -n w`/`e` and `git -c` executed; `git grep -l` couldn't say which chunk matched; ambiguous `<child>` across projects; the orchestrator kept `bypassPermissions` | Hook asks with `run crook open`. Pane command carries `env -i K=V`. Root substring search. `cd` tracked, `sed -n` scripts limited, `git` global options limited. `git grep -h -o`. `<slug>/<child>`. Mode mapping for every session. |

## Questions for the user

All answered.

1. **Sessions never message each other.** Inbound messages are refused, and
   tasks arrive through the crook mod reading the child's own inbox. No token
   or wrapper format needs trusting. **Answer, 2026-11-20:** yes, "seems
   safest and wisest".

## Spikes before building

Run on 2026-11-20 with Claude Code 2.1.329 and throwaway Haiku sessions,
cleaned up afterwards. Spikes 1–6 are done. Spike 8 was dropped with project
memory. Spikes 7 and 9 need a working build, so they're the plan's first
measurements.

| Spike | Result |
|---|---|
| 1. Park filter coverage | `/loop` typed while parked passed as a slash command and didn't lift the park. Its `scheduled-trigger` firings and a background `task-notification` arrived with those origins. A subagent's report arrives as `peer`. Dropping them lost the results, so the design now lets them through without lifting. `bridge` is not tested (needs Remote Control). |
| 2. Idle tracking | Pass. `turn.start`/`turn.complete` carry `agentId` for subagents and none for the main loop. An interrupt gives `turn.complete` with `reason: "aborted"`. A save mid-turn reloaded the mod only after the turn ended, and the old module heard the end. |
| 3. Resume, forks, `$.fs`, floor | Resume: `SessionStart` `resume` with the same id, and the mod started again. `/branch`: the user moves into the fork in the same process, so the design adopts it automatically. The mod's `$.fs` reads and writes the data dir under the Read and Edit/Write deny rules. Floor 2.1.318, from the changelog. Not tested: moving a conversation to the background, and the `/fork` background copy. |
| 4. Project card | Pass. The model quoted the `SessionStart` context on `startup`, `clear` and `compact`, and the hook fired on `resume`. |
| 5. Privacy check | Matcher `*` saw `Write`, Bash and mod-tool inputs. `ask` held in `auto` mode for `Write` and for a mod tool. The mod-tool dialog showed crook's reason; the `Write` dialog showed the file's content but not the reason. A sorted, merged hash index of 700 notes (6.8 MB) answers a lookup in about 5 ms, plus about 20 ms of Python start-up. |
| 6. Auto memory | Shared across a repo's worktrees (changelog, 2.1.63). `autoMemoryDirectory` from `--settings` is honoured: the model wrote its memory there. |
| Extra: subagents under `refuse` | Pass. With `crossSessionInbound: "refuse"`, a background subagent's report was still delivered. |
| 7. False positives | Not run: needs a working build. |
| 8. Shared project memory | Dropped with project memory (deferred to Later, 2026-11-20). |
| 9. The mod running `crook` | Not run. From a mod in a pane and in `--bg`: `$.process.run([CROOK_BIN, …])` inside `prompt.submit`, `tool.run` and the 2 s tick, with `CROOK_*` inherited; whether `CLAUDECODE` is set there; `$.fs.stat` on `gen` under the Read deny; latency per call. `$.process` is CLI only (`claude-code.d.ts:4190`). |

## Left to the builder

- The languages, decided 2026-11-20: TypeScript for the mod (the engine
  compiles it), Rust for the binary and the hook, with `rusqlite`. The
  mod's logic is plain TypeScript under `plugin/hooks/core/`, unit-tested
  outside the engine.
- How verbatim chunks are matched, the minimum chunk length, and the index
  format. Tested shape: 8-word shingles hashed to 8 bytes, one sorted merged
  file plus a per-file mtime manifest, kept outside `crook.db`. The build
  keeps both in one file, so one rename replaces them.
- The fork rule needs the hook to know its Claude Code process, for example
  its parent pid recorded next to the session id.
- The inbox poll interval (2 s tested).
- The stale-card thresholds. 3 `done`s and 7 days are starting guesses.
- The `crook.db` schema, the card's wording, the protocol wording and the
  `crook status` and `crook list` formats.
- The herdr layout, as long as each project has one workspace.
