# crook Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build crook v1. One orchestrator Claude Code session per project
starts, tasks and tracks child sessions in other repos, routes their questions
to the user, and asks before private material leaves through a tool call.

**Architecture:** One Rust package builds one binary, `crook`, with the
spec's three entry points: user verbs (clap), mod verbs (`crook mod <verb>`)
and the settings hook (`crook hook <event>`). Only this binary opens the
per-project SQLite `crook.db`. A TypeScript mod, embedded in the binary and
unpacked at launch, is loaded with `--plugin-dir`. It registers the session
tools, watches the inbox, filters parked prompts and keeps the orchestrator's
status line. It reaches state only by running `crook mod <verb>` through
`$.process.run`. Repo automation (packslip gate, release bump, pre-commit
checks) is toolr commands in `tools/`, as in toolr itself.

**Tech Stack:**

- Rust, edition 2024, toolchain pinned in `mise.toml` (toolr pins 1.99.1 with
  `profile = "minimal"` and `components = "clippy,rustfmt"`; copy that)
- `rusqlite` with the `bundled` feature, `serde` + `serde_json`, `toml`,
  `globset`, `clap` + `clap_complete`, `include_dir`, `uuid` (v4),
  `thiserror`, `anyhow` (CLI only), `bitflags`, `nix` (`signal` feature)
- dev: `cargo-nextest`, `cargo-llvm-cov` (coverage, uploaded to Codecov as
  toolr does), `tempfile`, `assert_cmd`
- TypeScript for the mod, against Claude Code 2.1.329's mod API. Its logic
  is also a private npm package at the repo root, tested with `vitest` and
  measured with `@vitest/coverage-v8`
- repo tooling, copied in shape from toolr: mise (with `mise.lock`), prek,
  toolr with Python commands in `tools/`, git-cliff, Renovate, packslip
- herdr 0.9.5, optional

**Spec:** [2026-11-19-crook-design.md](2026-11-19-crook-design.md), draft 8.7.
Executors read both documents. Section names in this plan refer to the spec.

**Calls this plan makes** (the spec leaves them to the builder):

- **Rust** for the binary and the hook. It gives one static `CROOK_BIN` and
  fast start-up, which matters because the hook runs on every tool call.
- **One binary, not two.** The spec's Pieces section says "`crook`, one
  Rust binary with three entry points". `main` routes `argv[1] == "hook"` and
  `argv[1] == "mod"` before clap, because clap exits 2 on a bad argument, and
  with `onFailure: "block"` that would block every tool call. One binary also
  means one packslip `bin` entry and one `CROOK_BIN`.
- **One package, no workspace.** `Cargo.toml` at the root, `src/lib.rs` and
  `src/main.rs`. Repo automation lives in toolr's `tools/`, so there is no
  `xtask` crate to need a workspace.
- **The mod's logic is plain TypeScript with no `$`.** A mod may import other
  files of its own plugin with `import` declarations (`reference.md:14` in the
  plugin-authoring skill), but nothing says it may import npm packages, and
  `claude plugin test` reports no coverage. So:
  - `plugin/hooks/core/*.ts` holds every decision (watcher, park, origin,
    orchestrator, verb results) as functions over a `Ports` interface. It
    imports nothing but other `core/` files, and `claude-code` types only
    with `import type`.
  - `plugin/hooks/register.ts` and `plugin/hooks/adapter.ts` are thin: they
    build `Ports` from `$` and wire hooks to `core/`.
  - `core/` is unit-tested by `vitest` in `*.spec.ts` beside each module, with
    coverage. The adapter is tested by `claude plugin test` in `*.test.ts`.
    The two suffixes keep the two runners apart.
  - `package.json` (private, `crook-mod`), `package-lock.json` and
    `vitest.config.ts` sit at the repo root, so `node_modules/` never lands
    in `plugin/`.
- **The mod is unpacked to `${XDG_DATA_HOME:-~/.local/share}/crook-plugin/<version>-<hash>/`**,
  outside the `crook/` state dir, so the state-dir Read deny can't touch it.
- **macOS and Linux only.** crook uses `exec`, `kill(pid, 0)`, the parent pid
  and herdr. Release archives: `x86_64`/`aarch64` Linux musl and
  `x86_64`/`aarch64` macOS. No Windows.
- **The code lives in this repo**, package name `crook`, repository
  `https://github.com/s0undt3ch/shepherds-crook`. Pushes, tags and release
  dispatches are confirm-first.
- **Minimum chunk length:** 8 words.
- **No `PostToolUse` hook.** `asks` logs hits only, not the user's answer
  (user's call, 2026-11-20): a denied call never reaches `PostToolUse`, so it
  could only ever record "allowed", at one extra process per tool call.
- **No Bash exemption** (user's call, 2026-11-20). Bash and Monitor are
  checked like every other tool: a command that names a private path or
  quotes private text asks. No shell lexer or parser. The protocol skill
  tells sessions to read notes with `Read`, `Grep` and `Glob` and change
  them with `Write` and `Edit`, which never ask.
- **Caps:** a brief is at most 4000 characters; at most 8 live children.

**Suggested models** (the cheapest model that does the job): Sonnet for
most tasks. Opus for Tasks 16–17 (watcher and park interplay). Fable only for a security review of Milestone C before Milestone
D starts, because it is the privacy boundary.

## Global Constraints

- **Open source.**
  - Nothing in this repo names or links private material: the author's notes
    or vault by name, the author's employer, or any repo that isn't public.
    That covers code, docs, commit messages and PR text.
  - The private words themselves never go in the repo either. Task 21's
    `private-words` check reads them from a file outside the repo.
  - Borrowed ideas are credited by name and public link: herdr-quartermaster,
    bermuda, [loam](https://github.com/GregorMcC/loam). claude-snooze is
    credited by name only, as "a private Claude Code plugin", with no link
    and no org. No code is copied from any of them. toolr is the author's own,
    so its CI and tooling may be copied.
- **Commits:** Conventional Commits (git-cliff builds the changelog from
  them). No Co-Authored-By or session trailers. British English in prose,
  comments and messages.
- **Claude Code versions.** The floor is 2.1.318 and the tested version is
  2.1.329. `crook open` refuses older versions and warns on newer ones.
- **Slugs** match `^[a-z0-9-]+$`.
- **Paths** (built from `XDG_STATE_HOME`, `XDG_DATA_HOME` and `HOME` by
  hand, never with the `dirs` crate, which gives
  `~/Library/Application Support` on macOS). An empty variable counts as
  unset:
  - state: `${XDG_STATE_HOME:-~/.local/state}/crook/<slug>/` holds
    `crook.db`, `gen`, `index/` and `settings-<role>.json`. The state dir is
    `<state>/crook`, and the Read/Edit/Write deny covers `<state>/crook/**`.
    Tasks 1 to 10 were built with this under `XDG_DATA_HOME`; Task 10b moves
    it.
  - home: `${XDG_DATA_HOME:-~/.local/share}/crook/<slug>/` by default (Task
    13)
  - mod: `${XDG_DATA_HOME:-~/.local/share}/crook-plugin/<version>-<hash>/`
  - config: `~/.config/crook/config.toml`
  - Claude Code's config: `${CLAUDE_CONFIG_DIR:-~/.claude}`, never a
    hard-coded `~/.claude`
- **SQLite:**
  - WAL mode, and `busy_timeout` = 5000 ms
  - every write transaction is IMMEDIATE:
    `conn.transaction_with_behavior(TransactionBehavior::Immediate)`
  - the schema version is in `PRAGMA user_version`
  - only `crook open` and `crook resume` migrate
  - a non-migrating open never creates a file: it uses `OpenFlags` without
    `SQLITE_OPEN_CREATE`
- **No panics in the hook path.**
  - `[profile.release]` sets `panic = "unwind"` explicitly, so
    `catch_unwind` works.
  - `src/lib.rs` carries
    `#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic))]`.
  - The hook writes stdout with errors ignored (`let _ = writeln!(…)`), never
    `println!`, which panics on a closed pipe.
  - The hook always exits 0.
- **Environment reads are injected.** Library code takes
  `env: &dyn Fn(&str) -> Option<String>`, never `std::env::var`. Rust 2024
  makes `set_var` unsafe, and test threads share one environment.
  Integration tests run `env!("CARGO_BIN_EXE_crook")` through one helper,
  `tests/common/mod.rs::crook()`, which calls `env_clear()` and then puts
  back `PATH` and every `LLVM_PROFILE_FILE` and `CARGO_LLVM_COV*` variable,
  so binary-driven tests still count under `cargo llvm-cov`.
  `herdr`, `claude` and `git` lookups go through the injected `Env`'s `PATH`
  too.
- **`CLAUDECODE` counts only when its value is `1`.** Spike 9's fallback sets
  it to the empty string.
- **Text goes over stdin.** Questions, tasks and briefs never go in argv.
- **Worktrees** are `<repo>/.worktrees/<slug>/<child>`, on branch
  `crook/<slug>/<child>` unless `branch` is given (draft 8.8). A child's
  extra worktrees are `<repo>/.worktrees/<slug>/<child>.<name>`, on
  `crook/<slug>/<child>.<name>` (draft 8.9). Every one is a row in the
  `worktrees` table (Task 19), the spawn worktree included, and there are
  no links under the home. `children.worktree` stays as the child's cwd.
- **Worktree removal safety.** `stop` and `wipe` only ever run
  `git worktree remove` on paths that are both in the `worktrees` table and
  under `<repo>/.worktrees/<slug>/`.
- **The home** is `config.home`, by default `${XDG_DATA_HOME:-~/.local/share}/crook`,
  so a project's home is `<home>/<slug>/`. It is never inside the state
  dir. The diary is `<home>/<slug>/diary`, or `<diary_root>/<slug>`.
- **Addressing.** Sessions go by session id, never by name. herdr panes are
  looked up each time, never stored.
- **Fixed strings, verbatim:**
  - child wake-up: `crook: you have tasks waiting. Call take_task.`
  - orchestrator wake-up: `crook: children are done. Call status().`
  - ask reason: `crook: private reference: <first hit>`, plus `(+N more)`
    when N > 0
  - stale nudge: `the card is stale: move Where it stands on, keeping what is still true`
  - missing binary: `crook: binary not found`
  - card changed, a second line on the next wake-up only:
    `crook: the project card changed: call status()`
  - stopped child: `crook: this child was stopped`
  - fork refusal: `this is a fork; the child is session <id>`
  - question label: `waiting on the user: don't answer or work around it`
  - old or missing database: `run crook open <slug>`
  - hook in a broken crook session: `crook: hook error: run crook open <slug>`
  - launch prompt: `You are child <child> of crook-<slug>. Your project card is above.`
  - the card's private rule: `use freely for work; anything from these leaving through a tool call raises a prompt; never reference them in public without asking`
- **Numbers:**
  - the watcher polls every 2 s, with a 60 s safety net
  - the orchestrator stops waking after 5 machine-started turns in a row
  - question text in context is capped at 300 characters
  - the card is stale after 3 `done`s or 7 days
- **Done outcomes** are only `ok`, `failed` or `dropped`. A `pr_url` must
  match `^https://[^/]+/[^/]+/[^/]+/pull/[0-9]+$`.
- **Mod tools** use `isDeferred: false` and are served at `tool.run`, never
  at `tool.call`.
- **Checks every task must pass before its commit:** `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and,
  from Task 1 on, `prek run --all-files`; from Task 3 on, `npm test` and
  `tsc -p plugin/`.

## Review Focus

1. **`claude agents --json` has two state keys.** Interactive rows carry
   `status` and background rows carry `state`. `crook status` and `crook next`
   must read both. Test in Task 7.
2. **macOS symlinks.** `/tmp` is `/private/tmp`, `/var` is `/private/var`,
   and `~` expands. A private path written either way must hit, and so must a
   path matching a glob written either way. Test in Task 10.
3. **The hook never crashes.** With `onFailure: "block"`, a panic or a
   non-zero exit blocks every tool call in every crook session. The hook gives
   a defined answer, not a crash, for: malformed stdin, a missing database, an
   older schema, a missing or corrupt index, and a forced panic inside a
   handler (caught by `catch_unwind` in the release-profile binary). Test in
   Tasks 9, 11 and 12.
4. **Large inputs.** A 5 MB `Write` with a 7 MB index is checked in under
   500 ms in a release build. Test in Task 12; CI runs it in Task 21.
5. **Missing or old database for a mod verb.** The verb prints
   `run crook open <slug>`, changes nothing, and creates no file. Test in
   Task 3.

---

## Milestone A: foundation and spike 9

### Task 1: Repo, tooling, paths and entry-point gates

**Files:**

- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `src/paths.rs`,
  `src/gate.rs`, `src/error.rs`
- Create: `.gitignore`, `LICENSE` (Apache-2.0), a stub `README.md`,
  `.editorconfig`, `.rustfmt.toml` (`edition = "2024"`, `max_width = 100`, as
  toolr's)
- Create: `mise.toml` with `lockfile = true` and these tools: `rust` (pinned
  as above), `cargo:cargo-nextest`, `aqua:taiki-e/cargo-llvm-cov`, `prek`,
  `node` (Task 3's npm package brings `tsc` and `vitest`), `uv` (toolr's
  tools venv; toolr's own `mise.toml` pins it), `git-cliff` (Task 22), `typos`, `rumdl`, `gitleaks`, `shellcheck`, `jactionlint`,
  `gh`, and `"aqua:s0undt3ch/ToolR" = "0.37.0"` (the latest release on
  2026-11-20; Task 21 uses it). Pin every tool to an exact version, the
  newest on the day, never `latest`: Renovate can't bump a `latest` pin.
  Commit `mise.lock`.
- `Cargo.toml` sets `rust-version` equal to the mise `rust` pin, as toolr's
  does, and Renovate moves the two together (Task 21).
- Create: `.pre-commit-config.yaml`, modelled on toolr's, keeping only what
  applies:
  - `default_install_hook_types: [pre-commit, commit-msg]`,
    `default_stages: [pre-commit]`
  - `pre-commit/pre-commit-hooks`: `check-merge-conflict`,
    `trailing-whitespace` (`--markdown-linebreak-ext=md`),
    `mixed-line-ending` (`--fix=lf`), `end-of-file-fixer`
  - `s0undt3ch/pre-commit-hooks`: `pin-github-actions`, `jactionlint-fix`,
    `shellcheck`, `gitleaks`, `typos`, `typos-commit-msg` (the `system-tool`
    form at the `commit-msg` stage), `rustfmt` (`system-tool`), `rumdl`
  - local: `cargo-lock` (`cargo fetch`) and `clippy`
    (`cargo clippy --all-targets -- -D warnings`), as toolr's
  - Task 21 adds the toolr-backed hooks.
- Test: `src/paths.rs` and `src/gate.rs` unit tests, `tests/gate_cli.rs`,
  and the shared helper `tests/common/mod.rs` (see Global Constraints)

**Interfaces:**

- Produces:
  - `pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;`
  - `paths::valid_slug(s: &str) -> Result<(), Error>`
  - `paths::data_dir(env: Env) -> PathBuf` (Task 10b turns this into
    `state_dir`)
  - `paths::project_dir(env: Env, slug: &str) -> Result<PathBuf, Error>`
  - `paths::plugin_dir(env: Env) -> PathBuf`: `<version>-<hash>` (Task 3
    defines the hash)
  - `paths::config_file(env: Env) -> PathBuf`
  - `paths::claude_config_dir(env: Env) -> PathBuf`
  - `pub enum gate::Entry { User, Mod, Hook }`
  - `pub struct gate::Identity { pub slug: String, pub role: Role, pub child: Option<String> }`,
    `pub enum Role { Orchestrator, Child, None }`
  - `gate::check(entry: Entry, env: Env) -> Result<Identity, Error>`
  - `error::Error`, a `thiserror` enum. Later tasks add variants.
- [ ] **Step 1: Write the failing tests.**
  - `valid_slug`: accepts `foo` and `a-1`. Rejects `Foo`, `a/b`, `..`, and
    the empty string.
  - `data_dir_xdg` (Task 10b turns it into `state_dir_xdg`):
    `XDG_DATA_HOME=/x` gives `/x/crook`. With it unset and
    `HOME=/h`, the result is `/h/.local/share/crook`, on macOS too.
  - `plugin_dir`: `<xdg>/crook-plugin/<pkg version>-<hash>`.
  - `claude_config_dir`: `CLAUDE_CONFIG_DIR=/c` gives `/c`; unset gives
    `<HOME>/.claude`.
  - `gate_user`:
    - `CLAUDECODE=1` gives an error whose message contains `CLAUDECODE`
    - `CLAUDECODE=` (empty) passes
    - `CROOK_ROLE=child` gives an error containing `CROOK_ROLE`
    - a clean env passes
  - `gate_mod`:
    - a missing `CROOK_ROLE` is refused
    - `CLAUDECODE=1` is refused; `CLAUDECODE=` passes
    - `orchestrator` or `child` passes. The slug comes only from
      `CROOK_PROJECT` and is validated.
    - `child` without `CROOK_CHILD` is refused
  - `gate_hook`: never refuses. A missing `CROOK_PROJECT` gives
    `Role::None`.
  - `tests/gate_cli.rs`, against the built binary through
    `tests/common/mod.rs::crook()`:
    - `crook init x` with `CLAUDECODE=1` exits 2 and names `CLAUDECODE` on
      stderr
    - `crook --version`, `crook --help` and `crook completion print zsh` with
      `CLAUDECODE=1` exit 0. They touch no state, and `mise install` may run
      them inside a Claude Code shell.
    - `crook hook PreToolUse` with empty stdin exits 0 (stub-only: Task 9
      replaces the stub, and its tests then expect an `ask`)
    - `crook hook NoSuchEvent` exits 0 with empty stdout
- [ ] **Step 2: Run them and check they fail.**
  Run `cargo nextest run`. Expected: compile errors.
- [ ] **Step 3: Implement.**
  - `main` reads `std::env::args_os()`. `hook` goes to `hook::main` (a stub
    that drains stdin and returns 0 until Task 9), `mod` goes to
    `modverb::main` (Task 3), anything else to clap.
  - clap's `completion print <shell>` prints a static script with
    `clap_complete::generate` for `bash`, `zsh` and `fish`: no dynamic
    `__complete` callback, unlike toolr's, because crook's commands are
    fixed. It skips the gate, as do `--version` and `--help`. packslip ships
    it as an `exec` completion resource (Task 21); mise runs it on demand and
    caches the output. Every other user verb calls
    `gate::check(Entry::User, …)` first; a refusal prints the error and exits 2.
  - `Cargo.toml`: `edition = "2024"`, `[profile.release] panic = "unwind"`,
    `lto = "thin"`, `strip = true`.
- [ ] **Step 4: Run every check and confirm it passes.**
  `mise install`, `prek install`, then the Global Constraints checks.
- [ ] **Step 5: Commit.** Message: `feat: repo skeleton, tooling, paths and entry gates`.

### Task 2: Store, schema v1 and the write helper

**Files:**

- Create: `src/store.rs`, `src/store/schema.sql` (loaded with `include_str!`)
- Test: unit tests in `src/store.rs`, `tests/store_concurrency.rs`

**Interfaces:**

- Consumes: `paths::project_dir`
- Produces:
  - `pub enum Mode { ReadOnly, ReadWrite, Migrate }`
  - `Store::open(env: Env, slug: &str, mode: Mode) -> Result<Store, Error>`
  - `Error::Schema { slug }`, displayed as `run crook open <slug>`
  - `bitflags! pub struct Touch: u8 { INBOX, STATUS, CHILDREN, PROJECT }`
  - `Store::write<T>(&mut self, touches: Touch, f: impl FnOnce(&rusqlite::Transaction) -> Result<T, Error>) -> Result<T, Error>`
  - `Store::read<T>(&self, f: impl FnOnce(&rusqlite::Connection) -> Result<T, Error>) -> Result<T, Error>`
  - `Store::gen_path(&self) -> &Path`
  - `pub const SCHEMA_VERSION: i32 = 1;`
  - Domain functions in later tasks take `&rusqlite::Connection`. A
    `Transaction` derefs to one, so the same function serves reads and
    writes.
- [ ] **Step 1: Write the failing tests.**
  - `migrate_creates_schema`: after `open(Migrate)`, `user_version` is 1,
    `journal_mode` is `wal`, and every table below exists.
  - `open_refuses_newer_or_missing`:
    - `open(ReadWrite)` on a missing database returns `Error::Schema`, whose
      message contains `run crook open`, and **no file or directory is
      created**
    - with `user_version=99`, it also returns `Error::Schema`
  - `read_only_older_version`: `open(ReadOnly)` on `user_version=0` with a
    `project` table succeeds and doesn't migrate. This is for `crook list`.
  - `tests/store_concurrency.rs::immediate_concurrent`: two threads, each with
    its own `Store`, each runs `write` that reads a counter, sleeps 50 ms, then
    writes counter+1. Both succeed and the counter ends at 2. This pins the
    review-11 finding.
  - `gen_bump`:
    - a `write` with `Touch::INBOX` changes the `gen` mtime
    - a `write` with `Touch::empty()` doesn't
    - `read` doesn't
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement the store.**
  - Open flags: `Migrate` uses `SQLITE_OPEN_READ_WRITE | SQLITE_OPEN_CREATE`
    and creates the directory first. `ReadWrite` uses
    `SQLITE_OPEN_READ_WRITE` only. `ReadOnly` uses `SQLITE_OPEN_READ_ONLY`.
    All add `SQLITE_OPEN_NO_MUTEX`.
  - Set `conn.busy_timeout(Duration::from_secs(5))`. Set WAL with
    `pragma_update_and_check` (the pragma returns a row), in `Migrate` only;
    WAL persists in the file.
  - `write` takes `&mut self` because `transaction_with_behavior` needs
    `&mut Connection`. Check that signature in the rusqlite version you pin.
    It commits, and drops (rolls back) on error.
  - After a commit whose `touches` is non-empty, write `gen` through a temp
    file and `rename`.
  - The schema is this plan's call; the spec's State section names the
    tables:

```sql
CREATE TABLE project(id INTEGER PRIMARY KEY CHECK(id=1), what TEXT NOT NULL DEFAULT '',
  why TEXT NOT NULL DEFAULT '', status_text TEXT NOT NULL DEFAULT '',
  version INTEGER NOT NULL DEFAULT 1, card_updated_at INTEGER NOT NULL, dones_since_card INTEGER NOT NULL DEFAULT 0);
CREATE TABLE project_item(kind TEXT NOT NULL CHECK(kind IN ('repo','notes','private','allow','link','ticket')),
  value TEXT NOT NULL, pos INTEGER NOT NULL, PRIMARY KEY(kind,value));
CREATE TABLE orchestrator(id INTEGER PRIMARY KEY CHECK(id=1), session_id TEXT, pid INTEGER);
CREATE TABLE children(child TEXT PRIMARY KEY, repo TEXT NOT NULL, worktree TEXT NOT NULL, branch TEXT NOT NULL,
  brief TEXT NOT NULL, session_id TEXT NOT NULL, claude_pid INTEGER, archived INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL);
CREATE TABLE inbox(seq INTEGER PRIMARY KEY AUTOINCREMENT, child TEXT NOT NULL, text TEXT NOT NULL, taken INTEGER NOT NULL DEFAULT 0);
CREATE TABLE status(child TEXT PRIMARY KEY, state TEXT NOT NULL CHECK(state IN ('working','parked','done')),
  seq INTEGER NOT NULL, question TEXT, outcome TEXT, pr_url TEXT,
  current_task INTEGER, done_seq INTEGER, changed_at INTEGER NOT NULL);
CREATE TABLE forks(child TEXT NOT NULL, session_id TEXT NOT NULL, PRIMARY KEY(child,session_id));
CREATE TABLE seen(child TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE notified(child TEXT PRIMARY KEY, seq INTEGER NOT NULL);
CREATE TABLE asks(id INTEGER PRIMARY KEY, at INTEGER NOT NULL, session_id TEXT, tool TEXT NOT NULL,
  hit TEXT NOT NULL, others INTEGER NOT NULL);
```

  `status.seq` goes up by one on every change to the row.

- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: per-project sqlite store`.

### Task 3: The mod verb transport, the embedded mod and a minimal mod

**Files:**

- Create: `src/modverb.rs`, which dispatches and holds the JSON envelope
- Create: `src/launch/plugin_dir.rs`, which unpacks the embedded mod
- Create: `plugin/.claude-plugin/plugin.json` (name `crook`),
  `plugin/hooks/hooks.json`, `plugin/hooks/register.ts`,
  `plugin/hooks/adapter.ts`, `plugin/hooks/core/ports.ts`,
  `plugin/hooks/core/verb.ts`, `plugin/tsconfig.json`
- Create: `package.json` (`"private": true`, `"name": "crook-mod"`,
  `"type": "module"`; scripts `test` = `vitest run` and `coverage` =
  `vitest run --coverage`; devDependencies `vitest`, `@vitest/coverage-v8`,
  `typescript`, each pinned exactly), `package-lock.json`,
  `vitest.config.ts` (`include: ['plugin/hooks/core/**/*.spec.ts']`,
  coverage `provider: 'v8'`, `include: ['plugin/hooks/core/**']`,
  `exclude: ['**/*.spec.ts']`, reporters `text` and `cobertura`, output
  `coverage/`)
- Modify: `.gitignore` (`node_modules/`, `coverage/`)
- Create: `build.rs`, only if Step 3's check needs it
- Test: unit tests in `src/modverb.rs` and `src/launch/plugin_dir.rs`,
  `tests/modverb_cli.rs`

**Interfaces:**

- **Mod verb contract.** Every verb is `crook mod <verb>`. It reads one JSON
  object on stdin and writes one JSON object on stdout.
  - Exit 0: `{"ok":true,...}`
  - Exit 1: `{"ok":false,"error":"..."}`
  - Every input carries `session_id` (string) and `user_turn` (bool).
  - Later tasks add verbs to this table. This task adds only `ping`.

| Verb | Extra input | Output |
|---|---|---|
| `ping` | none | `slug`, `role`, `child`, `schema`, `gen` (absolute path the watcher stats) |
| `status` | none | `card`, `version`, `children[]` of `{child,repo,state,question?,task?,untaken}` (see Task 6) |
| `take-task` | none | `seq`, `text` |
| `need` | `question` | none |
| `done` | `outcome`, `pr_url?` | none |
| `spawn` | `child`, `repo`, `brief`, `branch?` | `session_id` |
| `worktree` | `name`, `branch?` | `path` |
| `task` | `child`, `text` | `seq` |
| `stop` | `child` | none |
| `project-set-what-why` | `what`, `why`, `version` | `version` |
| `project-set-status` | `text`, `version` | `version` |
| `project-add` | `kind` (`link`, `ticket`, `repo` or `notes`), `value` | `version` |
| `lift` | none | `lifted` |
| `snapshot` | orchestrator only: `ack?`, a map of child to the `done_seq` the wake-up announced; writes `notified` to exactly those values | child: `untaken`, `taken`, `newest_seq`, `task_open`, `parked`, `is_current`, `current_id`, `card_version`. Orchestrator: `done_unnotified[]`, `waiting`, `working`, `done`, `untaken` (map of child to count), `card_stale`, `card_version`, `is_current` |
| `orch-context` | none | `lines[]`, `stale`; marks `seen` |
| `adopt` | none | none |

- Produces:
  - `modverb::main(env: Env, stdin: &mut dyn Read, stdout: &mut dyn Write) -> i32`
  - `pub struct Request { pub session_id: String, pub user_turn: bool, #[serde(flatten)] pub rest: serde_json::Value }`
  - `plugin_dir::ensure(env: Env) -> Result<PathBuf, Error>`: unpacks the
    embedded `plugin/` into `paths::plugin_dir` when it's missing, writing a
    temp directory and renaming it into place. Tasks 13 and 19 call it from
    `open`, `resume` and `spawn`.
  - `paths::plugin_dir` is `<xdg>/crook-plugin/<version>-<hash>`, where
    `<hash>` is the first 12 hex digits of a hash over the embedded files'
    paths and bytes, computed once per process. A rebuild that changes the
    mod gets a new directory, so smoke tests never run a stale mod.
- **The mod side:**
  - `core/ports.ts` exports
    `interface Ports { run(verb: string, input: object, timeoutMs?: number): Promise<VerbResult>; stat(path: string): Promise<{ mtimeMs: number } | null>; submit(text: string): Promise<void>; status(text: string | undefined): void; every(ms: number, fn: () => void): void; now(): number; env(name: string): string | undefined }`
    and `type VerbResult = { ok: boolean; error?: string; [k: string]: unknown }`.
  - `core/verb.ts` exports
    `parseVerb(exitCode: number, stdout: string): VerbResult`. A non-JSON or
    empty stdout gives `{ok:false, error:"crook: binary not found"}` when the
    exit code says the binary didn't run, else `{ok:false, error:"crook: bad reply"}`.
  - `adapter.ts` exports `ports($): Ports`. Its `run` calls
    `$.process.run([<CROOK_BIN>, "mod", verb], { stdin: JSON.stringify(input), timeoutMs })`
    and hands the result to `parseVerb`. Check in `claude-code.d.ts` how a mod
    reads its environment and what `$.process.run` resolves to.
  - Later tasks add `core/` modules; each takes a `Ports`, never `$`.
- [ ] **Step 1: Write the failing tests.**
  - `mod_ping`: in a temp `XDG_STATE_HOME`, after `Store::open(Migrate)`,
    `crook mod ping` gives
    `{"ok":true,"slug":"p","role":"child","child":"c1","schema":1,"gen":"<state>/crook/p/gen"}`.
  - `mod_missing_db`: `ping` on a missing project exits 1 with `error`
    containing `run crook open p`. Nothing appears under the state dir. This
    is Review Focus 5.
  - `mod_bad_json`: invalid stdin exits 1 with a JSON error, and doesn't
    panic.
  - `plugin_dir_unpacks`: `ensure` creates
    `<xdg>/crook-plugin/<version>-<hash>/.claude-plugin/plugin.json`. A
    second call changes nothing. No `*.test.ts`, `*.spec.ts` or `tsconfig.json` file is
    unpacked.
  - `plugin/hooks/core/verb.spec.ts` (vitest): good JSON, an `ok:false`
    reply, empty stdout, non-JSON stdout, and a missing binary each give the
    stated `VerbResult`.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement the dispatcher, the embed and the minimal mod.**
  - `include_dir!("$CARGO_MANIFEST_DIR/plugin")`, filtered at unpack time to
    skip `*.test.ts`, `*.spec.ts` and `tsconfig.json`.
  - Check whether `include_dir!` rebuilds when a *new* file appears under
    `plugin/`. Add a file, run `cargo build`, and see if the binary changes.
    If it doesn't, add a `build.rs` that prints
    `cargo:rerun-if-changed=plugin`.
  - At `session.start`, `register.ts` calls `ports($).run("ping", {...})`.
    On failure it sets `$.ui.status("crook: binary not found")`. On success
    it sets `$.ui.status("crook: <role>")`.
- [ ] **Step 4: Verify.**
  - The Global Constraints checks. Expected: PASS.
  - `npm ci && npm run coverage`. Expected: PASS, with `coverage/` written.
  - `claude plugin validate plugin/`, then `tsc -p plugin/`. Expected: no
    errors. Check that validate accepts `core/` imported through relative
    `import` declarations; if it doesn't, stop and report.
- [ ] **Step 5: Commit.** Message: `feat: mod verb transport, embedded mod and minimal mod`.

### Task 4: Spike 9, a gate before Milestone B

**Files:**

- Create: `spikes/09-mod-process/`, holding a throwaway mod and `RESULTS.md`
- [ ] **Step 1: Build the probe.**
  - Copy `plugin/` to the spike folder. Add calls to `ports($).run("ping", …)`
    from:
    - `prompt.submit`
    - a `tool.run` hook on a registered tool `probe`
    - a `$.clock.every(2000, …)` tick
  - Each call records its wall time and the mod's view of `CLAUDECODE`, and
    appends one line to a log file through `$.fs`.
  - Add one call that passes `env: { CLAUDECODE: "" }` and logs whether
    `CROOK_PROJECT` still reaches `crook`. This says whether `$.process.run`'s
    `env` merges with the inherited environment or replaces it.
  - Also record `$.fs.stat(<state dir>/<slug>/gen)` under a `--settings` file
    that denies Read and Edit on the state dir (`<state>/crook`). Task 8 isn't built yet, so
    write that file by hand.
- [ ] **Step 2: Run it in a herdr pane and under `claude --bg`.**
  Use Haiku, with `CROOK_PROJECT`, `CROOK_ROLE=child` and `CROOK_CHILD=c1`
  exported:
  `cargo build --release && CROOK_BIN=$PWD/target/release/crook claude --model haiku --plugin-dir spikes/09-mod-process/plugin --settings <deny file>`.
- [ ] **Step 3: Record the results in `RESULTS.md`.**
  - Is `CLAUDECODE` set in the mod's process, and to what value?
  - Does `env` merge or replace?
  - Does `ping` pass from all three call sites?
  - `stat` on `gen` under the deny: pass or fail
  - Does the hook's `cwd` follow the Bash shell? Add a throwaway
    `PreToolUse` command hook to the hand-written settings file that appends
    each input's `cwd` to a log. Run `cd <a subdirectory of the session's
    working directory, or of an --add-dir folder>` in one Bash call and `pwd`
    in the next (Claude Code keeps a `cd` only inside those directories),
    then compare the second call's hook `cwd` with what `pwd` printed. If it doesn't follow,
    stop and ask the user: relative path detection and Task 10's bare-name
    rule both rely on it, and the safe fallback is to treat every relative
    operand as unresolvable.
  - Latency per call: p50 and max
  - Clean up the sessions.
- [ ] **Step 4: Apply the gate.**
  - If `CLAUDECODE=1` is set there and `env` merges, make `adapter.ts`'s `run` pass
    `env: { CLAUDECODE: "" }`, as the spec allows.
  - If `env` replaces, pass these through explicitly as well:
    `CROOK_PROJECT`, `CROOK_ROLE`, `CROOK_CHILD`, `CROOK_BIN`, `HOME`,
    `PATH` (for `git`, `claude` and `herdr`), `XDG_STATE_HOME`, `XDG_DATA_HOME`,
    `CLAUDE_CONFIG_DIR`, `HERDR_ENV` and `HERDR_PANE_ID`.
  - If `$.process.run` fails from any call site, or `stat` is denied, stop.
    Report to the user before going on. Don't edit the spec.
- [ ] **Step 5: Commit.** Message: `spike: mod runs crook via $.process (spike 9)`.

---

## Milestone B: state verbs and the CLI

### Task 5: Project record, version check and card

**Files:**

- Create: `src/project.rs`, `src/card.rs`
- Create: `src/cli/init.rs`, `src/cli/project.rs`
- Modify: `src/modverb.rs`, adding `project-set-what-why`,
  `project-set-status` and `project-add`
- Test: unit tests in each, `tests/cli_project.rs`

**Interfaces:**

- Consumes: `Store::write`, `Store::read`, `gate::Identity`
- Produces:
  - `project::init(env: Env, slug: &str) -> Result<(), Error>`
  - `project::get(c: &Connection) -> Result<Record, Error>`
  - `project::set_what_why(c: &Connection, what: &str, why: &str, version: i64) -> Result<i64, Error>`
  - `project::set_status(c: &Connection, text: &str, version: i64) -> Result<i64, Error>`
  - `project::add(c: &Connection, kind: ItemKind, value: &str) -> Result<i64, Error>`
  - `Error::Stale`
  - `card::render(r: &Record, kids: &[ChildLine], me: Option<&SelfPart>) -> String`.
    `SelfPart` holds the child's brief, current task text and open question;
    `None` is the orchestrator.
- [ ] **Step 1: Write the failing tests.**
  - `set_version_check`: `set_status(…, v)` returns v+1. A second call with
    v returns `Error::Stale`. A call with v+1 succeeds.
  - `add_bumps_version`: `add` returns the new version and takes no version
    argument.
  - `set_resets_stale_counters`: a set zeroes `dones_since_card` and stamps
    `card_updated_at`.
  - `mod_set_refuses_machine_turn`: `project-set-status` and `project-add`
    with `user_turn:false` exit 1. The error says
    `only on a turn the user started; don't retry`. Both also refuse unless
    the role is `orchestrator`.
  - `add_notes_is_private`: `crook project add-notes` puts the folder in
    both `notes` and `private`.
  - `card_contents`:
    - the card has what/why, the status, links and tickets
    - it has one line per child as `id · repo · goal` (Task 19 adds its
      worktrees)
    - it has the private locations, with the card's private rule verbatim
    - the child-only parts show only when `me` is `Some`
    - it is at most 40 lines
  - `cli_project_version_check`: the CLI's `set-status` reads the version and
    writes with it, so a concurrent tool write gives `Error::Stale`, not an
    overwrite.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The version check is one `UPDATE project SET … , version=version+1 WHERE id=1 AND version=?`,
    then a test that one row changed.
  - `crook init` refuses an existing slug.
  - `crook project <slug>` takes `add-repo`, `add-notes`, `add-private`,
    `add-allow`, `add-link`, `add-ticket`, `remove <kind> <value>`,
    `set-what`, `set-why` and `set-status` (text on stdin for the last
    three).
  - Repo and notes paths are stored absolute and lexically cleaned.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: project record, card and version-checked writes`.

### Task 6: Children, inbox, status and forks

**Files:**

- Create: `src/crew.rs`, holding DB functions only, with no launching
- Modify: `src/modverb.rs`, adding `status`, `take-task`, `need`, `done`,
  `task`, `lift`, `snapshot`, `orch-context` and `adopt`
- Create: `src/cli/unpark.rs`
- Test: unit tests in `src/crew.rs`, `tests/modverb_verbs.rs`

**Interfaces:**

- Consumes: `Store`, `project::get`, `card::render`
- Produces:
  - `crew::add_child(c: &Connection, ch: &Child) -> Result<(), Error>`,
    which inserts `children` and `status(working)`
  - `crew::task(c, child: &str, text: &str) -> Result<i64, Error>`
  - `crew::take_task(c, child: &str) -> Result<(i64, String), Error>`
  - `crew::need(c, child: &str, q: &str) -> Result<(), Error>`
  - `crew::done(c, child: &str, outcome: Outcome, pr_url: Option<&str>) -> Result<(), Error>`
  - `crew::lift(c, child: &str) -> Result<bool, Error>`
  - `crew::archive(c, child: &str) -> Result<(), Error>`
  - `crew::is_current(c, child: &str, session_id: &str) -> Result<(bool, String), Error>`
  - `crew::record_session(c, child: &str, session_id: &str, pid: u32) -> Result<(), Error>`
  - `crew::add_fork(c, child: &str, session_id: &str) -> Result<(), Error>`
  - `crew::adopt(c, child: &str, session_id: &str) -> Result<(), Error>`
  - `crew::parked(c, child: &str) -> Result<bool, Error>`
  - `crew::snapshot(c, id: &Identity, session_id: &str, now: i64, ack: &BTreeMap<String, i64>) -> Result<Snap, Error>`
    (`ack` maps child to the `done_seq` announced),
    where `enum Snap { Child(ChildSnap), Orch(OrchSnap) }`
  - `crew::orch_context(c) -> Result<Vec<String>, Error>`, which also writes
    `seen`
  - `pub enum Outcome { Ok, Failed, Dropped }`, serde `lowercase`
- [ ] **Step 1: Write the failing tests.**
  - `take_task_order`: takes the oldest untaken task. It refuses with
    `task open` while `current_task` is set. It refuses while parked. It
    refuses `no tasks` on an empty inbox.
  - `done_clears_task_and_counts`:
    - `done` clears `current_task`, sets `state=done`, bumps `seq` and sets
      `done_seq` to the new `seq`
    - a later `take-task` sets `state=working` again
    - it increments `project.dones_since_card`
    - an outcome outside `ok|failed|dropped` is refused at the verb's JSON
      parse
    - a bad `pr_url` is refused
  - `need_parks_lift_unparks`: `need` sets `parked` and the question.
    `lift` returns true once, then false. Both bump `seq`.
  - `fork_refusal`: a mod verb whose `session_id` differs from
    `children.session_id` exits 1 with `this is a fork; the child is session <id>`.
    The exceptions are `ping`, `adopt`, and `snapshot`, which returns
    `is_current:false`. `ping` must pass: after `/clear` the mod may ping
    before the `SessionStart` hook records the new id. Currency is checked
    on every tool call, not once at start.
  - `archived_refusal`: every child verb for an archived child exits 1 with
    `crook: this child was stopped`.
  - `adopt_removes_fork`: after `add_fork(c1, F)` and `adopt(c1, F)`,
    `children.session_id` is F and `forks` doesn't hold F.
  - `child_status_own_question_only`: a child's `status` shows its own open
    question and no sibling's; siblings appear as `id · repo · state`.
  - `status_hides_question_on_machine_turn`: `status` with `user_turn:false`
    omits `question`. With `true`, it includes the question prefixed by the
    question label.
  - `status_carries_current_task`: after `take-task`, `status` for the child
    returns `task`, and the card carries it too.
  - `orch_snapshot`:
    - `done_unnotified` lists done children whose `done_seq` exceeds
      `notified`
    - `snapshot` with `ack` mapping them to their `done_seq` clears them
    - an `ack` with an older `done_seq` leaves a newer `done` unnotified
    - a later `need` or `lift` on a done child doesn't put it back in
      `done_unnotified`
  - `card_version`: both snapshots carry `project.version`
    - `card_stale` is true at `dones_since_card >= 3`, or when
      `now - card_updated_at >= 7 days`
  - `orch_context`:
    - one line per child whose `seq` moved past `seen`
    - the question is wrapped as `[untrusted, from child c1] …` and cut to
      300 characters (count `char`s, not bytes)
    - a second call returns nothing
  - `archived_kept_in_counts`: archiving keeps the rows. A snapshot leaves
    archived children out of `working`.
  - `cli_unpark`: `crook unpark c1` lifts the park and bumps `gen`.
  - `child_address`: `unpark`, `resume` and `stop` take `<child>` or
    `<slug>/<child>`. A bare `<child>` found in more than one project's
    database is refused with the matching `<slug>/<child>` forms listed.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - Each verb checks the role:
    - child verbs need `Role::Child`, and act only on `CROOK_CHILD`
    - orchestrator verbs need `Role::Orchestrator`
  - `task` reads its text from the JSON on stdin, which keeps it out of argv.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: children, inbox, status and fork state`.

### Task 7: User config, `crook list` and `crook status`

**Files:**

- Create: `src/config.rs`, which loads TOML with `private` and `allow`
  arrays
- Create: `src/agents.rs`, which parses `claude agents --json`
- Create: `src/cli/list.rs`, `src/cli/status.rs`, `src/cli/next.rs`
  (selection only; Task 20 adds the herdr jump)
- Test: unit tests in each, with `tests/fixtures/agents.json`

**Interfaces:**

- Produces:
  - `config::load(env: Env) -> Result<Config, Error>`. A missing file gives
    an empty config. `#[serde(deny_unknown_fields)]`.
  - `agents::list(env: Env) -> Result<Vec<Agent>, Error>` and
    `agents::parse(json: &str) -> Result<Vec<Agent>, Error>`, where
    `Agent { session_id, pid, cwd, kind, state }`. `state` is taken from
    `state`, or else from `status`.
  - `next::pick(rows: &[Row]) -> Option<Target>`. `crook next` builds `rows`
    from every project's database (opened `ReadOnly`, as `list` does) plus
    `claude agents --json`.
- [ ] **Step 1: Write the failing tests.**
  - `agents_both_keys`: the fixture has an interactive row with
    `"status":"idle"` and a background row with `"state":"blocked"`. Both
    parse to the right `state`. This is Review Focus 1.
  - `list_broken_db`: given two projects, one with a corrupt `crook.db`,
    `crook list` prints one good line and one error line, and exits 0.
  - `list_old_schema`: a `user_version=0` database lists without being
    migrated.
  - `status_flags_no_mod`: a live agent whose cwd is inside a crook worktree,
    but whose session id isn't recorded, is flagged `no crook mod`.
  - `next_picks_oldest`: the oldest of these is picked:
    - a parked child, by `changed_at`
    - a child whose agent row is `waiting` or `blocked`
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - `list` globs `<state>/crook/*/crook.db` and opens each `ReadOnly`, one at
    a time.
  - Its columns are: slug, what, working/parked/done counts, `card stale`,
    and the orchestrator's state (`live` or `-`). The orchestrator is live
    when `nix::sys::signal::kill(pid, None)` succeeds and the pid's session
    matches.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: crook list, status and next selection`.

---

## Milestone C: the hook and the privacy check

### Task 8: Generated `--settings`

**Files:**

- Create: `src/settings.rs`
- Test: unit tests in `src/settings.rs`

**Interfaces:**

- Produces:
  - `settings::build(o: &Opts) -> Result<Vec<u8>, Error>`. `Opts` holds
    `role: Role`, `state_dir: PathBuf` (`<state>/crook`; Task 10b renames it
    from `data_dir`), `crook_bin: PathBuf`.
  - `settings::permission_mode(user_default: &str) -> &'static str`
  - `settings::user_default_mode(env: Env) -> String`, which reads
    `permissions.defaultMode` from `<claude_config_dir>/settings.json`
- [ ] **Step 1: Write the failing tests.**
  - `settings_core`: the JSON has:
    - `disableAllHooks:false` and `crossSessionInbound:"refuse"`
    - Read, Edit and Write deny rules on `<state_dir>/**`
    - `PreToolUse` (matcher `*`) and `SessionStart` hooks, and no
      `PostToolUse` hook
    - each hook in exec form:
      `{"type":"command","command":"<crook_bin>","args":["hook","<Event>"],"onFailure":"block"}`
  - `settings_child`:
    - allow `mcp__crook__take_task`, `mcp__crook__need`, `mcp__crook__done`,
      `mcp__crook__status` and `mcp__crook__worktree` (Task 8 is built
      without `worktree`; Task 10b adds the allow)
    - deny `CronCreate`, `ScheduleWakeup` and `RemoteTrigger`
    - deny `/loop`. Check the permissions doc for a deny-rule syntax for a
      slash command. If none exists, the mod refuses `/loop` from
      `prompt.submit` in children instead; record which one was used.
  - `settings_no_memory_dir`: the settings never set `autoMemoryDirectory`.
    Auto memory stays per repo, as in the user's other sessions.
  - `permission_mode`: `bypassPermissions` and `plan` become `manual`;
    `acceptEdits`, `auto`, `manual` and `dontAsk` stay; empty or any other
    value becomes `manual`. The orchestrator and every child get it.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement** with `serde_json::json!`.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: generated session settings`.

### Task 9: The hook: SessionStart, park denial and parked-target asks

**Files:**

- Create: `src/hook/mod.rs`, `src/hook/sessionstart.rs`, `src/hook/park.rs`,
  `src/hook/output.rs`
- Test: unit tests in each, `tests/hook_cli.rs`

**Interfaces:**

- Consumes: `crew`, `project`, `card`, `Store`, `gate::check(Entry::Hook, …)`
- Produces:
  - `hook::main(event: &str, stdin: &mut dyn Read, env: Env, out: &mut dyn Write) -> i32`,
    which always returns 0
  - `hook::output` builds the decision JSON. Take the field names from the
    hooks doc's `PreToolUse` decision control and `SessionStart`
    `additionalContext` sections, and pin them in tests.
  - Output is valid JSON, or empty for "no decision".
- [ ] **Step 1: Write the failing tests.**
  - `hook_never_crashes`, a table test for Review Focus 3. Each case gives the
    stated answer and returns 0:
    - empty stdin, invalid JSON, or a missing `tool_name`: `ask`, reason
      `crook: hook error: bad hook input`
    - no `CROOK_PROJECT`, a missing database, or a schema the hook can't
      read: `ask` on every `PreToolUse` except `Read`, `Glob` and `Grep`,
      reason `crook: hook error: run crook open <slug>` (`<slug>` empty when
      unknown). The hook is only ever wired by crook's generated
      `--settings`, so each of these is a broken crook session, and it must
      not fail open. `SessionStart` adds the same line as context.
    - a store error during the park check: `ask`, reason
      `crook: hook error: …`
  - `tests/hook_cli.rs::forced_panic_release`: the test runs
    `cargo build --release --features test-panic --target-dir target/panic-test`,
    then runs that binary with `CROOK_HOOK_TEST_PANIC=1`. With the feature
    on, the handler panics when that variable is set. The binary exits 0
    and prints the hook-error `ask`. This proves `panic = "unwind"` holds in
    the release profile. Mark it `#[ignore]` and run it with the budget test.
  - `session_start_records`:
    - `source` of `startup`, `resume`, `clear` or `compact` records the
      session id and the hook's parent pid (`std::os::unix::process::parent_id`)
      for the child, or the orchestrator row
    - it prints `additionalContext` holding the card
  - `session_start_fork`:
    - `source:"fork"` adds the id to `forks`
    - when the parent pid equals `children.claude_pid` (the `/branch` case),
      it records the fork as the child instead
  - `park_denial`: while c1 is parked:
    - the session in `children` is denied every tool except
      `mcp__crook__status`, even when the same id also sits in `forks`
    - an unrecorded session in the same child env is also denied
    - a recorded fork is allowed
  - `stop_spawn_ask`: in the orchestrator:
    - `mcp__crook__stop` on a parked child gets `ask`
    - `mcp__crook__spawn` gets `ask` when a parked child is in the same repo
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The hook never migrates. `SessionStart` opens the store `ReadWrite`.
    `PreToolUse` opens it `ReadOnly` and compares `user_version` with
    `SCHEMA_VERSION` itself, because a `ReadOnly` open accepts any version
    (for `crook list`); a mismatch gives the hook-error `ask`. Test it with
    `user_version=99`.
  - `hook::main` wraps the whole handler in
    `std::panic::catch_unwind(AssertUnwindSafe(…))`. A caught panic becomes
    the hook-error `ask`. Install a panic hook that writes to stderr only.
  - The `test-panic` feature is off by default and never in release
    archives.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: settings hook, session ids, card and park denial`.

### Task 10: The private set and path detection

**Files:**

- Create: `src/privacy/mod.rs`, `src/privacy/set.rs`, `src/privacy/paths.rs`
- Test: unit tests in each

**Interfaces:**

- Consumes: `project::Record`, `config::Config`
- Produces:
  - `privacy::Set::load(rec: &Record, cfg: &Config, env: Env, slug: &str) -> Set`
  - `Set::contains(&self, abs: &Path) -> bool`
  - `Set::allowed(&self, abs: &Path) -> bool`
  - `privacy::find_paths(input: &serde_json::Value, cwd: &Path, s: &Set) -> Vec<Hit>`,
    where `Hit { text: String, kind: HitKind }`
- [ ] **Step 1: Write the failing tests.**
  - `symlink_realpath`, for Review Focus 2:
    - with a private root of `/tmp/notes`, an input of
      `/private/tmp/notes/a.md` hits
    - the reverse also hits
    - `~/notes/x` hits when `~/notes` is private
  - `relative_resolves`: an input of `../notes/a.md` with `cwd` set to
    `/r/repo` hits when `/r/notes` is private.
  - `globs`:
    - a `private` glob of `~/kb/**/secret-*.md` hits on a matching path
    - a glob written under `/tmp/…` hits the same file reached as
      `/private/tmp/…`
  - `allow_glob_stays_in_dir`: `allow = ["~/x/*.md"]` allows `~/x/a.md` but
    not `~/x/sub/y.md`. globset's `*` crosses `/` unless
    `literal_separator(true)` is set, and that is the leak direction.
  - `allow_list`: a path on the `allow` list doesn't hit.
  - `nested_json`: paths inside nested arrays and objects of `tool_input` are
    found.
  - `embedded_forms`: each of these hits when `/abs/notes` is private:
    `--body-file=/abs/notes/p.md`, `file=@/abs/notes/a`,
    `https://x/?f=/abs/notes/a`, `a,/abs/notes/b`, and a root with a space,
    `~/Documents/My Notes/x.md` inside a longer string, plus
    `--body-file "$HOME/notes/p.md"` and `${HOME}/notes/p.md` when
    `~/notes` is private.
  - `root_boundary`: `/abs/notes-old/a` doesn't hit when `/abs/notes` is
    private.
  - `bare_names_in_private_cwd`: with `cwd` inside `/abs/notes`, where
    `plan.md` exists, each of these hits: `cp plan.md ~/repo/docs/plan.md`,
    `gh pr create --body-file plan.md`, `curl -F file=@plan.md`, `tar c .`,
    `cat *.md`, `gh pr create --body-file "$PWD/plan.md"` and
    `cp "$(pwd)/plan.md" /tmp/`. With `cwd` in a public repo, `cp plan.md x`
    doesn't hit. A `Write` whose `content` holds `see [plan](x)` with `cwd`
    private doesn't hit through this rule, because it covers Bash and Monitor
    only.
    A bare `git push` with `cwd` in a private repo still doesn't ask (the
    spec's known gap).
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.** Two passes, and a hit from either counts:
  - **Root substrings.** For every string in the input, search for each
    private root in five forms: as written in the record, with `~`, with
    `$HOME`, with `${HOME}` for the home directory, and canonical. A match counts when the next character is `/`,
    the end of the string, or a character that can't be part of a path
    (whitespace, a quote, `)`, `>`, `,`, `;`, `&`, `|`). This catches roots
    with spaces and paths embedded in URLs and flags.
  - **Tokens.** Candidate tokens are runs with no whitespace and none of
    ``'"`<>|;&()``, then split again at `=`, `@`, `,` and `:`. A token
    counts if it contains `/` or starts with `~`.
  - **Bare names in a private `cwd`.** This applies only to the `command`
    field of Bash and Monitor, never to other tools' input (a markdown body
    with `*` or `[` would ask for nothing). When the hook's `cwd` is inside
    a private location, a token in that field also counts when it names an
    existing entry under `cwd`, is exactly `.`, has a glob character (`*`,
    `?`, `[`), or is `$PWD` or `${PWD}` (alone or as a prefix). `$(pwd)`
    and `` `pwd` `` are found by a substring search on the raw `command`
    before tokenising, because the tokeniser splits on `(`, `)` and the
    backtick. Outside a private `cwd`, bare names never count (review 7
    dropped matching file names anywhere).
  - Expand `~` and join relative tokens to `cwd`. Resolve the longest
    existing prefix with `std::fs::canonicalize` first (the kernel's view of
    `..` through symlinks), then apply any `.` and `..` left in the
    non-existent tail lexically.
  - Do the same for every root, and for the literal prefix
    of every glob (the part before the first glob metacharacter).
  - Build globs with `GlobBuilder::new(p).literal_separator(true)`.
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: private set and path detection`.

### Task 10b: crook's state moves to XDG_STATE_HOME

Tasks 1 to 10 are built with crook's state under `XDG_DATA_HOME`. Draft 8.9
moves it to `XDG_STATE_HOME` and denies sessions the state tree instead of
the data tree. This task changes the built code; the numbering of the other
tasks is unchanged.

**Files:**

- Modify: `src/paths.rs`: `data_dir` becomes `state_dir`, and `project_dir`
  uses it. `data_home` stays for `plugin_dir`.
- Modify: `src/settings.rs`: the deny rules cover `<state>/crook/**`, and
  the child allow list gains `mcp__crook__worktree` (a follow-up to Task 8,
  which is built without it)
- Modify: `src/cli/list.rs` and `src/cli/address.rs`, where `all_slugs`
  holds the `crook list` glob
- Modify: every other caller of `paths::data_dir`, and every test helper
  that builds the old path from `XDG_DATA_HOME`: `src/project.rs`,
  `src/store.rs`, `src/crew.rs`, `src/modverb.rs`, `src/hook/mod.rs`,
  `src/cli/project.rs`, and `tests/*.rs` (`cli_project`, `hook_cli`,
  `modverb_cli`, `modverb_verbs`, `store_concurrency`, `cli_unpark`,
  `cli_list`). `src/launch/plugin_dir.rs` keeps `XDG_DATA_HOME`.
- Modify: `plugin/hooks/adapter.ts`, only if Task 4 found that `env`
  replaces the inherited environment: add `XDG_STATE_HOME` to the passed
  list
- Test: the unit tests in `src/paths.rs` and `src/settings.rs`,
  `tests/cli_list.rs`, and the helpers above

**Interfaces:**

- Produces:
  - `paths::state_dir(env: Env) -> PathBuf`:
    `${XDG_STATE_HOME:-~/.local/state}/crook`. An empty variable counts as
    unset. It replaces `paths::data_dir`.
  - `paths::project_dir(env: Env, slug: &str) -> Result<PathBuf, Error>`:
    `<state_dir>/<slug>`, for `crook.db`, `gen`, `index/` and
    `settings-<role>.json`
  - `settings::Opts.state_dir: PathBuf`, the whole `<state>/crook`, not one
    project's directory. It replaces `data_dir`.
- Unchanged: `paths::plugin_dir` (`<xdg data>/crook-plugin/<version>-<hash>`)
  and `paths::config_file`.
- [ ] **Step 1: Write the failing tests.**
  - `state_dir_xdg` (replaces `data_dir_xdg`):
    - `XDG_STATE_HOME=/x` gives `/x/crook`
    - with it unset and `HOME=/h`, the result is `/h/.local/state/crook`, on
      macOS too
    - `XDG_STATE_HOME=` (empty) gives the same fallback
    - `XDG_DATA_HOME=/d` alone doesn't change it
  - `project_dir_validates_slug` (now a state-dir test): with
    `XDG_STATE_HOME=/x`, `foo` gives `/x/crook/foo` and `../etc` is an
    error. With only `XDG_DATA_HOME=/x` set and `HOME=/h`, `foo` gives
    `/h/.local/state/crook/foo`.
  - `plugin_dir_has_version_and_hash`: still `<XDG_DATA_HOME>/crook-plugin/…`,
    and `XDG_STATE_HOME` doesn't move it.
  - `settings_core`: the deny rules name `<state>/crook/**` for Read, Edit
    and Write (fixture `/s/crook`). No rule names the data tree
    (`<XDG_DATA_HOME>/crook` or `~/.local/share/crook`).
  - `settings_child`: the allow list holds `mcp__crook__worktree` beside
    `take_task`, `need`, `done` and `status`. The orchestrator's doesn't.
  - `list_state_glob` (`tests/cli_list.rs`): with a temp `XDG_STATE_HOME`,
    `crook list` finds `<state>/crook/<slug>/crook.db`. A database under
    `<XDG_DATA_HOME>/crook/<slug>/` isn't listed.
  - Every existing test that built the old path now sets `XDG_STATE_HOME`
    and asserts the same behaviour. `grep -rn data_dir src tests` finds
    nothing, and `grep -rn XDG_DATA_HOME src tests` finds only the
    `plugin_dir` tests.
- [ ] **Step 2: Run them and check they fail.**
  Run `cargo nextest run`. Expected: compile errors and failures on the new
  names.
- [ ] **Step 3: Implement.**
  - Rename `data_dir` to `state_dir`, reading `XDG_STATE_HOME` through the
    same empty-is-unset helper, with the `.local/state` fallback.
  - `project_dir` joins the slug onto `state_dir`.
  - In `settings::build`, deny `Read`, `Edit` and `Write` on
    `//<state_dir>/**`, and add `mcp__crook__worktree` to the child allow
    list.
  - `all_slugs` reads `state_dir`.
  - No data migration: no database has shipped. A developer's old
    `<xdg data>/crook/` can be deleted.
- [ ] **Step 4: Run every check and confirm it passes.**
  The Global Constraints checks, then `crook init x` and `crook list` in a
  shell with `XDG_STATE_HOME` set to a temp directory: the database appears
  there and nowhere under `XDG_DATA_HOME`.
- [ ] **Step 5: Commit.** Message: `refactor: move crook state to XDG_STATE_HOME`.

### Task 11: The verbatim index and subtraction

**Files:**

- Create: `src/privacy/index.rs`, `src/privacy/chunks.rs`,
  `src/privacy/subtract.rs`
- Test: unit tests in each

**Interfaces:**

- Produces:
  - `privacy::Index::refresh(dir: &Path, s: &Set) -> Result<Index, Error>`.
    It re-indexes changed files only.
  - `Index::lookup(&self, text: &str) -> Vec<ChunkHit>`, where
    `ChunkHit { chunk: String, file: PathBuf }`
  - `privacy::subtract(hits: Vec<ChunkHit>, public: &[String], file_content: &str, old_string: &str, repos: &[PathBuf], s: &Set) -> Vec<ChunkHit>`
- [ ] **Step 1: Write the failing tests.**
  - `shingles`:
    - normalisation lowercases nothing, collapses whitespace within a line,
      and doesn't cross lines
    - a line of fewer than 8 words gives no shingles
  - `refresh_by_mtime`:
    - a file edited after the first refresh is re-indexed, and a deleted file
      drops out
    - writes go to a temp file, then `rename`
    - no lock file is left behind
  - `corrupt_index`: a truncated `index/index.bin` makes `refresh`
    rebuild, with no error. This is Review Focus 3.
  - `index_scope`: `.git/` is skipped, a file over 1 MiB is skipped, and a
    file with a NUL byte in its first 8 KiB (binary) is skipped.
  - `subtract_repo_per_chunk`: two overlapping chunks taken from one
    9-word line of a tracked public file are both dropped, and a chunk
    found nowhere is kept, in the same call.
  - `lookup_names_file`: a hit reports the private file it came from.
  - `subtract_public`: a chunk equal to a ticket title from the record's
    tickets is dropped.
  - `subtract_edit_context`: a chunk already in the target file, or in
    `old_string`, is dropped.
  - `subtract_repo`:
    - a chunk that appears in a tracked file of a project repo, outside every
      private location, is dropped
    - a chunk only in a private folder tracked in that same repo is kept
    - a repo wholly inside a private location isn't searched
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The index shape is spike 5's, in **one** file, so a single `rename`
    replaces it and a concurrent hook never reads a torn pair:
    - 8-word shingles, hashed with FNV-1a 64 (a dozen lines; no crate)
    - `<state>/crook/<slug>/index/index.bin`: a header (magic `CRKI`, format version, entry
      count), then the manifest (file id, mtime, path), then sorted
      `(u64 hash, u32 file id)` records, little-endian. Read with
      `std::fs::read` and binary search.
  - Any header, length or count mismatch means corrupt: rebuild.
  - Scope: regular files under each private root, skipping `.git/`, files
    over 1 MiB and binary files (a NUL in the first 8 KiB). Over 20 000
    files, `crook open` and `crook resume` refuse, naming the root, until it
    is narrowed or `--allow-partial-index` is passed for that launch. A hook
    that finds the cap exceeded indexes the first 20 000 in path order and
    prints one stderr line. Paths still hit beyond the cap.
  - The repo search returns whole matching lines:
    `git -C <repo> grep -h -F -f <chunks file> -- . ':(exclude)<each private path inside repo>'`.
    Normalise each line as chunks are, and drop every chunk that is a
    substring of one. (`-o` would print only the first of overlapping
    shingles and keep the rest as false hits.)
- [ ] **Step 4: Run every check and confirm it passes.**
- [ ] **Step 5: Commit.** Message: `feat: verbatim index and public subtraction`.

### Task 12: Wiring the privacy check into `PreToolUse`, with the `asks` log

**Files:**

- Create: `src/hook/privacy.rs`
- Test: unit tests in each, `tests/hook_budget.rs`

**Interfaces:**

- Consumes: Tasks 9 to 11
- [ ] **Step 1: Write the failing tests.**
  - `exempt_tools`: these get no decision:
    - `Read`, `Glob`, `Grep`, `mcp__crook__*`, `Agent`, `Skill`, `TodoWrite`
      and `ToolSearch`
    - `Write` or `Edit` whose `file_path` is private
  - `write_into_repo_asks`: a `Write` into a repo whose `content` quotes an
    8-word line of a private note gets `ask`, reason
    `crook: private reference: <file>`.
  - `mcp_and_webfetch_checked`: `mcp__linear__save_comment` with a private
    path in `body` gets `ask`, and so does `WebFetch` with a private path in
    the URL.
  - `bash_checked`: `cat N/a` and `cat N/a | gh pr create -F -` both get
    `ask`; `ls /repo` gets no decision.
  - `count_others`: three hits give a reason ending `(+2 more)`.
  - `asks_row`: an ask inserts one `asks` row with the time, session id,
    tool, first hit and the count of others.
  - `asks_write_timeout`: with the database held by an IMMEDIATE transaction
    for 6 s, the hook still answers `ask` within the timeout, and writes the
    lost line to stderr.
  - `missing_index`: a deleted `index/` rebuilds and the check still answers.
  - `tests/hook_budget.rs::large_write_budget`, for Review Focus 4: a 5 MB
    `content` against a 7 MB index finishes in under 500 ms. Mark it
    `#[ignore]`; it runs with
    `cargo nextest run --release --run-ignored only` (this also runs Task 9's forced-panic test),
    locally now and in CI from Task 21.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement the order.**
  - park denial and the parked-target asks on `mcp__crook__stop` and
    `mcp__crook__spawn` (Task 9), then the exempt tools (which include the
    other `mcp__crook__*`), then paths, then the verbatim lookup, then
    subtraction
  - `PreToolUse` reads through a `ReadOnly` store. The `asks` insert opens
    its own short-lived `ReadWrite` connection with a 1 s busy timeout, so a
    held lock can't push the hook past its budget
- [ ] **Step 4: Run every check, plus the release budget test, and confirm they pass.**
- [ ] **Step 5: Commit.** Message: `feat: privacy check on every outbound tool call`.

Before Milestone D: a Fable security review of Milestone C's diff (Tasks
9–12), briefed with the spec's Threat model and Known gaps. Findings go to
the user before work continues.

---

## Milestone D: launching and spike 7

### Task 13: `crook open` and `crook resume`

**Files:**

- Create: `src/launch/mod.rs`, `src/launch/preflight.rs`, `src/cli/open.rs`,
  `src/cli/resume.rs`
- Create: `plugin/prompts/orchestrator.md`, read with `include_str!`
- Test: unit tests in each

**Interfaces:**

- Consumes: `plugin_dir::ensure`, `settings::build`, `agents::list`
- Produces:
  - `launch::argv(o: &LaunchOpts) -> Vec<OsString>`, a pure function that
    builds the `claude` argv
  - `launch::env(base: impl Iterator<Item = (OsString, OsString)>, id: &Identity, crook_bin: &Path) -> Vec<(OsString, OsString)>`.
    It drops every other `CROOK_*`, every `CLAUDE_CODE_MESSAGING_*` and
    `CLAUDECODE`.
  - `preflight::check(env: Env, claude_version: &str) -> Result<Vec<String>, Error>`
    (warnings, or a refusal)
  - `launch::reconcile(c: &Connection, agents: &[Agent]) -> Result<Vec<String>, Error>`
- [ ] **Step 1: Write the failing tests.**
  - `argv_orchestrator` gives exactly:
    - `-n crook-<slug>`
    - `--session-id <uuid>` for a new orchestrator, or `--resume <id>` when
      the `orchestrator` row holds an id whose process is gone (a reboot)
    - `--permission-mode` from `settings::permission_mode`
    - `--plugin-dir <xdg>/crook-plugin/<version>-<hash>`
    - `--append-system-prompt <text>`
    - `--settings <file>`
    - one `--add-dir` per notes folder outside `<home>/<slug>`, and one per
      record repo
    - cwd is `<home>/<slug>`
  - `home_config`: `config.toml` reads `home` and `diary_root`. The default
    `home` is `${XDG_DATA_HOME:-~/.local/share}/crook` (an empty variable
    counts as unset), so `<home>/demo` is `$XDG_DATA_HOME/crook/demo`. A
    `home` inside the state dir (`<state>/crook`) is refused.
  - `init_home`: `crook init demo` creates `<home>/demo/` and the
    diary (`<home>/demo/diary`, or `<diary_root>/demo`), and records the
    diary as a notes folder. It creates no `repos/`. It refuses when the
    home falls inside a private location. `init` is from Task 5; extend it
    here.
  - `private_diaries`: the private set always holds `<home>/*/diary`, or
    `<diary_root>/*` when set, so another project's diary is private.
  - `env_scrub`:
    - the result sets `CROOK_PROJECT`, `CROOK_ROLE` and an absolute
      `CROOK_BIN` (from `std::env::current_exe`, canonicalised)
    - it removes `CROOK_CHILD` for the orchestrator
    - it removes `CLAUDE_CODE_MESSAGING_X` and `CLAUDECODE`
  - `preflight_version`: `2.1.317` gives an error, `2.1.329` passes, and
    `2.1.340` gives a warning.
  - `preflight_managed`: managed settings with `allowManagedModsOnly` or
    `allowManagedHooksOnly` set give an error. Take the paths from the
    settings doc's managed-settings locations for macOS and Linux.
  - `preflight_mods`: an installed plugin under `<claude_config_dir>` whose
    `hooks.json` module source has `on("tool.check"` or `on('prompt.submit'`
    is named in a warning.
  - `open_live_attaches`: an `orchestrator` row naming a live pid never
    starts a second orchestrator. It focuses the herdr pane, or prints its
    pid and session id.
  - `resume_argv`: `crook resume c1` uses `--resume <children.session_id>`
    with the same flags, env and cwd as the worktree.
  - `reconcile`: a `children` row with no worktree on disk is reported, and a
    session that is running but unrecorded is flagged.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - `open` migrates with `Store::open(Migrate)`, calls
    `plugin_dir::ensure`, writes the settings into
    `<state>/crook/<slug>/settings-<role>.json`, prints warnings, then `exec`s
    `claude` (`std::os::unix::process::CommandExt::exec`).
  - The warnings always include one line saying that MCP tools able to read
    other projects (a loam plot reader, say) aren't closed.
  - If the orchestrator is live, `open` launches nothing: with herdr it
    focuses the pane (Task 20), otherwise it prints the orchestrator's pid
    and session id and exits 0. (`claude attach` is for background
    sessions; the orchestrator is interactive.)
  - `resume` migrates and calls `plugin_dir::ensure` too.
- [ ] **Step 4: Run every check, then smoke-test.**
  `crook init demo`, add a repo and notes, then `crook open demo` with
  `--model haiku`. Check that:
  - the card appears in the session
  - `crook: orchestrator` is on the status line
  - `Read` on the state dir is denied
  - spike 10: whether the new, non-git `<home>/demo` raises a trust dialog,
    and where its auto memory is kept. Record the result in the design's
    Spikes table. A trust dialog that would block a `--bg` start stops
    the task and goes back to the user.
- [ ] **Step 5: Commit.** Message: `feat: crook open and resume`.

### Task 14: Spike 7, the false-positive rate (a gate)

**Files:**

- Create: `spikes/07-false-positives/RESULTS.md`
- [ ] **Step 1: Set up.**
  - Open a real project with the user's private list.
  - Run one ordinary working session of about 30 tool calls. It should edit
    code, commit and draft a PR body without sending it.
- [ ] **Step 2: Record the results.**
  - Every `asks` row. Query them with `crook`; never read the database by
    hand from a session.
  - Mark each row as a true or a false positive.
  - `RESULTS.md` gives counts and classes only. It quotes no hit text and
    names no private path.
- [ ] **Step 3: Apply the gate.**
  - If any class of false positive goes over one false ask per session,
    stop and report it to the user.
  - Report the verbatim index's false asks separately from the path
    check's. The user leans towards dropping the index (keeping only path
    detection) if it proves annoying, so put that option first in the
    report, with its cost: a note pasted into a PR body or ticket would no
    longer ask. Don't drop it without the user's yes.
- [ ] **Step 4: Commit.** Message: `spike: privacy false positives (spike 7)`.

---

## Milestone E: the mod

The mod files live under `plugin/hooks/`. Each task's logic goes in a
`core/` module that takes `Ports` (Task 3), with its tests in a vitest
`*.spec.ts` beside it, using fake `Ports` and a fake clock. The task's
`register.ts` and `adapter.ts` wiring gets one `*.test.ts` that
`claude plugin test` runs against the engine: it checks the hooks are
registered and reach `core/`, not the logic again. Run these after every
task: `npm run coverage`, `claude plugin validate plugin/`, `tsc -p plugin/`
and `claude plugin test plugin/`. Every new `core/` line should be covered;
Codecov's 99% patch target will hold it there from Task 21. Rebuild the
binary before any smoke test, because the mod is embedded.

### Task 15: Session tools and turn origin

**Files:**

- Create: `plugin/hooks/core/tools.ts`, `plugin/hooks/core/origin.ts`
- Modify: `plugin/hooks/register.ts`, `plugin/hooks/adapter.ts`
- Test: `plugin/hooks/core/tools.spec.ts`, `plugin/hooks/core/origin.spec.ts`,
  `plugin/hooks/register.test.ts`

**Interfaces:**

- Consumes: `Ports` (Task 3) and the mod verb table
- Produces:
  - `origin.ts` exports `noteOrigin(kind: string, isCommand: boolean)`,
    `noteOwnSubmit()` and `userTurn(): boolean`
  - `userTurn()` is true when the last prompt that started a turn came from
    `composer` or `bridge`
- [ ] **Step 1: Write the failing tests.**
  - In `register.test.ts` (`claude plugin test`): `registers child tools
    only` and `registers orchestrator tools only` below, and `serves at
    tool.run`. The rest are vitest specs on `core/`.
  - `registers child tools only`: with `CROOK_ROLE=child`, exactly
    `take_task`, `need`, `done`, `status` and `worktree` are registered, each
    with `isDeferred:false`.
  - `registers orchestrator tools only`: `status`, `spawn`, `task`, `stop`,
    `project_set_what_why`, `project_set_status` and the four
    `project_add_*`.
  - `serves at tool.run`: a `tool.run` for `mcp__crook__need` runs `need`
    with `{session_id, user_turn, question}` and returns the verb's message.
    No `tool.call` hook answers for it.
  - `userTurn tracking`:
    - a `composer` prompt makes `userTurn()` true
    - `noteOwnSubmit()` makes it false
    - a `scheduled-trigger` prompt makes it false
    - the origin is latched at each main-loop `turn.start` from the prompt
      that started that turn. Check in `claude-code.d.ts` whether
      `turn.start` carries the prompt or its origin. If it doesn't, keep the
      origins of submitted prompts in a FIFO and shift one at each main-loop
      `turn.start`. Test the order: a wake-up submitted, then a `composer`
      prompt typed before the wake-up's turn starts, gives a machine turn
      first and a user turn second.
  - `crook-adopt`: `/crook-adopt` typed (`composer`) runs `adopt`; from any
    other origin it is refused. The origin is recorded in `prompt.submit` and
    checked in `command.run`.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The input schemas are the spec's argument lists. `done.outcome` is an
    enum. `worktree` takes `name` and `branch?` and never a path; the mod
    runs the `worktree` verb (Task 19).
  - Register at `session.start`, after `ping` succeeds. If `ping` fails,
    every tool returns `crook: binary not found`.
- [ ] **Step 4: Run the three checks and confirm they pass.**
- [ ] **Step 5: Commit.** Message: `feat(mod): session tools and turn origin`.

### Task 16: The child watcher (Opus)

**Files:**

- Create: `plugin/hooks/core/watcher.ts`
- Modify: `plugin/hooks/register.ts`
- Test: `plugin/hooks/core/watcher.spec.ts`, `plugin/hooks/register.test.ts`

**Interfaces:**

- Produces: `startChildWatcher(p: Ports, ids)` and
  `onTurn(e: {kind: 'start'|'complete', agentId?: string, calledTake: boolean, calledDone: boolean})`
- [ ] **Step 1: Write the failing tests.**
  Use fake `Ports`: a fake clock, `stat` and `run`.
  - `skips when gen unchanged`: no `snapshot` runs until the mtime
    moves or 60 s pass.
  - `mtime taken before read`: a `gen` change during the read triggers a new
    read on the next tick.
  - `skips while a run is in flight`.
  - `submits once`: with idle, not parked, `is_current`, `untaken=3` and
    `task_open=false`, exactly one `$.prompt.submit` of the fixed text goes
    out, and `noteOwnSubmit` is called.
  - `no submit when busy, parked, task open or not current`.
  - `subagent turns ignored`: `turn.start` with an `agentId` doesn't change
    idle.
  - `once per inbox state`: the offer key is the newest task's seq plus the
    taken count. A turn that ends without a `take_task` gets no second offer
    for the same key; a new task, a `take_task`, a park lift or a restart
    allows one more.
  - `offer after done`: a turn that took a task and called `done`, with
    tasks still untaken, gets the next offer at once (the key moved).
  - `no loop`: ten idle turn ends with an unchanged inbox give exactly one
    submit.
  - `card changed`: when `card_version` moved since the session last saw it,
    the next wake-up has the card-changed line as a second line. A card
    change alone never submits; the first snapshot after start only sets the
    baseline.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement with `$.clock.every(2000, …)`.**
  Idle is assumed at `session.start`.
- [ ] **Step 4: Run the three checks and confirm they pass.**
- [ ] **Step 5: Commit.** Message: `feat(mod): child inbox watcher`.

### Task 17: The park filter (Opus)

**Files:**

- Create: `plugin/hooks/core/park.ts`
- Modify: `plugin/hooks/register.ts`
- Test: `plugin/hooks/core/park.spec.ts`, `plugin/hooks/register.test.ts`
  (the `prompt.submit` hook reaches `core/park.ts` in a child only)
- [ ] **Step 1: Write the failing tests.**
  - `composer lifts`: a `composer` prompt with no `e.command` awaits `lift`
    before calling `next(e)`.
  - `bridge lifts`.
  - `slash passes without lift`: `e.command` set from `composer` calls `next`
    with no `lift`.
  - `machine prompts pass without lift`: `scheduled-trigger`,
    `task-notification` and `peer` each call `next(e)` unchanged.
  - `throw path`: if the filter throws, its `.catch` still runs `lift` and
    passes the `composer` prompt.
  - `failed lift`: the status line shows `crook: park lift failed`, and the
    prompt still passes.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  Register `on('prompt.submit', …)` in child sessions only. Also call
  `noteOrigin`.
- [ ] **Step 4: Run the three checks and confirm they pass.**
- [ ] **Step 5: Commit.** Message: `feat(mod): park filter`.

### Task 18: The orchestrator's wake-ups, status line and context

**Files:**

- Create: `plugin/hooks/core/orchestrator.ts`
- Modify: `plugin/hooks/register.ts`
- Test: `plugin/hooks/core/orchestrator.spec.ts`, `plugin/hooks/register.test.ts`
- [ ] **Step 1: Write the failing tests.**
  - `card changed`: as in Task 16, on the orchestrator's next wake-up.
  - `done wake-up once`:
    - two children done while busy give one submit of the fixed text after
      idle
    - right after the submit, a `snapshot` call carries `ack` mapping both
      children to the `done_seq` values that wake-up announced
    - a child that finishes again before the ack lands is still announced
      on the next wake-up
  - `no double wake after reload`: a `snapshot` with empty
    `done_unnotified` gives no submit.
  - `five machine turns`: after 5 wake-up turns with no `composer` between
    them, no submit goes out. A `composer` prompt resets the count.
  - `status line from cache`:
    - it shows `crook: 1 waiting on you · 2 working`
    - it adds `· card stale` when stale
    - it adds `c1: 3 untaken` for each child with untaken tasks
    - the render path never calls `Ports.run`
  - `context on user turn`: a `composer` prompt runs `orch-context` and calls
    `next({...e, context})` with its lines, plus the stale nudge when
    `stale`. A `scheduled-trigger` prompt gets no context.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  This shares the watcher loop with Task 16 under a role switch. Check the
  type of `PromptSubmitInput.context` in `claude-code.d.ts`.
- [ ] **Step 4: Run the three checks and confirm they pass.**
- [ ] **Step 5: Commit.** Message: `feat(mod): orchestrator wake-ups and status line`.

---

## Milestone F: spawn and stop

### Task 19: `spawn`, `task` and `stop`

**Files:**

- Create: `src/launch/child.rs`, `src/cli/stop.rs`
- Modify: `src/modverb.rs`, adding `spawn`, `task`, `stop` and `worktree`
- Modify: `src/store/schema.sql`, adding the `worktrees` table. This amends
  schema v1 in place: no database has shipped (spec, "Left to the
  builder"), so there is no version bump. Databases made by earlier smoke
  tests lack it; delete them.
- Modify: `src/crew.rs` (`worktrees` rows) and `src/card.rs` (each child's
  line lists its worktrees)
- Create: `plugin/prompts/child.md`, which says to make extra worktrees only
  with `worktree()` and never `git worktree add`. That is protocol only, not
  enforced.
- Test: unit tests in `src/launch/child.rs`

**Interfaces:**

- Consumes: `crew::add_child`, `crew::archive`, `launch::argv`,
  `launch::env`, `settings::build`, `plugin_dir::ensure`
- Produces:
  - `launch::spawn_child(store: &mut Store, req: &SpawnReq, run: &dyn Launcher) -> Result<String, Error>`,
    returning the session id
  - `pub trait Launcher { fn worktree_add(&self, …) -> Result<(), Error>; fn worktree_remove(&self, …) -> Result<(), Error>; fn worktree_list(&self, …) -> Result<Vec<PathBuf>, Error>; fn start(&self, …) -> Result<(), Error>; }`,
    injected so tests don't run `git` or `claude`. `worktree_list` parses
    `git worktree list --porcelain`; Task 19b uses it.
  - `launch::add_worktree(store: &mut Store, req: &WorktreeReq, run: &dyn Launcher) -> Result<PathBuf, Error>`:
    the `worktree` verb. The caller is the child, taken from the
    environment.
  - `crew::add_worktree`, `crew::worktrees(c, child: Option<&str>)` and
    `crew::remove_worktree_row` over this table, added to schema v1:
    `worktrees(child TEXT NOT NULL, repo TEXT NOT NULL, path TEXT PRIMARY KEY, branch TEXT NOT NULL, created_at INTEGER NOT NULL)`
  - `children.worktree` stays as the child's cwd. `spawn` also writes the
    spawn worktree as a `worktrees` row, so the table is the one list `stop`
    and `wipe` read.
- [ ] **Step 1: Write the failing tests.**
  - `spawn_checks`: a repo not in the record, a brief over 4000 characters,
    more than 8 live children, or a slug-unsafe child id are each refused.
  - `spawn_rows_first`: the rows are committed before the launch command
    runs. Use a launcher that fails, and check the rows remain for `open` to
    reconcile.
  - `spawn_worktree`: `git worktree add <repo>/.worktrees/demo/c1 -b
    crook/demo/c1` runs, or `-b <branch>` when a branch is given. The path
    is in `children.worktree` and in a `worktrees` row, and it is the
    child's cwd. No link is made under the home.
  - `spawn_two_projects`: projects `a` and `b` each spawning `c1` in one
    repo get separate worktrees and branches.
  - `worktree_creates_and_records`: `worktree` with `name` `fix` from child
    `c1` of `demo` runs `git worktree add <repo>/.worktrees/demo/c1.fix -b
    crook/demo/c1.fix` (or `-b <branch>`) in the child's repo, returns that
    path, and records it. The row is committed before the launcher runs: a
    launcher that fails leaves the row for `open` to reconcile. A second
    call with the same `name` is refused. A call from an orchestrator
    session is refused.
  - `worktree_rejects_bad_name`: `Fix`, `a/b`, `../x`, `a.b`, `a b` and the
    empty string are refused. An input that carries a `path` field has it
    ignored: the path is always built by crook. Nothing is run or written.
  - `card_lists_worktrees`: a child with its spawn worktree and two from
    `worktree` has one card line listing all three paths, and the card
    stays within 40 lines.
  - `spawn_argv_child`:
    - the orchestrator's flags, plus `--permission-mode` from
      `settings::permission_mode`
    - the launch prompt string
    - `--bg` when herdr isn't present (`herdr::present`, Task 20)
    - `CROOK_ROLE=child` and `CROOK_CHILD=c1`
  - `spawn_ensures_plugin`: `spawn` calls `plugin_dir::ensure`, so a binary
    upgraded mid-project still finds its own mod version.
  - `cli_stop_refuses_dirty`: `crook stop c1` refuses when
    `git -C <path> status --porcelain` isn't empty for any of the child's
    recorded worktrees, naming each dirty one, and changes nothing.
    Otherwise it archives the child, stops the session and runs
    `git worktree remove` on each recorded worktree.
  - `stop_removes_all_child_worktrees`: a child with its spawn worktree and
    two from `worktree` has all three removed by their recorded paths and
    their `worktrees` rows deleted. Another child's worktrees stay. A
    recorded path that isn't under `<repo>/.worktrees/demo/` is refused and
    never passed to `worktree_remove`: the safety invariant.
  - `stop_kinds`: the `kind` of the child's row in `claude agents --json`
    (`Agent.kind`) tells a `--bg` child from an interactive (herdr) one. A
    `--bg` child is stopped with `claude stop <id>`. A herdr child is stopped
    with SIGTERM to `children.claude_pid`, then its pane is
    closed, but only when `agents::list` shows that pid running the child's
    session id; otherwise the child is only archived (the pid is stale after
    a reboot or crash). The tool's `stop` does the same, minus the worktree
    removal.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The caps (4000 characters, 8 children) go in one `const` block.
  - The mod passes `timeoutMs: 120000` for `spawn`.
- [ ] **Step 4: Run every check, then smoke-test.**
  In `crook open demo`, ask the orchestrator to spawn `c1` and give it one
  task. Check the child's wake-up, `take_task`, `need`, the park, then
  typing the answer through `claude attach`, then `done`, then the
  orchestrator's wake-up.
- [ ] **Step 5: Commit.** Message: `feat: spawn, task and stop`.

### Task 19b: `crook wipe`

**Files:**

- Create: `src/cli/wipe.rs`
- Test: unit tests in it, with the injected `Launcher` for `git`

**Interfaces:**

- Consumes: `crew` rows (`worktrees` included), `agents::list`, `paths`,
  `config`, `Launcher` (`worktree_remove`, `worktree_list`)
- Produces: `wipe::run(env: Env, slug: &str, with_diary: bool) -> Result<Vec<String>, Error>`,
  returning the leftover branch lines it prints
- [ ] **Step 1: Write the failing tests.**
  - `wipe_refuses_live`: a live orchestrator or child session refuses, naming it.
  - `wipe_refuses_dirty`: any recorded worktree with `git status --porcelain`
    output refuses, naming every dirty one, and removes nothing.
  - `wipe_removes`: removes every worktree recorded for the project by its
    recorded path, runs `git worktree prune` per record repo, removes
    `<home>/<slug>` except `diary/`, then the state dir `<state>/crook/<slug>/`.
  - `wipe_reports_unrecorded`: `worktree_list` shows a worktree under
    `<repo>/.worktrees/demo/` that has no `worktrees` row. `wipe` reports it
    and never removes it. A worktree outside `.worktrees/demo/` isn't
    reported.
  - `wipe_only_removes_recorded`: only paths that are both in `worktrees` and
    under `<repo>/.worktrees/demo/` reach `worktree_remove`. A recorded path
    outside that prefix makes `wipe` refuse, naming it, and remove nothing.
  - `wipe_keeps_diary`: the diary stays without `--with-diary`;
    `--with-diary` removes `<home>/<slug>/diary` but is refused for a
    diary under `diary_root`.
  - `wipe_branches`: prints the `crook/<slug>/*` branches left in each
    repo, and deletes none.
  - `wipe_gate`: a user verb, refused under `CLAUDECODE=1` or `CROOK_ROLE`.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.** Removal order is worktrees, home, state dir, so
  a crash part-way leaves the record able to say what's left.
- [ ] **Step 4: Run every check, then smoke-test** on `demo` from Task 19.
- [ ] **Step 5: Commit.** Message: `feat: crook wipe`.

---

## Milestone G: herdr, repo automation, release and finish

### Task 20: herdr panes, notifications and `crook next`

**Files:**

- Create: `src/herdr.rs`
- Modify: `src/launch/child.rs`, `src/cli/next.rs`, `src/cli/open.rs`,
  `src/crew.rs` (notifications from `need` and `done`)
- Test: unit tests in `src/herdr.rs`, using a fake `herdr` script on `PATH`
  in a temp dir

**Interfaces:**

- Produces:
  - Every `herdr::` function takes the injected `env: Env` first and finds
    `herdr` on its `PATH`.
  - `herdr::present(env) -> bool`: true when the injected `Env` has
    `HERDR_ENV=1`, which herdr sets in its panes. herdr installed but crook
    run outside it means not present, so children start with `--bg`.
  - `herdr::workspace(env, slug: &str) -> Result<String, Error>`, which finds
    or creates one workspace per project
  - `herdr::spawn_pane(env, ws: &str, cwd: &Path, command: &[OsString]) -> Result<(), Error>`
  - `herdr::pane_for(env, session_id: &str) -> Option<String>`, looked up live
  - `herdr::focus(env, pane: &str) -> Result<(), Error>`
  - `herdr::notify(env, title: &str, body: &str) -> Result<(), Error>`
  - a user verb `crook launch <slug>/<child>` (hidden from `--help`): it
    reads the child's rows, builds the environment with `launch::env` and
    the argv with `launch::argv`, and `exec`s `claude`. It refuses a child
    whose session is already running (use `crook resume`).
- [ ] **Step 1: Read `herdr --skill` and write the failing tests.**
  - `spawn_pane_argv`:
    `herdr pane split --current --direction right --cwd <worktree> --no-focus`,
    then `herdr pane run <pane-id> "env -u CLAUDECODE -u CROOK_PROJECT -u CROOK_ROLE -u CROOK_CHILD '<CROOK_BIN>' launch '<slug>/<child>'"`.
    When the orchestrator's `XDG_STATE_HOME` or `XDG_DATA_HOME` is set, the
    command also sets it (`env … XDG_STATE_HOME='<path>' …`), so `launch`
    finds the same state dir and home; each is a path, not a secret. Nothing else is typed into the pane: no
    other environment values (tokens would land on screen and in shell
    history) and no system prompt.
  - `launch_env`: `crook launch p/c1` execs `claude` with `CROOK_PROJECT=p`,
    `CROOK_ROLE=child`, `CROOK_CHILD=c1` and `CROOK_BIN`, and without
    `CLAUDECODE` (use an injected exec so the test doesn't start `claude`).
  - `next_focuses_or_attaches`: if a pane is found, `focus` runs; if not,
    `claude attach <id>` runs.
  - `notify`: `herdr notification show <title> --body <child> --sound request`.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement.**
  - The `need` and `done` mod verbs raise the herdr notification themselves
    when herdr is present, after their write commits. A failed notification
    goes to stderr and never fails the verb. No mod change and no extra verb.
  - `pane_for` matches the pane's process info against the session's
    `claude` pid from `claude agents --json`.
  - The `herdr pane run` argument is one string; quote `CROOK_BIN` and the
    address for the shell. Briefs travel on stdin, never in it.
- [ ] **Step 4: Run every check, then smoke-test inside herdr.**
- [ ] **Step 5: Commit.** Message: `feat: herdr panes, notifications and crook next`.

### Task 21: Repo automation with toolr, and CI

Modelled on toolr's own repo. Copy shapes and code from
`~/projects/me/toolr`; keep only what a Rust-plus-TypeScript repo needs. No
Python wheels, docs site, benchmarks or Windows. Use the
`toolr-command-authoring` skill for `tools/` and the `toolr-ci-setup` skill
for the workflows.

**Files:**

- `tools/` already exists, scaffolded by the user with `toolr project init`
  on 2026-11-20 and not yet committed: `tools/pyproject.toml` (name `tools`,
  `requires-python = ">=3.15"`, `venv-location = "cache"`), `tools/.gitignore`,
  `tools/uv.lock`, `tools/example.py`. The init also trusted the repo on this
  machine. A fresh clone needs `toolr project trust`, which is the user's to
  run (`! toolr project trust`); CI gets trust from the ToolR action. Don't
  re-run the init, and don't hand-edit `tools/pyproject.toml` or
  `tools/uv.lock`. In this task:
  - delete `tools/example.py`
  - run `toolr project venv add "toolr-py>=0.37.0" packaging pytest coverage`.
    The `>=` floor on `toolr-py` is what lets Renovate bump its lock, as in
    toolr's `tools/pyproject.toml`. The command wraps `uv add` and updates
    `tools/pyproject.toml` and `tools/uv.lock` together. Use
    `toolr project venv lock` and `remove` for later changes, never `uv`
    directly.
- Create: `tools/ci.py` with the command `packslip-check`
- Create: `tools/precommit/` with commands `check-gate-needs` (port of
  toolr's `tools/precommit/workflows.py`) and `private-words`
- Create the tests in toolr's layout (`~/projects/me/toolr/tests/tools/`):
  - `tests/__init__.py` and `tests/tools/__init__.py`, plus one package per
    tools module: `tests/tools/ci/`, `tests/tools/precommit/`,
    `tests/tools/release/`, each with an `__init__.py`.
  - No `tools/__init__.py` and no `pythonpath` setting. pytest's default
    import mode puts the first folder above the `tests` package (the repo
    root) on `sys.path`, so `from tools.ci import …` imports `tools/` as a
    namespace package, as in toolr.
  - Tests drive commands through `toolr.testing` (`make_context`,
    `make_command_result`, `CommandsTester`) and call private helpers such as
    `_packslip_statement_failures` directly, as toolr's
    `tests/tools/ci/test_packslip_check.py` does.
  - `tests/conftest.py` holds toolr's `_clean_registry` and `_restore_cwd`
    autouse fixtures.
  - Cargo ignores these: it builds only `tests/*.rs` and `tests/*/main.rs`.
- Create: `.coveragerc` (`[run] source = tools`, `branch = True`), so the
  `tools/` code counts towards coverage and the tests don't.
- The tools tests run with
  `toolr project venv run -- coverage run -m pytest tests/tools`, then
  `coverage xml -o coverage/tools.xml`.
- Create: `.github/env` (`MISE_VERSION`, `TOOLR_VERSION=0.37.0`),
  `.github/packslip.toml`
- Create: `.github/actions/setup-toolr/`, modelled on toolr's: it reads
  `TOOLR_VERSION` from `.github/env`, fails if it's empty, and runs
  `s0undt3ch/ToolR@<sha> # v0.37.0` with that `version` (see the
  `toolr-ci-setup` skill). The mise pin, `TOOLR_VERSION`, the action ref and
  `toolr-py` in `tools/uv.lock` are one release; Renovate bumps them in one
  PR, and `tests/tools/precommit/test_toolr_pins.py` fails when they drift.
- Create: `.github/actions/pipeline-gate/` (copied from toolr),
  `.github/actions/setup-pre-commit/` (toolr's, minus the Python and mypy
  steps)
- Create: `.github/workflows/ci.yml`, `.github/workflows/_build-binary-archive.yml`
- Create: `renovate.json5`, `typos.toml`, `.rumdl.toml`
- Create: `.codecov.yml`, `mise.ci.toml` + `mise.ci.lock` (the Codecov CLI,
  CI-only, as toolr's), `.config/nextest.toml` (a `ci` profile that writes
  JUnit XML), `.github/actions/codecov-stage-test-results/` and
  `.github/actions/codecov-publish-test-results/` (copied from toolr)
- Modify: `.pre-commit-config.yaml`, adding the local toolr hooks and a
  `tsc -p plugin/` hook on `plugin/**/*.ts`

**Interfaces:**

- The build matrix is static YAML, not generated: toolr generates its
  matrix because it has to work out Python versions, and crook has none.
  `_build-binary-archive.yml` takes a `targets` JSON input as toolr's does;
  each caller writes it inline. Release: `x86_64-unknown-linux-musl`
  (`ubuntu-latest`), `aarch64-unknown-linux-musl` (`ubuntu-24.04-arm`),
  `aarch64-apple-darwin` (`macos-15`), `x86_64-apple-darwin`
  (`macos-15-intel`), all `tar.gz`. CI: the first and third only.
- `toolr ci packslip-check <archive_dir> [--packslip PATH] [--manifest PATH]`,
  ported from toolr's, with these assertions on the `packslip show`
  statement and the archives:
  - every artifact has exactly one `bin` entry, ending `/crook`
  - Linux artifacts are `libc=musl`
  - a completion resource with `shells == ["bash","zsh","fish"]` exists
  - **no** `skill` resources. packslip installs skills user-wide, so the
    crook protocol skill would load in sessions crook didn't start, which is
    an anti-goal. It ships only inside the mod, which is embedded in the
    binary. Any skill added later goes under `plugin/skills/` the same way:
    embedded, unpacked with the mod, and loaded only in crook sessions.
  - constants: project `github.com/s0undt3ch/shepherds-crook`, source repo
    `https://github.com/s0undt3ch/shepherds-crook`
- `toolr pre-commit private-words` runs `git grep -n -i -E -f <file>` over
  the tracked tree, where `<file>` is
  `${CROOK_PRIVATE_WORDS:-~/.config/crook-dev/private-words}`, one regex per
  line, kept **outside** the repo. Missing file: pass with a one-line notice,
  so CI and other contributors pass. Any match: fail, printing `path:line`
  only, never the matched word.
- `.github/packslip.toml`:

```toml
# The `toolr ci packslip-check` PR gate fails when this and the archives disagree.
bin = ["crook"]

[[resource]]
kind = "completion"
shells = ["bash", "zsh", "fish"]
exec = ["crook", "completion", "print", "{shell}"]
```

- [ ] **Step 1: Write the failing tests** in `tests/tools/`:
  - `tests/tools/ci/test_packslip_check.py::test_packslip_statement`: a statement with two bin entries, a glibc
    Linux artifact, a skill resource, or no completion resource each yields
    one named failure; a good statement yields none.
  - `tests/tools/precommit/test_private_words.py`: with a temp words file holding `secretword`, a
    tracked file containing it fails with `path:line` and without the word in
    the output; a missing words file passes.
- [ ] **Step 2: Run them and check they fail.**
- [ ] **Step 3: Implement the commands, then the workflows.**
  - `ci.yml`, in toolr's shape: `permissions: {}`, the concurrency block,
    `CACHE_SEED`, `step-security/harden-runner` first in every job, "Load
    shared env vars" from `.github/env`, actions pinned by SHA with a version
    comment. Jobs:
    - `pre-commit`: setup-pre-commit, then `prek run --all-files`
    - `test-linux`, `test-macos`: mise, then
      `cargo llvm-cov nextest --no-report --profile ci`, then
      `cargo llvm-cov report --cobertura --output-path target/coverage/cobertura.xml`,
      then upload it with `codecov/codecov-action` and a per-OS flag, and stage
      the JUnit file with `codecov-stage-test-results`, as toolr's `_test.yml`
      does. Then the ignored release tests
      (`cargo nextest run --release --run-ignored only`, not under coverage),
      then `tsc -p plugin/`.
    - `test-mod`: mise, `npm ci`, `npm run coverage`, then upload
      `coverage/cobertura-coverage.xml` with the flag `mod`
    - `codecov-notify`: after every test job, `MISE_ENV=ci`, installs the
      Codecov CLI and runs `codecov-publish-test-results`, which uploads the
      test results and tells Codecov every upload is in
    - `test-tools`: Setup ToolR, then
      `toolr project venv run --no-sync -- coverage run -m pytest tests/tools`, then
      `coverage xml -o coverage/tools.xml`, uploaded with the flag `tools`
    - `build-binary-archive`: calls `_build-binary-archive.yml`
    - `packslip-check`: installs the packslip CLI exactly as toolr's job does
      (verified release asset, `cmp` against mise's copy), then
      `toolr ci packslip-check "$RUNNER_TEMP/archives" --packslip "$PACKSLIP"`
    - `set-pipeline-exit-status`: the pipeline gate over every job

  - `_build-binary-archive.yml`: toolr's, changed to build from the checkout
    (no source tarball), with `cargo build --release --locked --target <triple>`.
    Stage `crook`, `LICENSE`, `README.md` and `CHANGELOG.md` if present into
    `crook-<v>-<triple>/`. The mod isn't staged: it is embedded in the
    binary (user's call, 2026-11-20). Keep `musl-tools`, the sha256 files, attestation
    and upload. Don't copy toolr's sdist comment about `--locked`.

  - Whether `claude plugin validate` and `claude plugin test` run in CI
    without a login isn't checked. Try them in one CI run. If they need
    auth, keep them local-only and say so in `CONTRIBUTING.md`.

  - `.pre-commit-config.yaml` gains local hooks: `check-gate-needs` (on
    workflow changes), `private-words` (always), `tsc` (on `plugin/**/*.ts`).

  - `.codecov.yml`: toolr's, with `manual_trigger: true`, project
    `target: auto` and `threshold: 0%`, and patch `target: 99%`. Components,
    each with its own `auto` project status, so the privacy boundary can't
    hide behind gains elsewhere:
    - `privacy`: `src/privacy/**`, `src/hook/**`
    - `state`: `src/store.rs`, `src/crew.rs`, `src/project.rs`, `src/card.rs`,
      `src/modverb.rs`
    - `cli`: `src/cli/**`, `src/launch/**`, `src/settings.rs`,
      `src/herdr.rs`, `src/agents.rs`, `src/config.rs`
    - `mod`: `plugin/hooks/core/**`
    - `tools`: `tools/**`

    Ignore `tests`, `target`, `node_modules`, `**/*.spec.ts`,
    `**/*.test.ts`, and the mod's `register.ts` and `adapter.ts`, which
    `claude plugin test` covers without a report. As in toolr's
    `.codecov.yml`, `codecov.notify.manual_trigger: true` holds the statuses
    until `codecov-notify` runs, and `comment.after_n_builds: 4` (one Rust
    report per OS, the mod and the tools) holds the PR comment.

  - Codecov upload auth: toolr's jobs use `id-token: write`; copy its
    `codecov-action` inputs. A private repo may also need a
    `CODECOV_TOKEN` secret. That secret is the user's to create; list it in
    the task report.

  - `renovate.json5`, copied from toolr's and trimmed:
    - top level: `extends` (`config:recommended`,
      `helpers:pinGitHubActionDigestsToSemver`), `timezone`, the weekly
      `schedule`, `osvVulnerabilityAlerts`, monthly `lockFileMaintenance`,
      `assignees`, `labels: ["dependencies"]`
    - `customManagers` (regex): the packslip CLI pin in `ci.yml`/`release.yml`;
      `MISE_VERSION` and `TOOLR_VERSION` in `.github/env`; `rust-version` in
      `Cargo.toml` with the `rust-version` datasource
    - ecosystem labels: `cargo` → `dependencies:rust`, `pep621` →
      `dependencies:python`, `npm` → `dependencies:typescript`,
      `github-actions` → `dependencies:github-actions`
    - groups, in this order, because a later matching rule wins in Renovate
      and the `toolr release` rule must override `github-actions` and
      `mise tools` for the ToolR pins, as in toolr's `renovate.json5`:
      `Rust toolchain` (the mise `rust` pin and `rust-version`, with
      `minimumReleaseAge: "7 days"`); `github-actions` (all actions plus the
      packslip CLI pin); `mise tools` (every mise tool except `rust`, `node`
      and ToolR); `vitest` (`vitest` and `@vitest/coverage-v8`, which must
      share a version); `toolr release` (`toolr-py` in `tools/` with
      `rangeStrategy: "update-lockfile"`, plus `aqua:s0undt3ch/ToolR` and
      `s0undt3ch/ToolR` in `mise.toml`, `.github/env` and
      `.github/**`), with `schedule: ["at any time"]` and
      `recreateWhen: "always"`
    - `package.json` keeps exact pins

  - `tests/tools/precommit/test_toolr_pins.py`: the mise ToolR pin, `TOOLR_VERSION`,
    the setup-toolr action's version comment and `toolr-py` in
    `tools/uv.lock` are all equal.
- [ ] **Step 4: Run the checks.**
  - `toolr project venv run -- coverage run -m pytest tests/tools`: PASS, and
    `coverage report` lists every `tools/` module
  - `npm ci && npm run coverage`: PASS
  - `prek run --all-files`: PASS
  - `jactionlint` passes on every workflow (the prek hook)
  - `cargo llvm-cov nextest` runs locally and writes
    `target/coverage/cobertura.xml`
  - `curl --data-binary @.codecov.yml https://codecov.io/validate` says the
    file is valid
  - Build the CI archives locally for the host triple, then run
    `toolr ci packslip-check <dir>`: `packslip-check: ok`
- [ ] **Step 5: Commit.** Message: `ci: toolr-driven automation, prek hooks and CI`.
  Don't push. Ask the user before the first push, which is what runs CI.

### Task 22: Release workflow, packslip and install smoke

**Files:**

- Create: `.github/workflows/release.yml`, `.github/workflows/install-smoke.yml`
- Create: `.github/actions/apply-release-patch/`, `.github/actions/configure-git/`
  (copied from toolr)
- Create: `tools/release.py` with `toolr release bump <version>`
- Create: `cliff.toml` (toolr's, without its release-notes queue),
  `CHANGELOG.md` (header only)
- Test: `tests/tools/release/test_bump.py`

**Interfaces:**

- `toolr release bump <version>` validates `X.Y.Z` or `X.Y.Z-rc.N` with
  `packaging.version.Version`, then runs `cargo set-version <version>`
  (cargo-edit, as toolr's `tools/version.py` does), which updates
  `Cargo.toml` and `Cargo.lock` together. Add `cargo:cargo-edit` to
  `mise.toml`.
- [ ] **Step 1: Write the failing test** `test_bump`: a temp copy of
  `Cargo.toml` gets the new version; `1.2` and `v1.2.3` are refused.
- [ ] **Step 2: Run it and check it fails.**
- [ ] **Step 3: Implement the command, then the workflows.**
  - `release.yml`, in toolr's shape: `workflow_dispatch` with
    `release-version`; refuse any ref but `refs/heads/main`; the admin
    permission check; `concurrency` without cancelling. Jobs:
    - `prepare-release`: `toolr release bump`, then `git-cliff --unreleased --tag v<v> --prepend CHANGELOG.md`,
      then commit the bump **as a patch artifact**, not a push, as toolr's
      `apply-release-patch` flow does
    - `build-binary-archive`: the release matrix, applying the patch first
    - `publish-release`: apply the patch, push the commit and the `v<v>` tag
      with the GitHub App token (as toolr's), then
      `gh release create v<v>` with the archives, sha256 files and the
      changelog section. Nothing is pushed until every build has passed.
    - `publish-packslip`: toolr's job, with
      `ARCHIVE_PATTERNS: crook-*-*-apple-darwin.tar.gz crook-*-*-linux-musl.tar.gz`
      and the verify identity
      `https://github.com/s0undt3ch/shepherds-crook/.github/workflows/release.yml@refs/heads/main`
    - `dispatch-install-smoke`, then `set-pipeline-exit-status`
  - `install-smoke.yml`: on macOS and Linux, install with mise from the
    published packslip, then check that `crook --version` prints the version,
    and that `crook completion print zsh` exits 0.
  - The App id and key secrets are the user's to create. List them in the
    task report; don't create them.
- [ ] **Step 4: Run the checks.**
  `toolr project venv run -- coverage run -m pytest tests/tools`, `prek run --all-files`.
  Don't dispatch the release.
- [ ] **Step 5: Commit.** Message: `ci: release workflow, packslip and install smoke`.
  Ask the user before any push, tag or dispatch. Artifact attestations and
  Sigstore signing may not work while the repo is private; the first real
  release may have to wait until it is public.

### Task 23: The protocol skill, README and end-to-end run

**Files:**

- Create: `plugin/skills/crook-protocol/SKILL.md`, and fill in
  `plugin/prompts/*.md`
- Modify: `README.md`, covering install (mise + packslip), the CLI table, the
  threat model in brief, credits and the known gaps. Known gaps include:
  upgrading crook with mise removes the binary that live sessions' absolute
  `CROOK_BIN` names, so every tool call in them blocks until
  `crook resume <child>` or `crook open <slug>` relaunches them.
- Create: `CONTRIBUTING.md` (mise, prek, toolr, the checks, the
  private-words file), `scripts/e2e.sh`, a manual checklist script
- [ ] **Step 1: Write the protocol text.**
  It covers:
  - one task at a time
  - `need` before a PR
  - never answer for the user
  - never reference private locations in public without asking
  - work on notes with `Read`, `Grep`, `Glob`, `Write` and `Edit`, not Bash:
    a Bash command that names a note asks
  - `done` outcomes
  - make extra worktrees only with `worktree()`, never `git worktree add`
    (protocol only; crook doesn't enforce it)
- [ ] **Step 2: Write the README.**
  - Credit the four idea sources: herdr-quartermaster, bermuda and loam with
    their public links; claude-snooze by name only, as "a private Claude
    Code plugin".
  - Run `toolr pre-commit private-words` with the user's words file present.
    Expected: no matches.
- [ ] **Step 3: Run the end-to-end checklist without herdr, then with it.**
  The steps are: open, spawn two children, task, `need`, `crook next`,
  answer, `done`, the wake-up, `/clear` in a child (the card returns),
  `crook resume` after killing a child, `crook stop`, and `crook list`.
- [ ] **Step 4: Run every test.**
  `cargo nextest run`, the release budget test,
  `toolr project venv run -- coverage run -m pytest tests/tools`, `npm run coverage`,
  `claude plugin test plugin/` and `prek run --all-files`. Expected: PASS.
- [ ] **Step 5: Commit.** Message: `docs: protocol skill, README and e2e checklist`.

---

## After the plan

- Spike results that change a spec fact go back to the user as proposed
  edits to the spec's Spikes table, applied only on a yes.
- The first push, the first CI run, any tag and any release dispatch are
  confirm-first. Never auto-merge a PR.
