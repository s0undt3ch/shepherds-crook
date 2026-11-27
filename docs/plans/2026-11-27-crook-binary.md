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

Every command depends on the rules in this section. Where an ADR is silent, the maintainer's answer is in
[`open-questions.md`](open-questions.md) and cited here as Qn.

### Names and ids

- A slug and a child name match `^[a-z0-9]+(-[a-z0-9]+)*$` (Q45). A slug is at most 16 characters and not
  address-shaped (`^t[0-9]` is refused, D36); a child name is at most 15 (D27). hc names are at most 32 characters
  and a child's is `<slug>.<child>`. So they start and end with a letter or digit and have no double dash. They
  end up in paths, branch names, hc names and labels.
- The flock id is the slug (Q2). The folder is `<yyyy-mm-dd>-<slug>` (0004). Slugs are unique across dates and
  across closed flocks: `new` refuses a slug that any folder already uses, and `open` finds the one folder that
  ends in `-<slug>` and whose `roster.json` says `"flock": "<slug>"`.
- hc names (Q3): the coordinator is `<slug>`, a child is `<slug>.<child>`. The roster,
  `briefs/<child>.md` and `results/<child>.md` keep the short child name.
- Labels on every crook terminal: `crook.flock=<slug>`, `crook.version=<version>` (0019, 0020). Children also
  get `crook.child=<child>` (Q4). The coordinator also gets
  `herdr.space=crook-<slug>` and `herdr.tab=crook-<slug>-coord` (0015, 0021). Children get no `herdr.*`
  labels (0021).
- Branch `crook/<slug>/<child>`, worktree `<repo>/.worktrees/<slug>-<child>` (0023).

### Paths

| Name | Path | ADR |
| --- | --- | --- |
| Data home | `${XDG_DATA_HOME:-~/.local/share}` | 0004 |
| Flocks root | `<data home>/crook/flocks/` | 0004 |
| Flock folder | `<flocks root>/<yyyy-mm-dd>-<slug>/` | 0004 |
| Notes folder | `<notes_root>/<yyyy-mm-dd>-<slug>/` when `notes_root` is set, else the flock folder | 0004, 0012, Q18 |
| Config | `${XDG_CONFIG_HOME:-~/.config}/crook/config.toml` | 0026 |
| `current` link | `<data home>/crook/current`, pointing at the install folder | 0019, Q16 |
| Session version records | `${XDG_STATE_HOME:-~/.local/state}/crook/sessions/<session id>` | Q17 |
| herdr-app home | `${HERDR_APP_HOME:-~/.config/herdr-app}` | spike 17 |
| Mod install | `<herdr-app home>/mods/crook/` with `.crook-version` | 0019 |
| mise shim | `${MISE_DATA_DIR:-${XDG_DATA_HOME:-~/.local/share}/mise}/shims/crook` | 0019 |

An empty variable counts as unset, as in the XDG spec. Paths are built by hand, not with a platform crate
(0026). A missing `HOME` resolves under `/`, so writes fail loudly, as v1's `src/paths.rs` did.

Inside a flock folder (0012, 0024): `roster.json`, `briefs/`, `results/`, `results/.delivered.json`, and a
`.lock` file (proposed, see below). With `notes_root` unset, `README.md` and `notes/` live there too.

### Which flock and which session

A command run by the coordinator, or a hook, finds its flock from its own terminal:
`hc get "$HC_TERMINAL_ID"`, label `crook.flock` (spike 14 did this). A child hook finds its child name from
`crook.child`. The coordinator is the terminal with `crook.flock` and no `crook.child`. A maintainer outside
any crook terminal passes `--flock <slug>` (Q4). With neither, the command stops and
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
whether or not the assumption holds (Q18):

```text
hc run --bg --name <hc name> --cwd <folder>
  --label crook.flock=<slug> --label crook.version=<v> [--label crook.child=<child>]
  [--label herdr.space=crook-<slug> --label herdr.tab=crook-<slug>-coord]
  (--parent <coordinator terminal> | --no-parent)
  -- [env CROOK_BIN=<path>]
  claude --plugin-dir <data home>/crook/current/plugin
  --add-dir <flock folder> [--add-dir <notes folder>]
  [--settings <json>]
  --agent crook:<role> [--model <model>]
  (--session-id <uuid> | --resume <uuid>) "<prompt>"
```

- `--plugin-dir` is the literal link path, never the resolved version folder (0019, spike 18). A debug build
  uses the repository's `plugin/` instead (0019, Q16).
- `--no-notify` and `--outlive-parent` are never passed (0007).
- The coordinator is started with `--no-parent`, as in spike 24, so a maintainer's `crook open` from inside
  another terminal does not make that terminal its parent.
- `env CROOK_BIN=<path>` is added when `CROOK_BIN` is set in crook's environment (0019, spike 23). For
  `open` too (Q19).
- The coordinator gets `--agent crook:coordinator` (Q15), and `--model <coordinator_model>` only when that
  config key is set and not empty (0025, 0026, Q32). Otherwise it runs on the maintainer's default model.
- A child gets `--agent crook:child` (Q15) and always a `--model`: the one given to `spawn`, else
  `default_child_model` from the config, `sonnet` when that is unset (0025, Q9).
- Only `crook open` adds `--settings`, with JSON that allows the shim command in Bash, so the coordinator is
  not asked about each `crook` call (Q21). The rule text must match the shim form the skill uses (Q20). Its
  syntax is not checked against Claude Code; the e2e run confirms it.

### Never break the session

`crook hook <event>` always exits 0, writes errors to stderr only, and catches panics (0020). The release profile keeps
`panic = "unwind"`, or `catch_unwind` does nothing (v1's `Cargo.toml` did the same). Every subprocess has a timeout, and
the hook has an overall time budget below the timeout `hooks.json` sets: 120 seconds for `session-start`, 30 for the
other events (Q25, area 2).
A Stop hook never prints a `block` decision.

A hook returns before any hc call when `HC_TERMINAL_ID` is unset. A hook never runs `hc server start`: a
session with `HC_TERMINAL_ID` runs inside a live core. Hooks skip the version and schema checks of preflight,
which would cost two extra hc calls on every turn (Q25).

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
| `src/hook/version.rs` | Session version records, the self-reload and the reload nudge (Q26) |
| `schema/roster.schema.json` | JSON Schema for `roster.json` (Q5) |
| `schema/status.schema.json` | JSON Schema for `crook status --json` (Q5) |
| `tests/schema.rs` | Validates what crook writes against `schema/` |
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
Dev: `tempfile` and a JSON Schema validator such as `jsonschema` (Q5; not checked, pin the version when added).
No YAML crate: crook reads one frontmatter line (0024). No date crate: the folder date comes
from `date +%F` with a UTC fallback computed by hand. No semver crate: versions compare by semantic
versioning precedence (D28), and anything that is not a semantic version compares as unknown and leaves the link
alone. No locking crate: std `File::lock`.

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
  (`new <slug> [--task <text>]` (Q1), `spawn <role> <child> <brief> [--repo <path>] [--model <model>]`,
  `resume [<child>] [--model <model>]`, `doctor [--fix] [--json]` (Q23), `completion print <shell>`,
  `hook <event>`, `status [<slug>] [--json]` (Q5)); `--flock <slug>` on `spawn`, `stop`, `resume` and
  `status` (Q4).
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

- What: read `config.toml` with `deny_unknown_fields`. Three keys: `notes_root`, a path; `default_child_model`,
  a string, `sonnet` when unset (Q9); `coordinator_model`, a string, empty by default, which means the
  maintainer's default model (Q32). A leading `~/` in `notes_root` expands to `HOME`; a relative path is an
  error. A missing file means every default.
- Files: `src/config.rs`.
- ADR: 0026, 0004, 0025.
- Tests: missing file; empty file; `notes_root` absolute, `~/`-relative and relative; `default_child_model`
  unset gives `sonnet`, set gives the value; `coordinator_model` unset and empty both give none; an unknown key
  gives an error naming the key; bad TOML gives an error naming the file.
- Done when: every command reads config through this module only.

#### 1.3 Names

- What: validate slugs and child names; build hc names, labels, branch and worktree names, and the herdr space
  and tab ids.
- Files: `src/names.rs`.
- ADR: 0015, 0021, 0023; the hc name rule is Q3, the length cap Q12, the character rule Q45.
- Tests: valid and invalid names (empty, upper case, `/`, `..`, leading `-`, trailing `-`, `--`; a slug of 17
  characters and a child name of 16 are refused, 16 and 15 are valid; `t7` and `t1x` are refused as slugs), for slugs and
  child names; every derived name for one example.
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

- What: types for the 0024 shape plus one field: `version`, `flock`, `coordinator` (`session_id`, `folder`,
  `state`, `started`), `children` (`name`, `role`, `model`, `session_id`, `folder`, `repo`, `branch`, `brief`,
  `state`). `state` is `active` or `done`. `started` is a boolean, false until the coordinator's first launch
  has succeeded (Q8). `model` is always a string, the model the child was started on (Q9). `repo` and `branch`
  may be `null`. A `version` other than 1 is an error that names the file. Operations: load, create, add child,
  remove child (Q11), set child state, set child model, set coordinator state, set coordinator started.
- Files: `src/files/roster.rs`.
- ADR: 0024, 0017, 0025; Q8, Q9, Q11.
- Tests: round trip of the 0024 example with `started` added; unknown `version`; adding a child whose name
  exists is an error; removing a child frees its name; a `null` model is an error; `brief` stays relative
  (`briefs/<child>.md`).
- Done when: the roster is only changed through these operations, each under the lock.

#### 2.3 Results and delivery marks

- What: read only the result's frontmatter `status` (`working`, `done`, `failed`) and hash the whole file with
  SHA-256 (Q27). A file without a readable frontmatter is `unreadable`; a missing file
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
  (Q18). crook never runs git in the notes root (0004).
- Files: `src/flock.rs`.
- ADR: 0004, 0012.
- Tests: create in a temp data home; find by slug; two folders with one slug is an error that names both;
  notes folder with and without `notes_root`.
- Done when: `new` and every flock lookup go through this module.

#### 2.5 Git worktrees

- What: resolve `--repo` to its top level (`git rev-parse --show-toplevel`). Add `git worktree add -b
  crook/<slug>/<child> <repo>/.worktrees/<slug>-<child>` from the current `HEAD`. Safety check for removal: no
  uncommitted or untracked changes (`git status --porcelain`), and the branch tip is pushed (some remote-tracking branch
  contains it) or merged (it is an ancestor of the branch the main worktree has checked out; a branch with no new
  commits counts as merged) (Q13). Remove with `git worktree remove` without `--force`. Never delete the branch. Also
  `is_ignored(repo)` for `.worktrees/`, by `git check-ignore`, for `doctor`.
- Files: `src/git.rs`.
- ADR: 0023.
- Tests: against real `git` in temp folders (git is local and fast): add creates path and branch; an existing path or
  branch refused; dirty, untracked, unpushed each block removal with a reason; pushed to a local bare remote allows
  removal; merged into the main branch allows removal; a branch with no new commits allows it; the branch still exists
  after removal; ignored and not-ignored `.worktrees/`.
- Done when: `spawn`, `stop`, `close` and `doctor` use only this module for git.

#### 2.6 `roster.json` schema

- What: `schema/roster.schema.json`, a JSON Schema for the 2.2 shape, committed to the repository (Q5). `version`
  is 1; every field is required; `state` is `active` or `done`; `model` is a non-empty string; `repo` and
  `branch` are a string or `null`; unknown fields are refused. Area 3's tests validate against the same file.
- Files: `schema/roster.schema.json`, `tests/schema.rs`.
- ADR: 0024; Q5.
- Tests: the 0024 example with `started` added validates; a roster written by each 2.2 operation validates; a
  `null` model, an unknown `state`, an unknown field and a missing `started` each fail.
- Done when: the tests that write a roster (5.4, 6.1) check it with the shared schema helper.

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
  `version`. The release stamps it; in the repository, and so in debug builds, it reads `0.0.0-dev` (Q17).
  Compare versions by semantic versioning precedence (D28): `MAJOR.MINOR.PATCH[-PRE][+BUILD]`, build metadata
  ignored, a release above its pre-releases, pre-release identifiers dot by dot.
- Files: `src/install.rs`.
- ADR: 0019.
- Tests: parse and compare `0.2.0 < 0.10.0` and `0.1.0-rc.1 < 0.1.0-rc.2 < 0.1.0-rc.10 < 0.1.0 < 0.1.1-rc.1`; a
  junk version compares as unknown.
- Done when: the link and the hooks use this to read versions.

#### 3.3 Forward-only `current` link

- What: on every run, before the command, point `<data home>/crook/current` at the install folder, so
  `current/plugin` and `current/mod` resolve (Q16). Move it only when the link is
  missing, dangling, or points at a lower version. Swap atomically: make a temp link and rename it over, holding
  a lock on `current.lock` for the read, compare and rename. A debug build never moves it (0019, Q16). Failures
  warn on stderr and never stop the command.
- Files: `src/install.rs`, `src/main.rs`.
- ADR: 0019, spike 18.
- Tests: missing link; dangling link; older target moves; newer target stays; equal stays; unknown version
  stays; a regular file at the link path is left alone with a warning.
- Done when: `main.rs` calls it once per run, for hooks too.

#### 3.4 `CROOK_BIN`

- What: a helper that returns the `CROOK_BIN` value to pass on: the variable when set; in a debug build with
  no `CROOK_BIN`, its own `current_exe()` (0019: a debug build is started with `CROOK_BIN` set to itself).
- Files: `src/install.rs`.
- ADR: 0019, spike 23.
- Tests: set, unset in a release build, unset in a debug build.
- Done when: the launch builder (5.1) takes its value from here.

### Milestone 4: hc client and preflight

#### 4.1 Capture real hc output as fixtures

- What: in a scratch core (as the spikes did: `HC_SESSION` set, scratch `XDG_STATE_HOME` and `XDG_RUNTIME_DIR`, folders
  under `$TMPDIR/crook-e2e-*`), record the stdout, stderr and exit code of: `hc --version`, `hc schema`, `hc ls` and `hc
  ls --all` in their JSON form, `hc get <t>` for a running, blocked and finished terminal, `hc run --bg` with labels,
  `hc send` for each outcome in spike 2, `hc kill`, and whatever shows whether a core is running. Check that hc accepts
  `.` in a name (Q3). The exact flags for JSON output are not known to this plan; take them from `hc --help` in that
  scratch core.
- Files: `tests/fixtures/hc/<hc version>/*`, with a `README.md` saying how they were captured.
- ADR: 0016, 0020. Depends on area 4's e2e rules for scratch cores.
- Tests: none; this is input for 4.2 to 4.4.
- Done when: each call listed has a fixture, and the check on `.` in names is written down in the README.

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

- What: check `hc --version` is at least 0.1.22, which starts no core. Then always run `hc server start` with the
  scrubbed environment (4.4): every other hc call starts a core with the caller's environment when none runs, and
  `server start` on a running core only reports it (D29). Then check `hc schema` has `terminal.run` with
  `labels` (0016), polling with a deadline and repeating `server start` before each retry. Every command that
  touches hc runs this first (0020). Hooks follow the reduced rule in
  [Never break the session](#never-break-the-session). 0020 amends 0016: crook scrubs and starts, it does not
  refuse when `CLAUDECODE` is set.
- Files: `src/hc/preflight.rs`, `tests/preflight.rs` (library-level, fake `hc` script that logs its calls and
  saves the environment its core started with).
- ADR: 0016, 0020, spike 23.
- Tests: old version refused with the floor in the message; schema without `labels` refused; `server_start`
  is the first hc call after `--version`, whether or not a core runs, and precedes every `schema` retry; a core
  that never comes up times out with a clear message; the fake `hc` script shows the core only ever started from
  the scrubbed environment, even when it stops between two `schema` calls.
- Done when: the floor is one constant with a comment pointing at 0016.

### Milestone 5: flock lifecycle

#### 5.1 Launch builder and prompts

- What: a pure function from a launch spec (coordinator or child; new or resume) to the argv in
  [The launch command](#the-launch-command), with `CROOK_BIN` from 3.4. The prompts:
  - coordinator first start: a fixed text that names the flock, its folder and the notes folder and points at
    the `crook-coordinator` skill, followed by the `--task` text when `new` got one (Q1);
  - child first start: "Read <absolute brief path>. Write your result to <absolute result path>." with the
    reporting rule from 0007 and 0024 (wording agreed with area 2) (Q14);
  - child resume: 0017's text, "check what is already done on disk, then finish without redoing completed
    work", plus the brief and result paths (Q14);
  - coordinator resume: no prompt (0018).
- Files: `src/launch.rs`.
- ADR: 0007, 0013, 0017, 0018, 0019, 0025; Q1, Q14, Q15, Q21, Q32.
- Tests: golden argv for each of the four cases; the token before the prompt is `--session-id <uuid>` or
  `--resume <uuid>`; every `--add-dir` comes before `--agent`; `--plugin-dir` ends in `current/plugin`;
  `--agent` is `crook:coordinator` or `crook:child`; a child always has `--model`, the coordinator only when
  `coordinator_model` is set; `--settings` appears for `open` only; `env CROOK_BIN=…` present only when 3.4
  returns a value; `--no-notify` and `--outlive-parent` never appear.
- Done when: no other module assembles a `claude` or `hc run` argv.

#### 5.2 herdr-app renames

- What: `herdr-app do space.rename '{"id":"crook-<slug>","name":"<slug>"}'` and `tab.rename` for
  `crook-<slug>-coord` with the name `coordinator`, JSON argument form as in spike 25. The space may not exist
  yet when `hc run` returns, so retry a few times over about 3 seconds. If the app is not running or every try
  fails, warn on stderr and go on. crook never calls `space.new` (0015, Q24).
- Files: `src/herdr_app.rs`.
- ADR: 0021, spike 25.
- Tests: argv; retry then success; app missing gives a warning and success.
- Done when: `open` calls it after every coordinator start.

#### 5.3 `crook open <slug>`

- What: find the flock; refuse if the roster is `done` (Q7); preflight; if the coordinator already has a live
  terminal, redo the renames and say so, starting nothing (Q7); otherwise start it with `--session-id` while
  the roster's `coordinator.started` is false and `--resume` after (0018, Q8). Set `started` under the lock as
  soon as `hc run` succeeds; a failed launch leaves it false. Then rename (5.2). Print one line. `open` takes
  the first task from `new` when `new` calls it (Q1).
- Files: `src/cmd/open.rs`.
- ADR: 0015, 0018, 0020, 0021; Q1, Q7, Q8, Q19, Q21.
- Tests: first open uses `--session-id` and sets `started`; a later open uses `--resume`; a failed launch
  leaves `started` false; `CROOK_BIN` and `--settings` present; labels and `--no-parent` present;
  live coordinator starts nothing; closed flock refused; unknown slug refused with the slugs that exist.
- Done when: the mod can call `open` and get exit 0 with one line on stdout.

#### 5.4 `crook new <slug> [--task <text>]`

- What: validate the slug; refuse if any flock uses it, closed ones included (Q2); create the folder and the
  notes folder (2.4); write `roster.json` with a fresh coordinator session id, `folder` set to the flock
  folder, `state: active`, `started: false`, no children; then run `open` (5.3), passing `--task` on (Q1).
- Files: `src/cmd/new.rs`.
- ADR: 0004, 0012, 0017, 0020, 0024; Q1, Q2.
- Tests: folder and roster created with today's date from the injected clock; an existing slug, closed
  flocks included, refused before anything is written; an invalid slug refused; `open` is called; `--task` text
  reaches the coordinator's first prompt, and without it only the fixed text does.
- Done when: `crook new demo` against the fake hc leaves the 0024 roster shape on disk and one coordinator
  launch recorded.

#### 5.5 `crook close <slug>`

- What: end each live child with `hc kill` first, so no worktree is in use. Then mark every child and the
  coordinator `done` under the lock (0018, 0020). Then remove each child's worktree when safe (2.5), and print
  a `warning:` line for each worktree it keeps, with the reason (Q6). End the coordinator with `hc kill` last:
  the coordinator usually runs `close` itself, from a process inside its own terminal, so everything above
  must be finished and printed before that terminal ends (D33). The
  coordinator skill runs `close` only when the maintainer asks (Q6, area 2). On a flock that is already closed,
  `close` still exits 0 and kills nothing, but it retries removing any worktree kept earlier, with the same
  rule (clean, and pushed or merged: 0023, 2.5), and prints the same `warning:` line for each one it still
  keeps (Q44).
- Files: `src/cmd/close.rs`.
- ADR: 0018, 0020, 0023; Q6, Q44.
- Tests: roster states after close; worktree removal tried per child; unsafe worktrees kept, each with a
  `warning:` line; a second `close` after the worktree was pushed removes it and exits 0; a second `close` with
  the worktree still dirty prints the `warning:` line again and exits 0; a second `close` with nothing kept
  prints nothing and exits 0.
- Done when: a closed flock is skipped by `resume` and refused by `open` and `spawn`.

### Milestone 6: children

#### 6.1 `crook spawn <role> <child> <brief> [--repo <path>] [--model <model>]`

- What: find the flock (shared mechanics); refuse if it is closed; validate the child name and that it is not
  in the roster; the brief must exist and sit under the flock's `briefs/` (stored relative, 0024; Q12); if
  `--repo`, create the worktree (2.5) and use it as the folder, else use the flock folder (0023); pick a
  session id; resolve the model: `--model`, else `default_child_model`, else `sonnet` (Q9); write the roster
  entry, with that model, under the lock before launching (0007, 0017); preflight; launch with
  `--parent "$HC_TERMINAL_ID"` (0020). The first prompt names the brief and the result path (Q14). If the launch
  fails, remove the roster entry under the lock, and the worktree if clean, so the coordinator can retry with
  the same name (Q11). Print one line: the child's hc name and session id (format agreed with area 2).
- Files: `src/cmd/spawn.rs`.
- ADR: 0007, 0017, 0020, 0023, 0024, 0025; Q9, Q11, Q12, Q14.
- Tests: roster entry written before the `hc run` call (check order in the fake's call log); `model` recorded as the
  flag's value, the config's, or `sonnet`, never `null` (Q9); `--repo` path and branch recorded; duplicate child
  refused; brief outside `briefs/` refused; missing `HC_TERMINAL_ID` without `--flock` refused; two spawns in parallel
  threads both land in the roster; a failed launch removes the entry, so the same name can be spawned again (Q11).
- Done when: the spawned argv matches 5.1's golden case.

#### 6.2 `crook stop <child>`

- What: mark the child `done` under the lock first, so a racing resume does not bring it back; then `hc kill`
  its terminal if live; then remove its worktree if safe (2.5), else keep it and print a `warning:` line with
  the reason, as `close` does (Q6). On a child that is already `done`, `stop` kills nothing but still retries
  removing a worktree kept earlier, with the same rule (clean, and pushed or merged: 0023), and prints the same
  `warning:` line if it is still kept (Q44).
- Files: `src/cmd/stop.rs`.
- ADR: 0020, 0023; Q6, Q44.
- Tests: order of roster write and kill; not-live child still marked done; unsafe worktree kept with a
  `warning:` line; unknown child refused; a second `stop` on a `done` child removes a worktree that is now
  pushed, and prints the `warning:` line again for one that is still dirty.
- Done when: a stopped child never appears in a later resume.

#### 6.3 `crook resume [<child>] [--model <model>]`

- What: with no child, every `active` child with no live terminal; with a child, that one. Resume under the original hc
  name (0017, spike 15) with `--resume <session id>`, the roster's role and model (Q9: the roster's model, even if the
  config has since changed), in the roster's folder. Parent: `$HC_TERMINAL_ID` when run from the coordinator; otherwise
  the coordinator's live terminal found by label; refuse if it has none. `--model` needs a child; it updates the roster
  and resumes on that model (0025, spike 26). If that child is live, refuse while hc reports it `working`; otherwise end
  its terminal with `hc kill` without marking it `done`, wait until it is finished, and resume it on the new model
  (Q10). The logic is a library function the SessionStart hook calls directly (8.5).
- Files: `src/cmd/resume.rs`.
- ADR: 0017, 0020, 0025; Q9, Q10.
- Tests: only active and not-live children resumed; done children skipped; original hc name reused; parent
  from env or from label lookup; no live coordinator refused; `--model` written to the roster before launch;
  `--model` with no child refused; `--model` on a working child refused; on an idle live child it is killed,
  not marked `done`, and resumed on the new model.
- Done when: one call resumes a flock's lost children against the fake hc, as spike 15 did by hand.

### Milestone 7: status

#### 7.1 `crook status [<slug>] [--json]`

- What: merge the roster, `hc ls` and the result files and marks. No slug means every flock; with a slug, that
  flock and its children. Without `--json` it prints a plain table (Q5). The JSON has the shape below, pinned
  by the schema in 7.2 (Q5); the field names are still to settle with area 3 (see Open questions):

```json
{
  "version": 1,
  "crook_version": "0.1.0",
  "flocks": [
    {
      "slug": "<slug>",
      "space": "crook-<slug>",
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
- ADR: 0014, 0020, 0024; Q5.
- Tests: golden JSON for a flock with a live coordinator, one live child, one paused child, one done child, one
  unreadable result; hc down; the plain table for the same flock; a slug limits the output to that flock.
- Done when: the golden JSON is the input for 7.2.

#### 7.2 `status --json` schema

- What: `schema/status.schema.json`, a JSON Schema for 7.1's output, committed to the repository (Q5). It
  requires `version` 1, `crook_version` and `flocks`; `model` is a string; `terminal` is an object or `null`;
  `result.status` is one of the five values; `hc_error` is an optional string. Area 3's tests validate against
  the same file.
- Files: `schema/status.schema.json`, `tests/schema.rs`.
- ADR: 0020, 0024; Q5.
- Tests: each golden JSON from 7.1 validates, hc-down output included; a missing required field, an unknown
  `result.status` and a `null` model each fail.
- Done when: the schema is committed and area 3 has the path.

### Milestone 8: hooks

#### 8.1 Hook contract

- What: read stdin JSON (`session_id`, `hook_event_name`, `source`, `cwd`); unknown fields ignored, bad JSON
  means do nothing. Install a panic hook that writes to stderr only; run the event in `catch_unwind`; an
  overall budget below the timeout `hooks.json` sets (Q25): 100 s for `session-start`, whose timeout is 120 s,
  and 20 s for the others, whose timeout is 30 s. Always exit 0. Output
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

- What: on SessionStart, write the plugin version the session loaded, keyed by session id, under
  `${XDG_STATE_HOME:-~/.local/state}/crook/sessions/` (Q17). On Stop, read the version again through
  `CLAUDE_PLUGIN_ROOT` (spike 20 did this; Q17), or `current/plugin` when that is unset; if it differs from the
  record, run `hc send "$HC_TERMINAL_ID" "/reload-plugins"` and, only on `Sent`, update the record and set its
  `reloaded` flag, which 8.3 turns into a nudge (0019, spike 20, Q26). Records older than 30 days are pruned
  on `setup` and `doctor --fix` (Q17).
- Files: `src/hook/version.rs`.
- ADR: 0019; Q17, Q26.
- Tests: same version sends nothing; new version sends once, updates the record and sets `reloaded`; failed
  send keeps the old record so the next Stop tries again; missing record writes one and sends nothing; a record
  older than 30 days is pruned, a newer one kept.
- Done when: it runs in every crook session, coordinator and child.

#### 8.3 `crook hook user-prompt-submit`

- What: in the coordinator, for each `results/*.md` (skip dot files) whose hash differs from its mark, add a
  line `Undelivered child result: <absolute path>` (spike 14's wording; agreed with area 2) and save the mark
  under the lock. In a child, add no result lines. In every session, when the version record has `reloaded`
  set (8.2), add one line telling the model to load its `crook-coordinator` or `crook-child` skill again,
  because a reload leaves the old skill text in the conversation, and clear the flag (Q26). The rescan is a
  library function that 8.5 also calls.
- Files: `src/hook/prompt.rs`.
- ADR: 0008, 0019, 0020; Q26.
- Tests: two changed results give two lines and two marks; no change gives no output; a child session gets no
  result lines; an unreadable result is still reported (hash only); after a reload the first prompt in a
  coordinator and in a child gets the nudge line, and the second prompt does not.
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

- What: install the install's `mod/` as `<herdr-app home>/mods/crook/`, with `HERDR_APP_HOME` honoured (Q22).
  Build it in a staging folder outside `mods/` (so herdr-app never loads a half copy) and write `.crook-version`
  last. If `mods/crook/` exists without the marker, stop and say it belongs to someone else. If its marker is
  newer than this binary's version, refuse to replace it (Q22). Then swap: rename the old folder aside, rename
  the staged one in, delete the old. No other command refreshes the mod by itself (Q22); `doctor --fix` calls
  this routine. Then move `current` (3.3). Create the flocks root. For the trust step of 0013, print the
  command that trusts each root (the flocks root, and the notes root when set) and run none, so CI can skip
  it (Q22). Prune the per-session version records older than 30 days under `XDG_STATE_HOME` (8.2; Q17, Q41).
- Files: `src/cmd/setup.rs`.
- ADR: 0013, 0019; Q17, Q22, Q41.
- Tests: fresh install; reinstall over a marked folder; refusal on an unmarked folder; refusal on a newer
  marker; the staged folder has no marker until the copy ends; the staging folder never appears under `mods/`;
  `HERDR_APP_HOME` honoured; one trust command printed per root and none run; a session record older than 30
  days is pruned and a newer one kept.
- Done when: `crook setup` twice in a row leaves one identical `mods/crook/`.

#### 9.2 `crook doctor [--fix]`

- What: report each check with ok, warn or fail, and exit 1 if any fails. With `--json`, print one JSON object
  with each check, its status and its fix, and the resolved `plugin_dir`, `mod_dir` and herdr-app home (Q23).
  The checks:
  - hc version and schema (0016);
  - herdr-app on `PATH` (0019);
  - `crook` on the login shell's `PATH`: `$SHELL -l -c 'command -v crook'` (0019);
  - the mise shim exists; when it is missing, warn and give `CROOK_BIN` as the fix (Q23);
  - mod installed and marker equal to this binary's version (0019);
  - `current` points at this binary's install folder (0019);
  - config path read and any error in it (0026);
  - each repository named in an `active` roster has `.worktrees/` ignored (2.5, 0023).
- Fix: `--fix` runs `setup`'s mod install and the link move, and prunes session records older than 30 days
  under `XDG_STATE_HOME` (Q17, Q41); the rest are report-only.
- Files: `src/cmd/doctor.rs`.
- ADR: 0016, 0019, 0023, 0026; Q17, Q22, Q23, Q41.
- Tests: each check passing and failing with fakes; `--json` carries the three resolved paths; a missing shim
  warns with `CROOK_BIN` as the fix; `--fix` repairs the mod and the link, prunes session records older than 30
  days (a newer one stays) and changes nothing else.
- Done when: a broken link and a missing mod both show as fail and are fixed by `--fix`.

### Milestone 10: completion

#### 10.1 `crook completion print <shell>`

- What: print a completion script for `bash`, `zsh` and `fish` that completes subcommands, slugs and child
  names. Proposed: clap_complete's dynamic completion (`CompleteEnv` with value candidates for slugs and
  children), with the completer named as bare `crook` so the script mise caches never holds a version path
  that mise later prunes (0019, spike 18). That API sits behind clap_complete's `unstable-dynamic` feature;
  pin the exact version. Not checked against `clap_complete` 4.6.12; check first. If it cannot name the
  completer, fall back to hand-written scripts that call a hidden `crook completion list <kind>`, a
  subcommand beyond 0020's table that `--help` does not show (Q35).
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
- Left to area 4's e2e runs, because they need real programs; CI cannot run hc or herdr-app, so these run
  locally only (Q34):
  - spikes 14 and 15 rerun through crook, and spike 23's scrub (11.1);
  - renames in the real herdr-app (0021), with the app open and closed;
  - self-location through the mise shim in a release install (0019 "Confirmation");
  - an upgrade between two versions: `current` moves, a live session reloads itself (spike 20);
  - `crook setup` in a scratch `HERDR_APP_HOME` and a real `mod.open`;
  - completion through mise.

## Dependencies on other areas

Area 2, the plugin:

- `hooks.json` calls
  `"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" hook <event>`
  for `session-start`, `stop` and `user-prompt-submit`, with timeouts of 120 seconds for `session-start` and 30 for
  the others (Q25), each above crook's
  budget (8.1).
- Agents named `coordinator` and `child`, passed as `crook:coordinator` and `crook:child` (Q15); the
  coordinator agent has no `model` (0025).
- `.claude-plugin/plugin.json` carries `version`: `0.0.0-dev` in the repository, stamped by the release (Q17).
- The coordinator skill uses `crook spawn`, `stop`, `resume` and `status --json`, always through the shim form and
  never a bare `crook` (Q20), and runs `close` only when the maintainer asks (Q6); it expects the result path as
  the message text and the rescan line wording (8.3, 8.4); it carries the brief template (0024).
- `plugin/` sits at the repository root (debug fallback, 3.1).

Area 3, the mod:

- Calls `crook new <slug> [--task <text>]`, `open`, `close` and `status --json` through the shim, honouring `CROOK_BIN`
  (0019).
- Validates against `schema/status.schema.json` and `schema/roster.schema.json` in its tests (Q5), and reads
  exit code plus stderr for errors.
- `mod/` sits at the repository root; `.crook-version` inside it is reserved for `setup`.

Area 4, release, CI and e2e:

- Archive layout `crook`, `plugin/`, `mod/` (0019); release profile keeps `panic = "unwind"`.
- The release stamps `version` in `plugin.json` (Q17).
- CI runs `cargo test`, clippy and fmt; coverage as v1.
- The e2e skill allows scratch cores for fixture capture (4.1) and runs the checks in 11.1 and the test
  strategy's e2e list.

## Risks

- **hc output drifts.** hc is 0.1.x (0002). Mitigation: fixtures named by hc version, lenient parsing, the
  preflight floor, and re-capturing fixtures when the floor moves.
- **`hc send` waits on a draft** (Q25). It waits for a person's unfinished draft (0008), which can outlast the hook
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
- **A crash between launch and the `started` write** leaves `started` false for a coordinator that exists, so
  the next `open` passes `--session-id` for a session that is already there (Q8). What Claude Code does then
  is not checked. Mitigation: the write follows `hc run` at once, under the lock.

## Open questions

The plan's earlier questions are decided by the maintainer. The answers are in
[`open-questions.md`](open-questions.md); the tasks above cite them as Qn.

- Flock id: decided, see Q2.
- hc names across flocks: decided, see Q3.
- How a command or hook knows its flock and child: decided, see Q4.
- Notes folder per flock: decided, see Q18.
- What `current` points at, and debug builds: decided, see Q16.
- Where versions come from and where session records live: decided, see Q17.
- `--add-dir` in the child argv: decided, see Q18.
- `CROOK_BIN` for `open`: decided, see Q19.
- What the delivery hash covers: decided, see Q27.
- The coordinator's first prompt: decided, see Q1.
- `close` with live terminals, and `open` on a closed or live flock: decided, see Q6 and Q7.
- How `open` knows it is the first start: decided, see Q8.
- "Pushed or merged": decided, see Q13.
- `spawn` without `--model`: decided, see Q9.
- `resume <child> --model` on a live child: decided, see Q10.
- Mod install direction: decided, see Q22.
- The trust step: decided, see Q22.
- Name limits and brief location: decided, see Q12.
- Preflight in hooks: decided, see Q25.
- A child whose launch failed: decided, see Q11.
- `status` arguments: decided, see Q5.
- Completion helper: decided, see Q35.

The field names of the `status --json` shape are settled across both plans: `slug`, `space`, `crook_version`,
and `folder` as an absolute path, whose last part the mod shows.
