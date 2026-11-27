# Plan: the `crook` Rust binary

Date: 2026-11-27. Area 1 of 4. The other areas are the Claude Code plugin (2), the herdr-app mod (3), and
release, CI and e2e tooling (4).

The spec is the accepted ADRs in [`docs/architecture/decisions/`](../architecture/decisions/), 0001 to 0026
without 0011. Evidence is in the [spike report](../architecture/research/2026-11-26-hc-herdr-app-spikes.md).
ADRs are cited by number below, spikes as "spike N".

## Goal

Build the `crook` binary that runs every mechanical step of a flock (0020): it creates and opens flocks, starts,
stops and resumes children through `hc`, keeps the flock's JSON files, runs the plugin's hooks, installs the
herdr-app mod and checks the install. The model keeps the judgement work; crook does the exact steps the same
way every time.

## Scope

In scope:

- CLI and argument parsing for every subcommand in 0020's table, plus `--model` (0025), `--repo` (0023),
  `setup` and `doctor [--fix]` (0019), and `completion print` (0020).
- Config file (0026) and XDG path resolution (0004, 0019, 0026).
- The flock folder layout (0004, 0012) and the file formats crook reads or writes (0024), with atomic writes.
- The hc client: calling `hc`, parsing its JSON, preflight, core start with a scrubbed environment (0016, 0020,
  spike 23).
- Child worktrees (0023).
- `crook hook <event>` for `session-start`, `stop` and `user-prompt-submit`, with the never-break-the-session
  contract (0020).
- herdr-app space and tab renames (0021).
- Self-location, the debug-build fallback, `CROOK_BIN`, and the forward-only `current` link (0019).

## Non-goals

- The plugin's agents, skills and `hooks.json` (area 2). This plan only states what the binary expects of them.
- The mod's UI (area 3).
- Release workflows, the packslip manifest, CI and the e2e skill (area 4).
- Children on other machines (`hc run --on`), out of scope by 0022.
- `list` and `wipe` subcommands, and typed tools (0020, "More information").
- Restricting children's access (0006).
- Any database (0014).

## Shared mechanics

Every command depends on the rules in this section. Where an ADR is silent, the rule below is marked
"proposed" and repeated under [Open questions](#open-questions). Tasks build on the proposed answer, so a
different answer changes only the task that owns the rule.

### Names and ids

- A slug and a child name match `[a-z0-9-]+`, start with a letter or digit, and are at most 40 characters
  (proposed, open question 19). They end up in paths, branch names, hc names and labels.
- The flock id is the slug (proposed, open question 1). The folder is `<yyyy-mm-dd>-<slug>` (0004). Slugs are
  unique across dates: `new` refuses a slug that any folder already uses, and `open` finds the one folder that
  ends in `-<slug>` and whose `roster.json` says `"flock": "<slug>"`.
- hc names (proposed, open question 2): the coordinator is `<slug>`, a child is `<slug>.<child>`. The roster,
  `briefs/<child>.md` and `results/<child>.md` keep the short child name.
- Labels on every crook terminal: `crook.flock=<slug>`, `crook.version=<version>` (0019, 0020). Children also
  get `crook.child=<child>` (proposed, open question 3). The coordinator also gets
  `herdr.space=crook-<slug>` and `herdr.tab=crook-<slug>-coord` (0015, 0021). Children get no `herdr.*`
  labels (0021).
- Branch `crook/<slug>/<child>`, worktree `<repo>/.worktrees/<slug>-<child>` (0023).

### Paths

| Name | Path | ADR |
| --- | --- | --- |
| Data home | `${XDG_DATA_HOME:-~/.local/share}` | 0004 |
| Flocks root | `<data home>/crook/flocks/` | 0004 |
| Flock folder | `<flocks root>/<yyyy-mm-dd>-<slug>/` | 0004 |
| Notes folder | `<notes_root>/<yyyy-mm-dd>-<slug>/` when `notes_root` is set, else the flock folder | 0004, 0012 (proposed, open question 4) |
| Config | `${XDG_CONFIG_HOME:-~/.config}/crook/config.toml` | 0026 |
| `current` link | `<data home>/crook/current` | 0019 (open question 5) |
| Session version records | `${XDG_STATE_HOME:-~/.local/state}/crook/sessions/<session id>` | proposed, open question 6 |
| herdr-app home | `${HERDR_APP_HOME:-~/.config/herdr-app}` | spike 17 |
| Mod install | `<herdr-app home>/mods/crook/` with `.crook-version` | 0019 |
| mise shim | `~/.local/share/mise/shims/crook` | 0019 |

An empty variable counts as unset, as in the XDG spec. Paths are built by hand, not with a platform crate
(0026). A missing `HOME` resolves under `/`, so writes fail loudly, as v1's `src/paths.rs` did.

Inside a flock folder (0012, 0024): `roster.json`, `briefs/`, `results/`, `results/.delivered.json`, and a
`.lock` file (proposed, see below). With `notes_root` unset, `README.md` and `notes/` live there too.

### Which flock and which session

A command run by the coordinator, or a hook, finds its flock from its own terminal:
`hc get "$HC_TERMINAL_ID"`, label `crook.flock` (spike 14 did this). A child hook finds its child name from
`crook.child`. The coordinator is the terminal with `crook.flock` and no `crook.child`. A maintainer outside
any crook terminal passes `--flock <slug>` (proposed, open question 3). With neither, the command stops and
says which to use. A hook with no `HC_TERMINAL_ID`, or whose terminal has no `crook.flock` label, does
nothing.

### Writing files safely

- Every JSON file crook writes goes to a temporary file in the same folder, is flushed, and is renamed into
  place (0024).
- Atomic rename stops torn reads, not lost updates. Two `spawn` calls, or two children's Stop hooks, can each
  read, change and write the same file. So every read-modify-write of `roster.json` or `.delivered.json` holds
  an exclusive lock on the flock's `.lock` file for its whole cycle. std's `File::lock` does this with no new
  dependency (it compiles and runs under the local `rustc 1.99.1`, checked 2026-11-27).
- Readers that only read (for example `status`) take no lock; the rename keeps them safe.

### The launch command

`crook spawn`, `crook resume` and `crook open` build one argv each, through one pure function, so the order is
tested once. The prompt rides last and must follow a flag that takes exactly one value (0007, spike 16).
This plan assumes `--add-dir` takes several values, like `--allowedTools`; that is not checked (spike 16 tested
only `--allowedTools`). So `--add-dir` goes before `--agent`, never right before the prompt, which is safe
whether or not the assumption holds (proposed order, open question 7):

```text
hc run --bg --name <hc name> --cwd <folder>
  --label crook.flock=<slug> --label crook.version=<v> [--label crook.child=<child>]
  [--label herdr.space=crook-<slug> --label herdr.tab=crook-<slug>-coord]
  (--parent <coordinator terminal> | --no-parent)
  -- [env CROOK_BIN=<path>]
  claude --plugin-dir <data home>/crook/current/plugin
  --add-dir <flock folder> [--add-dir <notes folder>]
  --agent <role> [--model <model>]
  (--session-id <uuid> | --resume <uuid>) "<prompt>"
```

- `--plugin-dir` is the literal link path, never the resolved version folder (0019, spike 18). A debug build
  uses the repository's `plugin/` instead (open question 5).
- `--no-notify` and `--outlive-parent` are never passed (0007).
- The coordinator is started with `--no-parent`, as in spike 24, so a maintainer's `crook open` from inside
  another terminal does not make that terminal its parent.
- `env CROOK_BIN=<path>` is added when `CROOK_BIN` is set in crook's environment (0019, spike 23). For
  `open` too (open question 8).
- The coordinator gets `--agent coordinator` and no `--model`: it runs on the maintainer's default model
  (0025).

### Never break the session

`crook hook <event>` always exits 0, writes errors to stderr only, and catches panics (0020). The release
profile keeps `panic = "unwind"`, or `catch_unwind` does nothing (v1's `Cargo.toml` did the same). Every
subprocess has a timeout, and the hook has an overall time budget below the timeout `hooks.json` sets (area 2).
A Stop hook never prints a `block` decision.

A hook returns before any hc call when `HC_TERMINAL_ID` is unset. A hook never runs `hc server start`: a
session with `HC_TERMINAL_ID` runs inside a live core. Hooks skip the version and schema checks of preflight,
which would cost two extra hc calls on every turn (proposed, open question 20).

## Crate and module layout

One crate at the repository root, as v1 had, so `env!("CARGO_MANIFEST_DIR")` is the repository root and the
debug fallback finds `plugin/` and `mod/` there (0019).

| Path | Holds |
| --- | --- |
| `Cargo.toml` | Package `crook`, edition 2024, `rust-version` as v1 (`1.99.1`); release profile `panic = "unwind"` |
| `src/main.rs` | Parse, build the context, dispatch, map errors to exit codes |
| `src/lib.rs` | Module list; the `Env` type; crate-wide lints (deny `unwrap`, `expect`, `panic` outside tests, as v1) |
| `src/cli.rs` | clap derive definitions for every subcommand |
| `src/error.rs` | One `Error` enum with plain messages |
| `src/ctx.rs` | `Ctx`: the injected environment, runner, hc client, herdr-app client, clock and install paths |
| `src/paths.rs` | XDG resolution and every path in the table above |
| `src/config.rs` | `config.toml` |
| `src/names.rs` | Slug and child validation; hc names, labels, branch and worktree names |
| `src/files/atomic.rs` | Temp-and-rename writes; the flock lock |
| `src/files/roster.rs` | `roster.json` |
| `src/files/result.rs` | Result frontmatter `status` and SHA-256 |
| `src/files/marks.rs` | `results/.delivered.json` |
| `src/flock.rs` | Find, create and lay out flock folders |
| `src/git.rs` | Worktree add and remove, and the safety checks |
| `src/install.rs` | Self-location, plugin and mod folders, version, the `current` link |
| `src/run.rs` | `Runner` trait and the real runner with timeouts |
| `src/hc/mod.rs` | `Hc` trait and its types |
| `src/hc/client.rs` | The real client over `Runner`: argv and JSON parsing |
| `src/hc/preflight.rs` | Version floor, schema check, core start |
| `src/hc/scrub.rs` | The variable filter for core start |
| `src/hc/fake.rs` | In-memory fake for tests (`#[cfg(test)]` and an internal test feature) |
| `src/herdr_app.rs` | `space.rename` and `tab.rename` |
| `src/launch.rs` | The pure argv builder and the prompt texts |
| `src/cmd/*.rs` | One file per subcommand: `new`, `open`, `close`, `spawn`, `stop`, `resume`, `status`, `setup`, `doctor`, `completion` |
| `src/hook/mod.rs` | The contract: stdin, panic catch, budget, exit 0 |
| `src/hook/{session_start,stop,prompt}.rs` | One file per event |
| `src/hook/version.rs` | Session version records and the self-reload |
| `tests/` | Binary-level tests with a fake `hc` on a cleared `PATH` |
| `tests/fixtures/hc/` | Real `hc` JSON captured from a scratch core (task 4.1) |

Unit tests sit in a `tests` submodule next to each module, as v1 did (`src/paths/tests.rs`). A task's
"Files" line names the source file; its unit tests go in the matching `tests.rs`.

Key interfaces (signatures only):

- `pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;` as v1, so library code never reads the process
  environment.
- `trait Runner { fn run(&self, cmd: Cmd, timeout: Duration) -> Result<Output, Error>; }` where `Cmd` carries
  the program, args, cwd and an optional full environment.
- `trait Hc` with one method per hc call: `version`, `schema`, `ls(all: bool) -> Result<Vec<Terminal>, Error>`,
  `get(id)`, `run(&RunSpec)`, `send(to, text) -> SendOutcome`, `kill(id)`, and
  `server_start(env: &[(String, String)])`.
- `enum SendOutcome { Sent, Blocked, Finished, NotFound, Failed(String) }` from exits 0, 9, 11, 3 and other
  (spike 2).

### Dependencies

Runtime: `clap` (derive), `clap_complete`, `serde`, `serde_json`, `toml`, `uuid` (v4), `sha2`, `thiserror`.
Dev: `tempfile`. No YAML crate: crook reads one frontmatter line (0024). No date crate: the folder date comes
from `date +%F` with a UTC fallback computed by hand. No semver crate: versions compare as three numbers, and
anything else compares as unknown and leaves the link alone. No locking crate: std `File::lock`.

## Milestones

Tasks are small and ordered: each depends only on tasks above it. Each one ends with `cargo test`,
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` passing. "Tests" names the tests that
prove the task; write them first.

### Milestone 0: skeleton

#### 0.1 Crate and CLI shell

- What: every subcommand from 0020 parses and prints "not implemented" with exit 1. `crook --version` prints
  the package version.
- Files: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/cli.rs`, `src/error.rs`, `tests/cli.rs`.
- ADR: 0019, 0020, 0022.
- Tests: clap's `debug_assert` test on the command; `--version` output; each subcommand's arguments parse
  (`spawn <role> <child> <brief> [--repo <path>] [--model <model>]`, `resume [<child>] [--model <model>]`,
  `doctor [--fix]`, `completion print <shell>`, `hook <event>`, `status --json`).
- Done when: `cargo run -- --help` lists exactly the 0020 table plus `setup` and `doctor`.

#### 0.2 Runner with timeouts

- What: run a command with captured stdout and stderr, an optional full environment, and a deadline after
  which the child is killed and reaped. v1's `run_captured` (`src/launch/real.rs` on `backup_crook_v1`) does
  this and may be copied (0022 allows it, since 0020 asks for bounded hook calls). Add a recording fake runner.
- Files: `src/run.rs`.
- ADR: 0020 (hooks must not hang), 0022.
- Tests: a command that sleeps past its deadline returns a timeout error within the deadline plus a margin;
  output larger than a pipe buffer does not deadlock; a missing program is a clear error.
- Done when: the tests pass on macOS and Linux.

#### 0.3 Context and environment injection

- What: `Ctx` with `Env`, the runner, the clock and the install paths. `main.rs` builds the real one; tests
  build fakes.
- Files: `src/ctx.rs`, `src/main.rs`.
- ADR: 0020 (testable hooks and commands).
- Tests: none of their own; later tasks use it.
- Done when: no module outside `main.rs` calls `std::env::var`.

### Milestone 1: paths, config, names

#### 1.1 XDG paths

- What: one function per row of the paths table.
- Files: `src/paths.rs`.
- ADR: 0004, 0019, 0026.
- Tests: each path with the variable set, unset and empty; `HOME` missing.
- Done when: no other module builds an XDG path.

#### 1.2 Config file

- What: read `config.toml` with `deny_unknown_fields`. One key today, `notes_root`, a path. A leading `~/`
  expands to `HOME`; a relative path is an error. A missing file means every default.
- Files: `src/config.rs`.
- ADR: 0026, 0004.
- Tests: missing file; empty file; `notes_root` absolute, `~/`-relative and relative; an unknown key gives an
  error naming the key; bad TOML gives an error naming the file.
- Done when: every command reads config through this module only.

#### 1.3 Names

- What: validate slugs and child names; build hc names, labels, branch and worktree names, and the herdr space
  and tab ids.
- Files: `src/names.rs`.
- ADR: 0015, 0021, 0023; the hc name rule is open question 2, the length cap open question 19.
- Tests: valid and invalid names (empty, upper case, `/`, `..`, leading `-`, too long); every derived name for
  one example.
- Done when: no other module formats one of these strings.

### Milestone 2: flock files and worktrees

#### 2.1 Atomic write and lock

- What: `write_json_atomic(path, value)` and `with_flock_lock(folder, f)`.
- Files: `src/files/atomic.rs`.
- ADR: 0024.
- Tests: the target is either the old or the new content after a simulated crash between write and rename (the
  temp file is left, the target untouched); two threads each adding a key under the lock both survive.
- Done when: roster and marks code use only these two helpers to write.

#### 2.2 Roster

- What: types for the 0024 shape exactly: `version`, `flock`, `coordinator` (`session_id`, `folder`,
  `state`), `children` (`name`, `role`, `model`, `session_id`, `folder`, `repo`, `branch`, `brief`, `state`).
  `state` is `active` or `done`. `model`, `repo` and `branch` may be `null`. A `version` other than 1 is an
  error that names the file. Operations: load, create, add child, set child state, set child model, set
  coordinator state.
- Files: `src/files/roster.rs`.
- ADR: 0024, 0017, 0025.
- Tests: round trip of the 0024 example; unknown `version`; adding a child whose name exists is an error;
  `brief` stays relative (`briefs/<child>.md`).
- Done when: the roster is only changed through these operations, each under the lock.

#### 2.3 Results and delivery marks

- What: read only the result's frontmatter `status` (`working`, `done`, `failed`) and hash the whole file with
  SHA-256 (proposed, open question 9). A file without a readable frontmatter is `unreadable`; a missing file
  is `missing`. Read and write `results/.delivered.json`, a map from child name to hex SHA-256.
- Files: `src/files/result.rs`, `src/files/marks.rs`.
- ADR: 0024, 0008.
- Tests: the 0024 example parses as `working`; no frontmatter; frontmatter without `status`; an unknown status
  value is `unreadable`; CRLF line ends; the hash of a fixed file; marks round trip; a missing marks file is
  an empty map.
- Done when: nothing else parses result files.

#### 2.4 Flock folders

- What: create a flock folder with `briefs/`, `results/` and the lock file; find a flock by slug; list flocks;
  resolve the notes folder. With `notes_root` set, create `<notes_root>/<folder name>/` and nothing inside it
  (proposed, open question 4). crook never runs git in the notes root (0004).
- Files: `src/flock.rs`.
- ADR: 0004, 0012.
- Tests: create in a temp data home; find by slug; two folders with one slug is an error that names both;
  notes folder with and without `notes_root`.
- Done when: `new` and every flock lookup go through this module.

#### 2.5 Git worktrees

- What: resolve `--repo` to its top level (`git rev-parse --show-toplevel`). Add
  `git worktree add -b crook/<slug>/<child> <repo>/.worktrees/<slug>-<child>` from the current `HEAD`.
  Safety check for removal: no uncommitted or untracked changes (`git status --porcelain`), and the branch tip
  is pushed or merged (definition in open question 13). Remove with `git worktree remove` without `--force`.
  Never delete the branch. Also `is_ignored(repo)` for `.worktrees/`, by `git check-ignore`, for `doctor`.
- Files: `src/git.rs`.
- ADR: 0023.
- Tests: against real `git` in temp folders (git is local and fast): add creates path and branch; an existing
  path or branch refused; dirty, untracked, unpushed each block removal with a reason; pushed to a local bare
  remote allows removal; the branch still exists after removal; ignored and not-ignored `.worktrees/`.
- Done when: `spawn`, `stop`, `close` and `doctor` use only this module for git.

### Milestone 3: install and self-location

#### 3.1 Find the install folder

- What: from `std::env::current_exe()`, canonicalised, take its folder. If `plugin/` is there, that is the
  install folder. If not and this is a debug build (`cfg!(debug_assertions)`), use
  `env!("CARGO_MANIFEST_DIR")`. A release build without `plugin/` next to it stops with "the install is
  broken" and the path it looked in.
- Files: `src/install.rs`.
- ADR: 0019.
- Tests: with a temp folder holding a fake `plugin/`; the debug fallback; the release refusal through an
  injected build flag.
- Done when: `plugin_dir()` and `mod_dir()` are the only way other modules find those folders.

#### 3.2 Version of an install

- What: read the plugin version of a folder from `<folder>/plugin/.claude-plugin/plugin.json`, field
  `version` (proposed, open question 6). Compare versions as three numbers.
- Files: `src/install.rs`.
- ADR: 0019.
- Tests: parse and compare `0.2.0 < 0.10.0`; a pre-release or junk version compares as unknown.
- Done when: the link and the hooks use this to read versions.

#### 3.3 Forward-only `current` link

- What: on every run, before the command, point `<data home>/crook/current` at the install folder, so
  `current/plugin` and `current/mod` resolve (proposed, open question 5). Move it only when the link is
  missing, dangling, or points at a lower version. Swap atomically: make a temp link and rename it over.
  A debug build never moves it (open question 5). Failures warn on stderr and never stop the command.
- Files: `src/install.rs`, `src/main.rs`.
- ADR: 0019, spike 18.
- Tests: missing link; dangling link; older target moves; newer target stays; equal stays; unknown version
  stays; a regular file at the link path is left alone with a warning.
- Done when: `main.rs` calls it once per run, for hooks too.

#### 3.4 `CROOK_BIN`

- What: a helper that returns the `CROOK_BIN` value to pass on: the variable when set; in a debug build with
  no `CROOK_BIN`, its own `current_exe()` (proposed, open question 5).
- Files: `src/install.rs`.
- ADR: 0019, spike 23.
- Tests: set, unset in a release build, unset in a debug build.
- Done when: the launch builder (5.1) takes its value from here.

### Milestone 4: hc client and preflight

#### 4.1 Capture real hc output as fixtures

- What: in a scratch core (as the spikes did: `HC_SESSION` set, scratch `XDG_STATE_HOME` and
  `XDG_RUNTIME_DIR`, folders under `$TMPDIR/crook-e2e-*`), record the stdout, stderr and exit code of:
  `hc --version`, `hc schema`, `hc ls` and `hc ls --all` in their JSON form, `hc get <t>` for a running,
  blocked and finished terminal, `hc run --bg` with labels, `hc send` for each outcome in spike 2, `hc kill`,
  and whatever shows whether a core is running. Check that hc accepts `.` in a name (open question 2). Start
  one `claude --session-id <uuid>` and note where `<uuid>.jsonl` lands under the Claude config folder (open
  question 12). The exact flags for JSON output are not known to this plan; take them from `hc --help` in that
  scratch core.
- Files: `tests/fixtures/hc/<hc version>/*`, with a `README.md` saying how they were captured.
- ADR: 0016, 0020. Depends on area 4's e2e rules for scratch cores.
- Tests: none; this is input for 4.2 to 4.4.
- Done when: each call listed has a fixture, and the two checks above are written down in the README.

#### 4.2 Real client

- What: build argv for each `Hc` method and parse the fixtures' JSON. Parse leniently: unknown fields are
  ignored, missing optional fields are `None`. Every call has a timeout (10 s, one constant). `send` maps exit
  codes to `SendOutcome` (spike 2). `hc` is found on the injected `PATH`.
- Files: `src/hc/mod.rs`, `src/hc/client.rs`.
- ADR: 0007, 0008, 0009, 0014.
- Tests: each fixture parses into the expected `Terminal` (name, id, parent, labels, status state and app);
  argv for each method; a timeout becomes an error, not a hang.
- Done when: no other module runs `hc`.

#### 4.3 Fake hc

- What: an in-memory core: terminals with names, labels, parents and states; records every call; can be told
  to fail a call or return a given `SendOutcome`.
- Files: `src/hc/fake.rs`.
- ADR: 0020 (unit tests for commands and hooks).
- Tests: the fake's own basics (run adds a terminal, kill finishes it, a finished name can be reused, as hc
  allows per 0017).
- Done when: milestones 5 to 8 test against it.

#### 4.4 Environment scrub

- What: a pure filter over the process environment. Remove `CLAUDECODE`, `CLAUDE_PID`, `CLAUDE_EFFORT`, every
  `CLAUDE_CODE_SESSION_*`, `CLAUDE_CODE_CHILD_*`, `CLAUDE_CODE_MESSAGING_*`, `CLAUDE_CODE_ENTRYPOINT`,
  `CLAUDE_CODE_EXECPATH`, `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET`, `HC_SCOPE`. Keep everything else, including
  `CLAUDE_CONFIG_DIR`, other `CLAUDE_CODE_*` settings, `HC_SESSION` and the XDG variables (so scratch cores
  keep working). Used only for `hc server start`.
- Files: `src/hc/scrub.rs`.
- ADR: 0020, spike 23.
- Tests: each removed name and prefix; `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_ENABLE_TELEMETRY`, `HC_SESSION` and
  `PATH` kept.
- Done when: the list lives in one place with a comment pointing at 0020.

#### 4.5 Preflight

- What: check `hc --version` is at least 0.1.22 and `hc schema` has `terminal.run` with `labels` (0016). If no
  core is running, start one with `hc server start` and the scrubbed environment (4.4), then poll until it
  answers, with a deadline. Every command that touches hc runs this first (0020). Hooks follow the reduced
  rule in [Never break the session](#never-break-the-session). 0020 amends 0016: crook scrubs and starts, it
  does not refuse when `CLAUDECODE` is set.
- Files: `src/hc/preflight.rs`, `tests/preflight.rs` (binary-level, fake `hc` script that logs its
  environment).
- ADR: 0016, 0020, spike 23.
- Tests: old version refused with the floor in the message; schema without `labels` refused; core running
  means no start; core not running means `server_start` is the first hc call and later calls follow; a core
  that never comes up times out with a clear message; the fake `hc` script shows `server start` got the
  scrubbed environment.
- Done when: the floor is one constant with a comment pointing at 0016.

### Milestone 5: flock lifecycle

#### 5.1 Launch builder and prompts

- What: a pure function from a launch spec (coordinator or child; new or resume) to the argv in
  [The launch command](#the-launch-command), with `CROOK_BIN` from 3.4. The prompts:
  - coordinator first start: the text from open question 10;
  - child first start: "Read <absolute brief path>. Write your result to <absolute result path>." with the
    reporting rule from 0007 and 0024 (wording agreed with area 2);
  - child resume: 0017's text, "check what is already done on disk, then finish without redoing completed
    work", plus the brief and result paths;
  - coordinator resume: no prompt (open question 10).
- Files: `src/launch.rs`.
- ADR: 0007, 0013, 0017, 0018, 0019, 0025.
- Tests: golden argv for each of the four cases; the token before the prompt is `--session-id <uuid>` or
  `--resume <uuid>`; every `--add-dir` comes before `--agent`; `--plugin-dir` ends in `current/plugin`;
  `--model` present only when given; `env CROOK_BIN=…` present only when 3.4 returns a value; `--no-notify`
  and `--outlive-parent` never appear.
- Done when: no other module assembles a `claude` or `hc run` argv.

#### 5.2 herdr-app renames

- What: `herdr-app do space.rename '{"id":"crook-<slug>","name":"<slug>"}'` and `tab.rename` for
  `crook-<slug>-coord` with the name `coordinator`, JSON argument form as in spike 25. The space may not exist
  yet when `hc run` returns, so retry a few times over about 3 seconds. If the app is not running or every try
  fails, warn on stderr and go on.
- Files: `src/herdr_app.rs`.
- ADR: 0021, spike 25.
- Tests: argv; retry then success; app missing gives a warning and success.
- Done when: `open` calls it after every coordinator start.

#### 5.3 `crook open <slug>`

- What: find the flock; refuse if the roster is `done` (open question 11); preflight; if the coordinator
  already has a live terminal, rename and say so, starting nothing (open question 11); otherwise start it with
  `--session-id` the first time and `--resume` after (0018). How "the first time" is known is open question 12;
  the proposed rule is "no transcript file named `<session id>.jsonl` under the Claude config folder's
  `projects/`". Then rename (5.2). Print one line.
- Files: `src/cmd/open.rs`.
- ADR: 0015, 0018, 0020, 0021.
- Tests: first open uses `--session-id`; a later open uses `--resume`; labels and `--no-parent` present;
  live coordinator starts nothing; closed flock refused; unknown slug refused with the slugs that exist.
- Done when: the mod can call `open` and get exit 0 with one line on stdout.

#### 5.4 `crook new <slug>`

- What: validate the slug; refuse if any flock uses it; create the folder (2.4); write `roster.json` with a
  fresh coordinator session id, `folder` set to the flock folder, `state: active`, no children; then run
  `open` (5.3).
- Files: `src/cmd/new.rs`.
- ADR: 0004, 0012, 0017, 0020, 0024.
- Tests: folder and roster created with today's date from the injected clock; an existing slug refused before
  anything is written; an invalid slug refused; `open` is called.
- Done when: `crook new demo` against the fake hc leaves the 0024 roster shape on disk and one coordinator
  launch recorded.

#### 5.5 `crook close <slug>`

- What: live terminals first (proposed, open question 11: end the coordinator with `hc kill`, which ends its
  children, spike 4). Then mark every child and the coordinator `done` under the lock (0018, 0020). Then remove
  each child's worktree when safe (2.5), and report the ones kept and why.
- Files: `src/cmd/close.rs`.
- ADR: 0018, 0020, 0023.
- Tests: roster states after close; worktree removal tried per child; unsafe worktrees reported and kept;
  closing twice is a no-op with exit 0.
- Done when: a closed flock is skipped by `resume` and refused by `open` and `spawn`.

### Milestone 6: children

#### 6.1 `crook spawn <role> <child> <brief> [--repo <path>] [--model <model>]`

- What: find the flock (shared mechanics); refuse if it is closed; validate the child name and that it is not
  in the roster; the brief must exist and sit under the flock's `briefs/` (stored relative, 0024; the "must sit
  under" rule is open question 19); if `--repo`, create the worktree (2.5) and use it as the folder, else use
  the flock folder (0023); pick a session id; write the roster entry under the lock before launching (0007,
  0017); preflight; launch with `--parent "$HC_TERMINAL_ID"` (0020). If the launch fails, see open question 21.
  Print one line: the child's hc name and session id (format agreed with area 2).
- Files: `src/cmd/spawn.rs`.
- ADR: 0007, 0017, 0020, 0023, 0024, 0025.
- Tests: roster entry written before the `hc run` call (check order in the fake's call log); `model` recorded
  or `null` (open question 14); `--repo` path and branch recorded; duplicate child refused; brief outside
  `briefs/` refused; missing `HC_TERMINAL_ID` without `--flock` refused; two spawns in parallel threads both
  land in the roster; a failed launch handled as open question 21 settles.
- Done when: the spawned argv matches 5.1's golden case.

#### 6.2 `crook stop <child>`

- What: mark the child `done` under the lock first, so a racing resume does not bring it back; then `hc kill`
  its terminal if live; then remove its worktree if safe (2.5), else keep it and say why.
- Files: `src/cmd/stop.rs`.
- ADR: 0020, 0023.
- Tests: order of roster write and kill; not-live child still marked done; unsafe worktree kept with a reason
  on stdout; unknown child refused.
- Done when: a stopped child never appears in a later resume.

#### 6.3 `crook resume [<child>] [--model <model>]`

- What: with no child, every `active` child with no live terminal; with a child, that one. Resume under the
  original hc name (0017, spike 15) with `--resume <session id>`, the roster's role and model, in the roster's
  folder. Parent: `$HC_TERMINAL_ID` when run from the coordinator; otherwise the coordinator's live terminal
  found by label; refuse if it has none. `--model` needs a child; it updates the roster and resumes on that
  model (0025, spike 26). If that child is live, see open question 15. The logic is a library function the
  SessionStart hook calls directly (8.5).
- Files: `src/cmd/resume.rs`.
- ADR: 0017, 0020, 0025.
- Tests: only active and not-live children resumed; done children skipped; original hc name reused; parent
  from env or from label lookup; no live coordinator refused; `--model` written to the roster before launch;
  `--model` with no child refused.
- Done when: one call resumes a flock's lost children against the fake hc, as spike 15 did by hand.

### Milestone 7: status

#### 7.1 `crook status --json [<slug>]`

- What: merge the roster, `hc ls` and the result files and marks. The positional `[<slug>]` and the plain-text
  form without `--json` are additions to 0020's row (proposed, open question 22). No slug means every flock.
  The output shape must be agreed with area 3 (open question 16); proposed:

```json
{
  "version": 1,
  "flocks": [
    {
      "flock": "<slug>",
      "folder": "<abs path>",
      "notes_folder": "<abs path>",
      "state": "active",
      "coordinator": { "session_id": "<uuid>", "terminal": { "id": "t3", "state": "working", "app": "claude-code" } },
      "children": [
        {
          "name": "<child>", "role": "child", "model": "sonnet", "state": "active",
          "folder": "<abs path>", "repo": null, "branch": null, "brief": "<abs path>",
          "terminal": null,
          "result": { "path": "<abs path>", "status": "working", "delivered": true }
        }
      ]
    }
  ]
}
```

- Rules: `terminal` is `null` when hc has no live terminal for it. `result.status` is `working`, `done`,
  `failed`, `unreadable` or `missing` (0024). `delivered` compares the file's hash with its mark. If hc is not
  reachable, the command still prints the file side with `terminal: null` and a top-level `"hc_error"` string,
  so the mod can show something.
- Files: `src/cmd/status.rs`.
- ADR: 0014, 0020, 0024.
- Tests: golden JSON for a flock with a live coordinator, one live child, one paused child, one done child, one
  unreadable result; hc down.
- Done when: area 3 has signed off the shape.

### Milestone 8: hooks

#### 8.1 Hook contract

- What: read stdin JSON (`session_id`, `hook_event_name`, `source`, `cwd`); unknown fields ignored, bad JSON
  means do nothing. Install a panic hook that writes to stderr only; run the event in `catch_unwind`; an
  overall budget (proposed 20 s, below a `hooks.json` timeout of 30 s set by area 2). Always exit 0. Output
  per event: `session-start` prints `hookSpecificOutput` JSON with `additionalContext`, the form v1's
  `src/hook/output.rs` used for SessionStart; `user-prompt-submit` prints the same form with
  `"hookEventName": "UserPromptSubmit"`, which is not checked against the Claude Code hooks reference yet
  (check it in this task); `stop` prints nothing. An unknown event does nothing. An internal test feature
  forces a panic, as v1's `test-panic` feature did; it never ships.
- Files: `src/hook/mod.rs`, `tests/hook.rs`.
- ADR: 0020.
- Tests: binary-level: bad stdin, unknown event, forced panic, `hc` missing from `PATH`, `hc` that hangs past
  the budget: each exits 0 with nothing on stdout and a line on stderr.
- Done when: no path through `crook hook` exits non-zero.

#### 8.2 Session version records and self-reload

- What: on SessionStart, write the plugin version the session loaded, keyed by session id (open question 6).
  On Stop, read the version again through `CLAUDE_PLUGIN_ROOT` (spike 20 did this), or `current/plugin` when
  that is unset; if it differs from the record, run `hc send "$HC_TERMINAL_ID" "/reload-plugins"` and, only on
  `Sent`, update the record (0019, spike 20).
- Files: `src/hook/version.rs`.
- ADR: 0019.
- Tests: same version sends nothing; new version sends once and updates the record; failed send keeps the old
  record so the next Stop tries again; missing record writes one and sends nothing.
- Done when: it runs in every crook session, coordinator and child.

#### 8.3 `crook hook user-prompt-submit`

- What: coordinator only. For each `results/*.md` (skip dot files) whose hash differs from its mark, add a line
  `Undelivered child result: <absolute path>` (spike 14's wording; agreed with area 2) and save the mark under
  the lock. In a child, do nothing. The rescan is a library function that 8.5 also calls.
- Files: `src/hook/prompt.rs`.
- ADR: 0008, 0020.
- Tests: two changed results give two lines and two marks; no change gives no output; a child session does
  nothing; an unreadable result is still reported (hash only).
- Done when: spike 14's last row holds against the fake hc.

#### 8.4 `crook hook stop`

- What: in a child: read `results/<child>.md`; if missing, or its hash equals its mark, stop here (0009 step 1);
  `hc get <parent>` and go on only if its status app is `claude-code` (0009 step 2, 0018); `hc send <parent>
  "<absolute result path>"` (0009 step 3); on `Sent`, write the mark under the lock. Any other outcome leaves
  the mark old for the rescan (0008). Then, in every session, the self-reload (8.2). A failure in the first
  part does not skip the second.
- Files: `src/hook/stop.rs`.
- ADR: 0008, 0009, 0018, 0019, 0020.
- Tests: unchanged result sends nothing; changed result sends and marks; parent not `claude-code` sends nothing
  and keeps the mark; each failing `SendOutcome` keeps the mark; `hc send` timeout keeps the mark; reload still
  runs after a failed send; coordinator Stop does only the reload.
- Done when: spike 14's table holds against the fake hc.

#### 8.5 `crook hook session-start`

- What: write the version record (8.2). In the coordinator only (shared mechanics): resume every `active` child
  with no live terminal (6.3), then the results rescan (8.3), since 0008 says the rescan runs "at start and on
  every turn". Report what was resumed and what is undelivered in `additionalContext`.
- Files: `src/hook/session_start.rs`.
- ADR: 0008, 0017, 0019, 0020.
- Tests: child session writes a record and resumes nothing; coordinator resumes the right children and prints
  them; a resume failure for one child still resumes the others and still exits 0.
- Done when: a restarted coordinator brings its children back with no model involvement.

### Milestone 9: setup and doctor

#### 9.1 `crook setup`

- What: copy the install's `mod/` to `<herdr-app home>/mods/crook/` and write `.crook-version`. If
  `mods/crook/` exists without the marker, stop and say it belongs to someone else. Copy into a temporary
  folder outside `mods/` (so herdr-app never loads a half copy), then swap: rename the old folder aside, rename
  the new one in, delete the old. Whether an older binary may overwrite a newer mod is open question 17. Then
  move `current` (3.3). Create the flocks root. The trust step of 0013 is open question 18; proposed: print the
  one command the maintainer runs to trust each root.
- Files: `src/cmd/setup.rs`.
- ADR: 0013, 0019.
- Tests: fresh install; reinstall over a marked folder; refusal on an unmarked folder; the temp folder never
  appears under `mods/`; `HERDR_APP_HOME` honoured.
- Done when: `crook setup` twice in a row leaves one identical `mods/crook/`.

#### 9.2 `crook doctor [--fix]`

- What: report each check with ok, warn or fail, and exit 1 if any fails:
  - hc version and schema (0016);
  - herdr-app on `PATH` (0019);
  - `crook` on the login shell's `PATH`: `$SHELL -l -c 'command -v crook'` (0019);
  - mod installed and marker equal to this binary's version (0019);
  - `current` points at this binary's install folder (0019);
  - config path read and any error in it (0026);
  - each repository named in an `active` roster has `.worktrees/` ignored (2.5, 0023).
- Fix: `--fix` runs `setup`'s mod install and the link move; the rest are report-only.
- Files: `src/cmd/doctor.rs`.
- ADR: 0016, 0019, 0023, 0026.
- Tests: each check passing and failing with fakes; `--fix` repairs the mod and the link and nothing else.
- Done when: a broken link and a missing mod both show as fail and are fixed by `--fix`.

### Milestone 10: completion

#### 10.1 `crook completion print <shell>`

- What: print a completion script for `bash`, `zsh` and `fish` that completes subcommands, slugs and child
  names. Proposed: clap_complete's dynamic completion (`CompleteEnv` with value candidates for slugs and
  children), with the completer named as bare `crook` so the script mise caches never holds a version path
  that mise later prunes (0019, spike 18). That API sits behind clap_complete's `unstable-dynamic` feature;
  pin the exact version. Not checked against `clap_complete` 4.6.12; check first. If it cannot name the
  completer, fall back to hand-written scripts that call a hidden `crook completion list <kind>`, which is a
  subcommand beyond 0020's table (open question 23).
- Files: `src/cmd/completion.rs`, `src/cli.rs`.
- ADR: 0020.
- Tests: each shell's output contains `crook` and no absolute path to the build; slug candidates come from a
  temp flocks root; child candidates come from the flock's roster.
- Done when: area 4's packslip check passes with the 0020 manifest.

### Milestone 11: hand-off to e2e

#### 11.1 Real-core confirmation

- What: with area 4's e2e skill, run spikes 14 and 15 again through `crook spawn`, `crook hook` and
  `crook resume` (0020 "Confirmation"), and spike 23's environment check through crook's own core start. Also
  check whether the coordinator's rescan reports a result the Stop hook has just delivered (see Risks).
- Files: area 4's e2e skill and scripts; no binary changes expected.
- ADR: 0020.
- Tests: the e2e run itself.
- Done when: both spike tables hold with crook in place of the throwaway hooks.

## Test strategy

- Unit tests, next to each module, cover all logic with injected `Env`, a fake runner and the fake hc. No test
  reads the real XDG folders, the real `current` link, the real herdr-app home or a real core.
- The real hc client is tested against fixtures captured from a scratch core (4.1), not against guessed JSON.
- Git logic is tested with real `git` in temp folders.
- Binary-level tests in `tests/` run the built `crook` with a cleared environment (as v1's
  `tests/rust/common.rs` did) and a fake `hc` script first on `PATH` that logs its argv and environment and
  answers from the fixtures. These cover the hook contract (exit 0, stdout shape), core start with the scrub,
  and the launch argv end to end.
- Left to area 4's e2e runs, because they need real programs:
  - spikes 14 and 15 rerun through crook, and spike 23's scrub (11.1);
  - renames in the real herdr-app (0021), with the app open and closed;
  - self-location through the mise shim in a release install (0019 "Confirmation");
  - an upgrade between two versions: `current` moves, a live session reloads itself (spike 20);
  - `crook setup` in a scratch `HERDR_APP_HOME` and a real `mod.open`;
  - completion through mise.

## Dependencies on other areas

Area 2, the plugin:

- `hooks.json` calls `"${CROOK_BIN:-$HOME/.local/share/mise/shims/crook}" hook <event>` for `session-start`,
  `stop` and `user-prompt-submit`, with a timeout above crook's budget (8.1).
- Agents named `coordinator` and `child`; the coordinator agent has no `model` (0025).
- `.claude-plugin/plugin.json` carries a `version` equal to the crate version (open question 6).
- The coordinator skill uses `crook spawn`, `stop`, `resume` and `status --json`; it expects the result path as
  the message text and the rescan line wording (8.3, 8.4); it carries the brief template (0024).
- `plugin/` sits at the repository root (debug fallback, 3.1).

Area 3, the mod:

- Calls `crook new`, `open`, `close` and `status --json` through the shim, honouring `CROOK_BIN` (0019).
- Agrees the `status --json` shape (7.1) and reads exit code plus stderr for errors.
- `mod/` sits at the repository root; `.crook-version` inside it is reserved for `setup`.

Area 4, release, CI and e2e:

- Archive layout `crook`, `plugin/`, `mod/` (0019); release profile keeps `panic = "unwind"`.
- Version bumps change `Cargo.toml` and `plugin.json` together.
- CI runs `cargo test`, clippy and fmt; coverage as v1.
- The e2e skill allows scratch cores for fixture capture (4.1) and runs the checks in 11.1 and the test
  strategy's e2e list.

## Risks

- **hc output drifts.** hc is 0.1.x (0002). Mitigation: fixtures named by hc version, lenient parsing, the
  preflight floor, and re-capturing fixtures when the floor moves.
- **`hc send` waits on a draft.** It waits for a person's unfinished draft (0008), which can outlast the hook
  budget. A timeout kills the client; whether hc still delivers afterwards is not known. The mark stays old,
  so the worst case is a duplicate report from the rescan, not a lost one.
- **Duplicate report in the same turn.** 0008 sends first and marks after. `hc send` presses Enter, which can
  fire the coordinator's UserPromptSubmit before the child's Stop hook writes the mark, so the rescan may also
  report that result as undelivered. A duplicate, not a loss; 11.1 checks whether it happens.
- **Variadic flags swallow the prompt.** A future flag added to the argv in the wrong place silences a child
  with no error (spike 16). Mitigation: the golden argv tests in 5.1.
- **The scrub list goes stale** as Claude Code changes its variables (0020). Mitigation: one list, and the e2e
  rerun of spike 23 on each Claude Code bump.
- **Renames race space creation** and fail with the app closed (0021). Mitigation: retry, then warn only.
- **Lost updates** on `roster.json` or `.delivered.json` from parallel writers. Mitigation: the flock lock.
- **`clap_complete` dynamic completion is unstable.** Mitigation: pin the version, or fall back (10.1).
- **Self-location through the mise shim** is checked with `python3` only, not crook (0019). The first
  release confirms it.
- **`--resume` vs `--session-id` on `open`** depends on knowing whether the transcript exists (open question
  12). Getting it wrong either fails to start or forks a new conversation.

## Open questions

Each has a suggested answer. The tasks above build on the suggestion and name the question they depend on.

1. **Flock id: slug or dated folder name?** 0004 names the folder `<yyyy-mm-dd>-<slug>`; 0020 takes
   `<slug>`; 0024's roster has `"flock": "<slug>"`. Suggest: the id is the slug, unique across dates; labels,
   branches and worktree names use the slug; `new` refuses a slug already used.
2. **hc names across flocks.** 0007 uses `--name <child>`, which clashes when two flocks each have a child
   called `review` in one core. Suggest: coordinator `<slug>`, child `<slug>.<child>`; check in 4.1 that hc
   accepts `.`.
3. **How a command or hook knows its flock and child.** No ADR says. Suggest: labels `crook.flock` and
   `crook.child` read with `hc get "$HC_TERMINAL_ID"` (spike 14 did this), and a `--flock <slug>` option on
   `spawn`, `stop`, `resume` and `status` for use outside a crook terminal.
4. **Notes folder per flock.** 0004 and 0012 put `README.md` and `notes/` in the notes root but name no
   subfolder. Suggest: `<notes_root>/<yyyy-mm-dd>-<slug>/`, created empty by `new`, passed to sessions with
   `--add-dir`.
5. **What `current` points at, and debug builds.** 0019 says `current` points at "its own `plugin/`" but
   sessions use `current/plugin`. Suggest: `current` points at the install folder, so `current/plugin` and
   `current/mod` both work; it lives under `XDG_DATA_HOME` like the flocks (0019 writes the literal default).
   A debug build never moves it, launches sessions with the repository's `plugin/`, and sets `CROOK_BIN` to
   itself when unset.
6. **Where versions come from and where session records live.** 0019 says the hook "records the plugin
   version" and reads it "through `current`". Suggest: read `version` from
   `$CLAUDE_PLUGIN_ROOT/.claude-plugin/plugin.json` (falling back to `current/plugin`), keep the record in
   `${XDG_STATE_HOME:-~/.local/state}/crook/sessions/<session id>`, and prune records older than 30 days on
   `setup` and `doctor --fix`.
7. **`--add-dir` in the child argv.** 0013 says pass the roots with `--add-dir`; 0007's command shows none.
   Suggest: `--add-dir <flock folder>` and, when set, `--add-dir <notes folder>`, placed before `--agent` so
   the prompt follows `--session-id` or `--resume`.
8. **`CROOK_BIN` for `open`.** 0019 names `spawn` and `resume` only. Suggest: `open` passes it too, so a dev
   coordinator's hooks run the dev build.
9. **What the delivery hash covers.** 0024 says SHA-256 of "the result". Suggest: the whole file's bytes.
10. **The coordinator's first prompt.** 0018 starts it with `'<first task>'`, but `crook new <slug>` takes no
    task. Suggest: a fixed prompt naming the flock, its folder and the notes folder, and telling it to use the
    `crook-coordinator` skill; the dialog's text, if any, goes in an optional `--task <text>` on `new`. On
    `--resume`, no prompt.
11. **`close` with live terminals, and `open` on a closed or live flock.** 0018 and 0020 only say `close`
    marks the roster done. Suggest: `close` ends the coordinator with `hc kill` (its children end with it,
    spike 4) and then marks done; `open` refuses a closed flock; `open` on a live coordinator renames and
    starts nothing.
12. **How `open` knows it is the first start.** 0018 says `--session-id` the first time, `--resume` after.
    Suggest: look for `<session id>.jsonl` under `${CLAUDE_CONFIG_DIR:-~/.claude}/projects/*/`. Not checked
    that Claude Code keeps that layout; 4.1 confirms it.
13. **"Pushed or merged" in 0023.** Suggest: pushed means some remote-tracking branch contains the tip;
    merged means the tip is an ancestor of the branch the repository's main worktree has checked out. A
    branch with no new commits counts as merged.
14. **`spawn` without `--model`.** 0025 says the coordinator picks one. Suggest: the flag is optional,
    the roster records `null`, and the agent file's model applies; the skill always passes it.
15. **`resume <child> --model` on a live child.** 0025 is silent. Suggest: refuse while hc reports it
    `working`; otherwise `hc kill` it, wait until finished, and resume.
16. **The `status --json` shape.** No ADR fixes it. Suggest the shape in 7.1, agreed with area 3.
17. **Mod install direction.** 0019 makes `current` forward-only but says nothing for the mod. Suggest:
    `setup` also refuses to replace a mod whose marker is newer than the binary.
18. **The trust step.** 0013 says "crook's setup step does this trust once"; 0019's `setup` list leaves it
    out. Suggest: `setup` creates the flocks root and prints the command to trust it (and the notes root),
    and `doctor` reports that it cannot check trust.
19. **Name limits and brief location.** No ADR caps name length or says where a brief must be. Suggest: slugs
    and child names at most 40 characters, and `spawn` refuses a brief outside the flock's `briefs/`, so the
    roster's relative `brief` path (0024) always resolves.
20. **Preflight in hooks.** 0020 says every command that touches hc runs preflight. Suggest: hooks skip the
    version and schema checks and never start a core, to save two hc calls per turn; commands run the full
    preflight.
21. **A child whose launch failed.** 0017 and 0020 say `spawn` writes the roster entry before launching, but
    not what happens when `hc run` fails. Suggest: remove the entry under the lock, so the coordinator can
    retry with the same name. The other choice, marking it `done`, keeps a record but blocks the name.
22. **`status` arguments.** 0020's row is `crook status --json` only. Suggest: an optional `<slug>` to limit
    it to one flock, and a short plain-text table when `--json` is absent.
23. **Completion helper.** If clap_complete's dynamic completion cannot be used, 10.1 adds a hidden
    `crook completion list <kind>` beyond 0020's table. Suggest: allow it, hidden from `--help`.
