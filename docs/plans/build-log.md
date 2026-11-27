# Build log

The unattended build, milestone by milestone. Each milestone is implemented, reviewed adversarially, fixed,
checked and pushed. Decisions made without the maintainer are recorded here with the reason, so they can be
reviewed and reversed.

## Decisions

| # | Decision | Why |
| --- | --- | --- |
| D1 | Build order: release M0 (tooling base), binary M0, release M1 (CI), then the binary milestones in order, then the plugin, the mod and the remaining release milestones. | The release plan's "Order against the other plans" puts M0 and M1 before the binary's first checked milestone, and the plugin and mod depend on binary subcommands. |
| D2 | Each milestone: a Sonnet agent implements it, an Opus agent reviews it adversarially, a Sonnet agent fixes the confirmed findings, then local checks run before the commit. | Sonnet is the cheapest model that implements well; adversarial review is the hard-reasoning step, so it gets Opus. |
| D3 | Work lands on `main` and is pushed after each milestone, with no pull requests. | The maintainer set main-only work for this session. |
| D4 | `mise.toml` pins rust 1.99.1 (minimal profile with clippy and rustfmt), cargo-nextest, cargo-edit, cargo-llvm-cov and git-cliff; no node and no `mise.ci.toml` yet. | Release M0.1 lists these; node and the CI lock belong to later milestones. |
| D5 | Restored tools tests define `REPO_ROOT` locally instead of importing it from a `tools/ci.py` that does not exist yet. | `tools/ci.py` arrives with release M4; creating it early would be dead code. |
| D6 | Tests for `tools/ci.py`, `tools/release.py`, workflow safety and e2e scripts are left out until their subjects exist. | A test for missing code fails or tests nothing. |
| D7 | Crate dependencies are added by the task that first uses them; milestone 0 has only `clap` and `thiserror`. | No unused dependencies to audit or update. |
| D8 | `crook hook <event>` and `spawn <role>` take free strings, not enums. | A clap usage error exits 2, but a hook must always exit 0 (0020); roles are plugin agent files, not a fixed list. |
| D9 | `crook --help` lists no `help` subcommand. | Task 0.1 says the help lists exactly the 0020 commands plus `setup` and `doctor`. |
| D10 | The runner returns a non-zero exit as normal output with its code, not as an error; after exit it drains pipes until both close or 300 ms pass with no data. | `hc send` exit codes carry meaning (spike 2); a descendant holding a pipe open must not hang a finished command. |
| D11 | A timeout kills the child's whole process group (`process_group(0)` and `killpg`), adding `nix` with only the `signal` feature. | The review showed grandchildren outliving a timeout; v1 used `nix` the same way. |
| D12 | Captured stdout and stderr are capped at 16 MiB each; the excess is read and dropped. | A chatty child must not grow memory without limit or block on a full pipe. |
| D13 | The environment rule is enforced by clippy's `disallowed-methods`, allowed only in `main.rs`; the substring test is gone. | The substring test was trivially bypassed by an import; clippy resolves the path. |
| D14 | `main` routes `crook hook …` before clap, so every hook invocation exits 0. | Clap exits 2 on a usage error, which would break the hook contract (0020). |
| D15 | The release-only test step passes `--no-tests=pass`. | With no `#[ignore]` tests yet, nextest fails with "no tests to run". |
| D16 | No separate schema job: `tests/schema.rs` runs inside the normal test jobs once binary tasks 2.6 and 7.2 add it. | M1.6 asks for schema checks on both OSes, which the test jobs already give. |
| D17 | Archive, smoke, packslip and mod jobs stay out of `ci.yml` until release M2 to M4. | Their subjects do not exist yet. |
| D18 | Codecov uploads use OIDC, as v1 did; the repository has no secrets. | `gh secret list` is empty, and v1 never needed a token. |
| D19 | Coverage uploads fail the job on error and use the same pinned Codecov CLI (v11.3.2) as `codecov-notify`. | Review: a dropped coverage report left CI green while statuses were computed from partial data. |
| D20 | Codecov components list each module's root file (`src/files.rs` and so on) next to its folder; `main.rs` and `lib.rs` belong to `cli`; `src/testutil.rs` is ignored like tests. | Review: root files fell in no component, so coverage drops there could not fail a status. |
| D21 | The test jobs still install all of `mise.toml`. | Trimming saves minutes on a cold cache only; revisit if CI time hurts. |
| D22 | The mise shim is `${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook`, in code, ADR 0019 and every plan. | Review: mise puts its shims under `MISE_DATA_DIR` or `XDG_DATA_HOME` when set (`XDG_DATA_HOME=/tmp/x mise doctor` showed `/tmp/x/mise/shims`); the fixed `~/.local/share` path would miss them. |
| D23 | Worktree removal keeps the worktree when it holds any ignored file, naming up to three. | Review: `git worktree remove` deletes ignored files such as `.env`; ADR 0023 says cleanup never loses work, which wins over tidiness. |
| D24 | `roster::load` reads `"version": 1.0` as 1. | JSON Schema treats 1.0 as the integer 1; loader and schema must agree. |
| D25 | Every path function returns a `Result`; a missing or relative `HOME` fails only when a path needs it, and relative or empty `XDG_*` values are ignored. | The XDG spec says relative values are invalid and must be ignored (review finding). |
| D26 | `flock::find` reports a folder with no roster, or a roster naming another flock, as a broken flock; `list` stays name-based. | Review nit: `find` ignored the roster; `status` must still be able to show broken flocks. |
| D27 | Slugs are at most 16 characters and must not look like an hc address (`t7`); child names at most 15. This replaces the 40-character limit in Q12 and Q45. | Captured from a scratch core: hc names are at most 32 characters of `a-z 0-9 - _ .` and must not be address-shaped, and the child's hc name is `<slug>.<child>` (Q3), so 16 + 1 + 15 fits. |
| D28 | Versions compare by semver precedence, pre-releases included (`0.1.0-rc.1 < 0.1.0-rc.2 < 0.1.0`). | The first release is `0.1.0-rc.1` (Q33); treating pre-releases as unknown would never move the `current` link between release candidates. |
| D29 | Preflight runs `hc --version`, then always `hc server start` with the scrubbed environment, then `hc schema`. | Every hc call except `--version` starts a core with the caller's environment if none runs (captured), and `server start` on a running core is harmless. |
| D30 | `plugin.json` carries `"author": {"name": "s0undt3ch"}`. | `claude plugin validate --strict` fails without an author, and the repository belongs to that account. |
| D31 | The coordinator skill writes the full shim expression in every Bash call rather than setting a shell variable once. | Shell state does not persist between Claude's Bash calls, so a variable set in one call is gone in the next. |
| D32 | The skills treat the exact wording of crook's messages as a guide, not a format to parse. | The binary plan does not fix that wording yet. |
| D33 | `crook close` kills the live children first, then marks the roster done, removes clean worktrees and prints its warnings, and kills the coordinator last. | Review: the coordinator runs `close` from inside its own terminal, so killing it first could kill `close` before the roster is marked and the warnings are seen. |
| D34 | The coordinator never uses AskUserQuestion while any child is live; every question to the maintainer is plain text. | Review: a blocked coordinator refuses child sends and may lose hc notes, for any question, not only the step-up one Q28 named. |
| D35 | Stop, close and step-up need consent typed in the coordinator's own pane; text arriving as `<hc from=…>` never counts as consent. | Review: a child, or text injected into a child, could otherwise impersonate the maintainer. |
| D36 | A slug starting with `t` and a digit is refused (`t7`, `t1x`). | hc refuses address-shaped names wider than `^t[0-9]+$` (it refused `t1x`); the coordinator's hc name is the bare slug. |
| D37 | Preflight's wait loop runs `server start` with the scrubbed environment before every `hc schema` retry. | Review: a bare `schema` could start a stopped core with the session's Claude variables still set. |
| D38 | The fake hc copies real hc 0.1.22 exactly (no labels from `run`, a bare-word status from `ls`, a finished parent accepted), checked against the captured fixtures. | Review: a fake kinder than real hc would let later milestones pass tests and fail for real. |
| D39 | A relative `CROOK_BIN` is refused, and the environment is carried as raw OS strings end to end. | Review: a relative path breaks in a child's own folder, and non-UTF-8 variables were being dropped from the core. |
| D40 | CI runs `claude plugin validate --strict plugin/` in the `plugin-check` job, installing the CLI with `npm ci` from the lock file in `.github/claude-code/` (Renovate's npm manager bumps it) on Node 24 (the CLI needs 22 or newer; jactionlint refuses an unlocked `npm install`). No JSON-shape fallback is needed. | The release plan left this unchecked. Checked: the npm package installs and `validate --strict` passes with an empty `HOME` and no login (2.1.332); the native binary comes from the package's postinstall, so `--ignore-scripts` breaks it. |
| D41 | `toolr ci plugin-check` is also a pre-commit hook (shape only, no tag compare); the CI job runs it with `--since-last-release` and fetches full history and tags. | The pre-commit job has no tags; the own job also hosts `validate`, so M2.6 is an own job. |
| D42 | The compatibility check covers agent files, skill folders and supporting files that a tagged `SKILL.md` names; other files may change. Tags sort by version with pre-releases below their release. | "Names that never change" freezes names a live conversation can still use; flagging every file would block harmless edits. |
| D43 | Agent bodies are capped at 30 lines and must name exactly one `crook-<role>` skill, with no `skills:` frontmatter key. | Plugin plan, agent files and Tier 1 structure test ("a line cap"); the real cap was not set, the files are 5 lines now. |

## Milestones

| Milestone | Status | Commit | Review findings |
| --- | --- | --- | --- |
| Foundation: release M0, binary 0 | done | `bb87293` | 1 blocker (runner drain could hang), 3 should-fix (grandchildren survive timeout, bypassable env guard, hook exit codes), 4 nits; all fixed |
| Release M1: CI | done except M1.6 | `d43a2db`, fixes in the next commit | 0 blockers, 2 should-fix (fail-open coverage uploads, component globs), 4 nits (3 fixed); M1.6 waits for the schema tests (D16) |
| Binary 1 and 2: paths, config, names, flock files, worktrees | done | `65a4569` | 2 blockers (worktree removal lost detached-HEAD commits and untracked files), 7 should-fix (ignored files, parent-repo status, relative XDG, mise shim path, roster/schema mismatch, half-made flock burns the slug, a test proving nothing), 5 nits; all fixed. Watch: `run::tests::a_timeout_takes_the_grandchildren_with_it` failed once under load, not reproduced in 10 runs. |
| Plugin M1 to M3: scaffold, hooks, both skills | done | `01c0013` | 1 cross-area blocker (`close` run from the coordinator could kill itself first: D33), 5 should-fix (consent spoofable through `hc send`: D35; worktree detection; repo paths pointing at the main checkout; a step-up that changes nothing; AskUserQuestion while children are live: D34), 5 nits; all fixed. Open: after `/clear` the session id may change; e2e checks it. |
| Binary 3 and 4: install, self-location, hc client, preflight | done | (next commit) | 0 blockers, 2 should-fix (preflight retry could start an unscrubbed core: D37; fake hc kinder than real hc: D38), 4 nits; all fixed. hc captures: names at most 32 characters (D27), every hc call but `--version` autostarts a core (D29). |
