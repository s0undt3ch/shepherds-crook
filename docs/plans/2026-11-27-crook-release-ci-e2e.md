# Plan: release, CI and e2e tooling for crook

Date: 2026-11-27. Area 4 of 4: release, CI and e2e. The other three plans cover the `crook` binary, the Claude
Code plugin and the herdr-app mod.

## Goal

Bring v1's build, release and test tooling back to `main`, adapted to the archive and install model in
[0019](../architecture/decisions/0019-ship-crook-as-a-binary.md) and
[0020](../architecture/decisions/0020-mechanical-steps-run-through-crook.md), as
[0022](../architecture/decisions/0022-rust-fresh-code-v1-tooling.md) asks. Add an end-to-end harness and a
rewritten `crook-e2e` skill that run crook against a scratch hc core, never the maintainer's real one.

## Scope

- CI on every push and pull request: lint, unit and integration tests, coverage, plugin and mod checks, a
  release-shaped archive build, a smoke run of each archive and a packslip check.
- The release workflow: version bump, changelog, archives, GitHub release, packslip signing and verification.
- The install smoke test with mise's `packslip:` backend, after each release and on a schedule.
- The e2e harness (`scripts/e2e/`), its guard tests (`tests/tools/`) and the `crook-e2e` skill.
- The order in which these must exist so the other three plans can check their first milestones.

## Non-goals

- The design of `crook`, the plugin or the mod. This plan only checks what the ADRs fix.
- Children on other machines (`hc run --on`). Out of scope by 0022.
- Running Claude in CI. CI has no Claude login.
- Driving the maintainer's real hc core or real herdr-app from the e2e harness. One opt-in, manual check is
  listed under "Local checks in the real app"; it needs the maintainer's yes each time.

## Fixed by the ADRs

- The archive holds the `crook` executable, `plugin/` and `mod/` (0019).
- The packslip manifest declares `bin = ["crook"]` and the completion `exec` resource (0020).
- The maintainer installs with `mise use -g packslip:github.com/s0undt3ch/shepherds-crook`; per-project installs
  are not supported (0019).
- Release builds never fall back to the repository's `plugin/` and `mod/`; they stop and say the install is
  broken (0019).
- Hooks and the mod call
  `"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}"`;
  `CROOK_BIN` overrides it for development (0019).
- `crook hook` exits 0 on any failure (0020).
- When crook starts a core, it removes Claude's session variables and `HC_TERMINAL_ID`, `HC_TERMINAL_SOCKET` and
  `HC_SCOPE` from that call (0020, spike 23).
- No database (0014), so nothing in the build needs SQLite.
- The `current` link sits under `${XDG_DATA_HOME:-~/.local/share}/crook/current` and points at the install folder;
  sessions use `current/plugin`. A debug build never moves it (0019, Q16).
- The config file has the keys `notes_root`, `default_child_model` (`sonnet` when unset) and `coordinator_model`
  (empty: the maintainer's default model) (0025, 0026; Q9, Q32).

## Decided in open-questions.md

The maintainer answered this plan's open questions in [open-questions.md](open-questions.md). The plan follows
them:

- Platforms (Q31): Linux (musl) and macOS, each on x86_64 and aarch64. herdr-core and herdr-app run on Linux.
  So T2 and T3 can run on a Linux machine, not only on macOS. Whether hc installs on a GitHub Linux runner is
  not checked; Q34 keeps real-core e2e out of CI. If that changes, T2 is the first tier to move.
- First release (Q33): `0.1.0-rc.1`, with a changelog that starts after `714568f`.
- CI limits (Q34): CI cannot run hc or herdr-app. Real-core e2e is local only. The mod gets `node --check` and
  shape checks in CI and `herdr-app check` in pre-commit. A crook-owned stub replaces `herdr.d.ts`.
- Schemas (Q5): `status --json` and `roster.json` each have a committed JSON Schema, checked in CI.
- Setup (Q22) and doctor (Q23): `setup`'s trust step only prints commands, so CI skips it. `doctor --json`
  reports the resolved `plugin/`, `mod/` and app home.
- E2E (Q28, Q32, Q36, Q37): a test checks whether hc holds or drops notes while the coordinator is blocked.
  `coordinator_model` gives a cheap coordinator. `hc keys <t> down enter` answers trust dialogs. The zsh pitfall
  is in spike 23.

## v1 files: keep, adapt or drop

Paths are on `backup_crook_v1`. "Keep" means copy as is. "Adapt" lists what must change. Files that `main`
already has are marked.

| v1 path | Verdict | What changes |
| --- | --- | --- |
| `.github/packslip.toml` | Adapt | Restore it from `backup_crook_v1` (M0.6). Set the bin to `crook` and add the completion `exec` resource (0020). Keep the header comment that names `toolr ci packslip-check` |
| `.github/env` | Keep (on `main`) | None. It names `tests/tools/precommit/test_toolr_pins.py`, which `main` lacks; M0 brings it back |
| `.github/workflows/release.yml` | Adapt | Do not rename it: the packslip identity pins `release.yml@refs/heads/main`. The release commit also owns the plugin's `plugin.json` (see "Versioning"). `ARCHIVE_PATTERNS` follows the target list |
| `.github/workflows/_build-binary-archive.yml` | Adapt | Do not rename it: provenance names it. Drop the rusqlite `CC_<triple>=musl-gcc` export. Replace the inline staging with `toolr ci stage-archive`, which adds `plugin/` and `mod/` next to `crook` |
| `.github/workflows/_smoke-binary-archive.yml` | Adapt | Drop the `hook PreToolUse` → `ask` check. Add the 0019 and 0020 checks listed in M4 |
| `.github/workflows/install-smoke.yml` | Adapt | `mise use -g` with a scratch mise home, run through the shim, check `plugin/` and `mod/` in the install folder, run `crook setup` on scratch homes |
| `.github/workflows/ci.yml` | Adapt (on `main`, pre-commit job only) | Bring back the test, build, packslip-check and smoke jobs. Replace `test-mod` (v1's Claude Code TS hooks) with plugin and mod check jobs. Replace `SKIP: tsc` with a `SKIP` for the `herdr-app check` hook, which CI cannot run (Q34) |
| `.github/workflows/codeql.yml` | Adapt (on `main`) | Add `rust` and `javascript-typescript` back to the matrix |
| `.github/codeql/codeql-config.yml` | Adapt (on `main`) | Restore `paths-ignore` for test code, with paths from the binary plan's layout |
| `.github/actions/pipeline-gate`, `setup-toolr` | Keep (on `main`) | None |
| `.github/actions/setup-pre-commit` | Adapt (on `main`) | Restore the `rustup component add clippy rustfmt` step |
| `.github/actions/apply-release-patch`, `configure-git` | Keep | None |
| `.github/actions/codecov-stage-test-results`, `codecov-publish-test-results` | Keep | None |
| JSON Schemas for `status --json` and `roster.json` | Binary plan | The binary plan commits them. CI validates against them (Q5, M1.6) |
| `.codecov.yml` | Adapt | Replace v1's components (`privacy`, `state`, `cli`) with the binary plan's modules; add `plugin` and `mod` only if they carry code with tests; fix `after_n_builds`; fix `ignore` |
| `.config/nextest.toml` | Adapt | Keep the `ci` JUnit profile. Drop the `release-timing` group: its filters name v1 tests |
| `cliff.toml` | Keep | The first release, `0.1.0-rc.1`, uses a commit range that starts after `714568f`; later releases rely on tags (Q33) |
| `tools/ci.py` | Adapt | Keep `targets` and `packslip-check`. Add `stage-archive`, `archive-check` and `plugin-check` |
| `tools/release.py` | Adapt | `bump` also writes `plugin.json`'s `version`. The mod has no version file |
| `tools/precommit/*` | Keep (on `main`) | None |
| `tools/pyproject.toml`, `tools/uv.lock` | Adapt (on `main`) | Add back `coverage`, `packaging` and `pytest`; relock |
| `tests/conftest.py`, `tests/tools/ci/*`, `tests/tools/release/*`, `tests/tools/precommit/*` | Adapt | Update the packslip and bump tests for the new checks |
| `tests/tools/test_repo_safety.py` | Adapt | Keep the workflow checks. Rewrite the e2e checks for the hc harness (M5) |
| `tests/tools/test_e2e_scripts.py` | Rewrite | Stub `hc` and `claude` instead of `herdr` |
| `.coveragerc` | Keep | None |
| `.pre-commit-config.yaml` | Adapt (on `main`) | Add back rustfmt, clippy, cargo-lock and no-inline-test-modules. Add `herdr-app check` as a pre-commit hook that CI skips (Q34). Do not add `sqruff-fix` or `tsc` |
| `.rustfmt.toml` | Keep | None |
| `mise.toml` | Adapt (on `main`) | Add back rust (with clippy and rustfmt), cargo-nextest, cargo-edit, cargo-llvm-cov and git-cliff. Add node only if the mod plan uses npm. Drop sqruff |
| `mise.ci.toml`, `mise.ci.lock` | Keep | Relock with `MISE_ENV=ci mise lock` |
| `renovate.json5` | Adapt (on `main`) | Restore the packslip pin manager, the Rust toolchain group and the cargo label. Restore the npm rule only if the mod plan uses npm |
| `.gitignore` | Adapt (on `main`) | Add back `target/`, `coverage/`, `.coverage`, `.coverage.*`; `node_modules/` if npm is used |
| `Cargo.toml` | Binary plan | The binary plan writes it. This plan needs the fields listed under "Dependencies" |
| `.claude/skills/crook-e2e/SKILL.md` | Rewrite | hc instead of herdr panes; see M6 |
| `scripts/e2e/*` | Rewrite | Keep the guard ideas (`load_run`, `in_run_path`, refusing panes and pids outside the run); replace herdr calls with hc |
| `.sqruff`, `package.json`, `vitest.config.ts`, `scripts/typecheck.mjs`, `build.rs` | Drop | No SQL. v1's npm setup and type-check served Claude Code TS hooks, not a herdr-app mod. `build.rs` embedded v1's mod |
| `plugin/`, `src/`, `tests/rust/`, `spikes/`, `specs/` | Drop | v1 design; not tooling (0002, 0022) |
| `CHANGELOG.md` | Drop | Generated fresh at the first release |

The unmerged patch `Claude outputs/0001-ci-mod-typecheck-stub.patch` also targets Claude Code's types, not
herdr-app's. Drop it.

## Versioning

- Versions are `X.Y.Z` or `X.Y.Z-rc.N`, as v1's `toolr release bump` enforces (`tools/release.py`).
- The release bumps two things: the binary's version (`Cargo.toml`, as in v1) and `version` in `plugin.json`. The
  mod has no version file. Its `.crook-version` marker is written by `crook setup` from the binary's version
  (0019), so the mod follows the binary with no stamp.
- `toolr release bump` writes every file that holds a version. The release commit may change only those files
  plus `CHANGELOG.md`; v1's `release.yml` enforces that list, so the list grows by `plugin.json`.
- The release is two commits. The first carries the stamps and is the one tagged. The second,
  `chore(release): Prepare for next development`, puts `plugin.json` back to `0.0.0-dev` (Q17), so `main` and debug
  builds read the dev stamp. Both are pushed with the tag. Tests in `tests/tools/release/` check that `bump`
  leaves the binary's and `plugin.json`'s versions equal, and that `plugin.json` on `main` is `0.0.0-dev`.
- The first release is `0.1.0-rc.1`. Its changelog starts after `714568f`, v1's last commit, through an explicit
  range for that release only. Later releases use tags (Q33).

## Milestones

Each task names its files, the ADRs it serves, how it is checked and when it is done.

### M0. Tooling base

Must land before any other plan's first milestone can be checked in CI.

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M0.1 Pin the Rust toolchain and test tools | `mise.toml`, `mise.lock` | 0022 | `mise install` on a clean machine; `cargo --version`, `cargo nextest --version`, `cargo llvm-cov --version` | Pins committed, lock refreshed |
| M0.2 Restore ignores | `.gitignore` | 0022 | `git status` clean after a build and a test run | No build output shows as untracked |
| M0.3 Restore the tools venv and its tests | `tools/pyproject.toml`, `tools/uv.lock`, `tests/conftest.py`, `tests/tools/precommit/*`, `.coveragerc` | 0022 | `toolr project venv run -- coverage run -m pytest tests/tools` | Passes; `test_toolr_pins.py` exists again, as `.github/env` claims |
| M0.4 Restore Rust hooks | `.pre-commit-config.yaml`, `.rustfmt.toml`, `.github/actions/setup-pre-commit/action.yml` | 0022 | `prek run --all-files` | rustfmt, clippy, cargo-lock and no-inline-test-modules run; they pass on an empty crate |
| M0.5 Renovate | `renovate.json5` | 0022 | Renovate's config validator, or the Renovate dashboard after merge | Rust, cargo and packslip rules back |
| M0.6 Restore the packslip manifest | `.github/packslip.toml` | 0020, 0022 | Copy from `backup_crook_v1`; `toolr ci packslip-check` once M4.4 exists, and until then the TOML parses | The file is on `main` with `bin = ["crook"]` and the completion `exec` resource, and no `skill` resources |

### M1. CI for the binary

Needs the binary plan's first `Cargo.toml` and an empty `main.rs`.

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M1.1 Test jobs on Linux and macOS | `.github/workflows/ci.yml`, `.config/nextest.toml` | 0022 | Workflow run | `cargo llvm-cov nextest --profile ci` and `cargo nextest run --release --run-ignored only` pass on both |
| M1.2 Coverage upload | `.codecov.yml`, `.github/actions/codecov-*`, `mise.ci.toml`, `mise.ci.lock` | 0022 | Codecov PR comment | One comment with linux, macos and tools flags; `codecov-notify` runs last |
| M1.3 Tools tests job | `.github/workflows/ci.yml` | 0022 | Workflow run | `test-tools` uploads `coverage/tools.xml` |
| M1.4 CodeQL for Rust | `.github/workflows/codeql.yml`, `.github/codeql/codeql-config.yml` | 0022 | Workflow run | `rust` analysis passes; test paths ignored |
| M1.5 Gate job | `.github/workflows/ci.yml` | 0022 | `toolr pre-commit check-gate-needs` | The gate needs every job |
| M1.6 Schema checks (Q5) | the schema files, `.github/workflows/ci.yml` | 0024 | Test jobs | The JSON Schemas for `status --json` and `roster.json` are valid; the binary's tests validate real output against them on both OSes |

### M2. Plugin checks

Needs the plugin plan's first `plugin/` tree. None of these needs Claude.

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M2.1 JSON and frontmatter shape | `tools/ci.py` (`plugin-check`), `tests/tools/ci/test_plugin_check.py` | 0007, 0019, 0025 | Unit tests with good and bad fixture plugins | `plugin.json` and `hooks/hooks.json` parse; each `agents/<role>.md` has frontmatter with a name; each `skills/*/SKILL.md` has `name` and `description` |
| M2.2 Hook commands call the shim | same | 0019, 0020 | Fixture with a bare `crook` in a hook fails | Every hook command is `"${CROOK_BIN:-${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook}" hook <event>` |
| M2.3 Roles stay thin and name existing skills | same | 0019, 0025 | Fixture with an agent file naming a missing skill fails | Each agent file names a `crook-<role>` skill that exists in `plugin/skills/` |
| M2.4 No skill removed since the last release | same | 0019 | Fixture git repo with a tag whose agent files name a skill that was then removed | `plugin-check --since-last-release` reads agent files at the newest `v*` tag; skipped with a notice when no tag exists |
| M2.5 Markdown | existing `rumdl` and `typos` hooks | 0005 | `prek run --all-files` | Covers `plugin/**/*.md` |
| M2.6 CI job | `.github/workflows/ci.yml` | 0022 | Workflow run | `plugin-check` runs in the pre-commit job or its own job |

`claude plugin validate` might add a check, but whether it runs without a login is not checked. Leave it out of
CI until someone tries it on a runner.

### M3. Mod checks

Needs the mod plan's first `mod/` tree. `herdr-app check` needs herdr-app, which CI does not have (Q34).

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M3.1 Syntax check of every mod script (Q34) | `tools/ci.py` (`mod-check`) | 0019 | Fixture with a syntax error fails | `node --check` passes on each `.js` file in `mod/` |
| M3.2 `mod.json` shape (Q34) | same | 0019 | Fixture with a missing field fails | Fields the mod plan names are present |
| M3.3 Mod exec calls go through the shim | same | 0019 | Fixture with `exec("crook", …)` fails | Every `exec` of crook uses `CROOK_BIN` or the shim path |
| M3.4 The mod's own tests, if any | as the mod plan sets up | 0019 | CI job | Runs, validates against the committed schemas (Q5) and uploads coverage under a `mod` flag |
| M3.5 `herdr-app check mod/` in pre-commit (Q34) | `.pre-commit-config.yaml`, `.github/workflows/ci.yml` (`SKIP`) | 0019 | Local `prek run`; CI log shows it skipped | Runs when `herdr-app` is on `PATH`; CI sets `SKIP` for it, as v1 did for `tsc` |
| M3.6 CodeQL for JavaScript | `.github/workflows/codeql.yml` | 0022 | Workflow run | `javascript-typescript` analysis passes |

The mod is not type-checked against herdr-app's `herdr.d.ts`: its licence is unchecked, so crook owns a stub for
the calls the mod uses (Q34). The mod plan writes the stub; if it adds a type check, CI runs it against the stub.
herdr-app runs on Linux (Q31), so a Linux runner might install it one day. That is not checked, and Q34 keeps
`herdr-app check` out of CI.

### M4. Archives, smoke and packslip check on every PR

Needs: the binary prints `crook <version>` for `--version`, has `completion print <shell>`, `doctor --json` and
`hook stop`; the plugin and mod plans have a `plugin/.claude-plugin/plugin.json` and a `mod/mod.json`.

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M4.1 One staging command | `tools/ci.py` (`stage-archive`), tests | 0019, 0022 | Unit test stages into a temp dir | Writes `crook-<version>-<triple>/` with `crook`, `plugin/`, `mod/`, `LICENSE`, `README.md` and `CHANGELOG.md` when present. CI and the e2e harness both use it |
| M4.2 Build job uses it | `.github/workflows/_build-binary-archive.yml` | 0019 | Workflow run | Archives for every target in `toolr ci targets`: Linux (musl) and macOS, each on x86_64 and aarch64 (Q31). Not attested here: the release attests in a job that runs no project code (D110) |
| M4.3 Archive layout check | `tools/ci.py` (`archive-check`), `.github/workflows/ci.yml` | 0019 | Fixture tarballs: one good, one without `mod/` | Each tarball has exactly one top folder holding `crook`, `plugin/.claude-plugin/plugin.json` and `mod/mod.json`. mise strips a lone top folder (spike 18), so these land next to `crook` |
| M4.4 Packslip check | `tools/ci.py` (`packslip-check`) | 0019, 0020 | Existing tests, updated | One `crook` bin per artifact; Linux is musl; no `skill` resources (they would install user-wide, against 0019); the completion resource lists bash, zsh and fish |
| M4.5 Smoke the shipped archive | `.github/workflows/_smoke-binary-archive.yml` | 0019, 0020 | Workflow run on each target | See the list below |

The smoke job runs the extracted release binary with `env -i`, `HOME` and every XDG folder in a temp dir, and a
scratch `HERDR_APP_HOME`:

- `crook --version` prints `crook <version>`; on a release, exactly the release version.
- `crook --help` exits 0.
- `crook completion print bash|zsh|fish` prints something that names `crook`.
- `crook hook stop </dev/null` exits 0, and so does `crook hook session-start` with bad JSON on stdin (0020).
- `crook setup` runs in full, because its trust step only prints the commands and CI skips them (Q22). It writes
  `<HERDR_APP_HOME>/mods/crook/.crook-version` with the version, and points `<XDG_DATA_HOME>/crook/current` at
  the archive's folder, so `current/plugin` is the archive's `plugin/` (0019, Q16, Q22).
- `crook doctor --json` reports the resolved `plugin/`, `mod/` and app home, all inside the archive and the scratch
  `HERDR_APP_HOME`. With no mise shim it reports the shim as missing and names `CROOK_BIN` as the fix (Q23).
- With `plugin/` renamed away, a command that needs it exits non-zero and says the install is broken (0019).

### M5. E2E harness base

Needs the binary plan's `crook new`, `open`, `spawn` and `hook`. The design is in "E2E harness" below.

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M5.1 Run layout and guards | `scripts/e2e/lib.sh`, `scripts/e2e/setup.sh` | 0016, 0020, 0026 | `tests/tools/test_e2e_scripts.py` with stub `hc`, `claude` and `crook` | `setup.sh` makes a run under `$TMPDIR/crook-e2e-run.*` and a socket folder under `/tmp/crook-e2e-rt.*`, and writes `config/crook/config.toml` with a cheap `coordinator_model` (Q32); `load_run` refuses an env file that points elsewhere |
| M5.2 Scrubbed wrappers | `scripts/e2e/hc.sh`, `scripts/e2e/crook.sh` | 0016, 0019, 0020 | Stub test logs each wrapped call's environment | Every call has the run's `HC_SESSION`, `XDG_*` and `HERDR_APP_HOME`; none has a variable from 0020's list; the wrapper refuses when `HC_SESSION` is not the run's |
| M5.3 Staged install and fake shim | `scripts/e2e/install.sh` | 0019 | Stub test | Unpacks `stage-archive` output to `$RUN/install/<version>/`; writes `$RUN/shims/crook`, which runs the version named in `$RUN/active` |
| M5.4 Core start check | `scripts/e2e/core_env.sh` | 0020 | Real hc, local | After crook starts the scratch core, a probe terminal running `env` shows none of 0020's variables |
| M5.5 Trust answering, guarded | `scripts/e2e/trust.sh` | 0013, 0023 | Stub test: a dialog naming a path outside the run is refused | Answers with `hc keys <terminal> down enter` (Q36), only for paths under `$RUN` |
| M5.6 Teardown | `scripts/e2e/teardown.sh` | 0014 | Stub test, then a real run | Stops the run's core only, ends leftover processes whose working folder is in the run, removes both run folders, exits non-zero and keeps the run when anything is left |
| M5.7 Tier 2 scenarios with a stub `claude` | `scripts/e2e/stubs/claude`, `scripts/e2e/tier2.sh` | see scenarios | Real hc, local only (Q34) | Scenarios marked T2 below pass |

### M6. The `crook-e2e` skill and Tier 3 scenarios

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M6.1 Rewrite the skill | `.claude/skills/crook-e2e/SKILL.md` | 0022 | A full local run driven by Claude | Limits, setup, steps, checks, mistakes and report sections cover hc, not herdr |
| M6.2 Scenarios S1 to S13 | the skill, `scripts/e2e/*` | see scenarios | Each scenario's pass check | Every scenario passes, or fails with evidence of a crook bug; S5 records whether hc holds or drops notes while the coordinator is blocked (Q28); the E1 to E7 mapping is below |
| M6.3 Rerun spikes 14 and 15 through crook | the skill | 0020 | S4, S5 and S6 | 0020's "after the first build" item is done |

### M7. Release and install smoke

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M7.0 Check the release setup | the repository's settings | 0022 | The maintainer reads the settings | The `release` environment exists, with `GH_APP_CLIENT_ID` as an environment variable and `GH_APP_PRIVATE_KEY` as an environment secret; M7.1 does not start before this |
| M7.1 Release workflow | `.github/workflows/release.yml`, `.github/actions/apply-release-patch`, `.github/actions/configure-git`, `tools/release.py`, `cliff.toml` | 0019, 0022 | Dispatch `0.1.0-rc.1` from `main` (Q33) | Tag, GitHub release with archives and `.sha256` files, `packslip.sigstore.json`; the changelog starts after `714568f` |
| M7.5 CI prepares the release | `.github/workflows/_prepare-release.yml`, `ci.yml`, `release.yml`, `tools/release.py` (`next-version`), `cliff.toml` (`[bump]`) | 0022 | Every CI run | As in ToolR: one reusable workflow prepares the release. `release.yml` calls it with the dispatched version in the `release` environment; `ci.yml` calls it with none, so it prepares a dev version after the latest tag (`toolr release bump` with no version, ported from ToolR's `tools/version.py`: `X.Y.(Z+1)-devN`, with `+g<sha>` on pull requests), and CI's build and smoke jobs run on that release patch. Nothing is pushed from CI (D124) |
| M7.2 Packslip verification | `release.yml` (`publish-packslip`) | 0019 | The job's own `packslip verify` step | Identity is `…/release.yml@refs/heads/main`; version and commit match |
| M7.3 Install smoke | `.github/workflows/install-smoke.yml` | 0019, 0020 | Dispatched by the release, and nightly | See the list below |
| M7.4 Record the result | `docs/architecture/README.md` "Packaging" | 0019 | Review | The README's "first release confirms it" note is settled with the run's link |

The install smoke runs on each runner in v1's matrix with a scratch `HOME` only. It leaves `MISE_DATA_DIR` and
`XDG_DATA_HOME` unset, so mise's data folder takes its default place under that `HOME`. The hooks' fallback path
is fixed (0019), so the smoke must test that exact path, not a custom mise folder. Where mise puts its data by
default is not checked here; the first step below checks it.

- `mise use -g packslip:github.com/s0undt3ch/shepherds-crook@<version>` succeeds.
- `${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook` exists. Spike 23 saw no shim for a linked
  packslip version; this checks a real install does get one, at the path the hooks call.
- `${MISE_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/mise}/shims/crook --version` prints `crook <version>`.
- `$(mise where packslip:github.com/s0undt3ch/shepherds-crook)` holds `crook`, `plugin/.claude-plugin/plugin.json`
  and `mod/mod.json`.
- `crook setup`, run through the shim on scratch homes, points `current` at the mise install folder. This
  confirms that a shim-started crook finds its own install folder (0019 "Confirmation"). Its trust step only
  prints commands (Q22).
- `crook doctor --json` shows the same install folder, `plugin/`, `mod/` and app home, and reports the shim path
  as present (Q23).
- `crook completion print zsh` is not empty.

### M8. After the second release

| Task | Files | ADRs | Check | Done when |
| --- | --- | --- | --- | --- |
| M8.1 Upgrade smoke | `.github/workflows/install-smoke.yml` | 0019 | Install the previous release, run `crook setup`, then `mise upgrade` to the new one | `current` points at the new install folder; running the old binary by its install path leaves `current` alone |
| M8.2 Changelog check | release run | 0022 | Read the generated notes | Notes cover only commits since the last final tag |

## E2E harness

### Run layout

- `$RUN`: `$(realpath "$TMPDIR")/crook-e2e-run.XXXXXX`. Holds `repo/` (a git repository with an initial commit
  and `.worktrees/` in its `.gitignore`), `remote.git` (a bare remote for the push rule in 0023), `data/`,
  `config/`, `state/`, `herdr-app/`, `install/`, `shims/` and `run.env`.
- `$RT`: `/tmp/crook-e2e-rt.XXXXXX`, used as `XDG_RUNTIME_DIR`. It is short because a Unix socket path has a
  length limit (about 104 bytes on macOS); `$TMPDIR` on macOS is long. The limit is general knowledge, not
  measured here. Claude never runs or trusts anything in `$RT`. `load_run` checks `$RT` against its own prefix.
- `HC_SESSION`: `crook-e2e-<run id>`.
- `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`: under `$RUN`. They hold the flocks (0004), the config
  file (0026), hc's state and the `current` link (Q16), so a run never moves the maintainer's real link. A debug
  build used as `CROOK_BIN` never moves `current` either.
- `HERDR_APP_HOME`: `$RUN/herdr-app`, for `crook setup` (spike 17 showed `appHome()` follows it).
- `CROOK_BIN`: `$RUN/shims/crook`. It is set on the call that starts the core, so every terminal gets it (spike
  23), and `crook spawn` also passes it with `env` (0019).
- `HOME` and `CLAUDE_CONFIG_DIR` are the run's in tier 3 too (D122). `login.sh` copies the maintainer's Claude
  login into the run without its refresh token (on macOS a keychain item named after the run's config folder,
  reached through a `Library/Keychains` link in the run's `HOME`), seeds the run's `.claude.json` from the
  maintainer's without `projects` and `mcpServers`, and teardown deletes the copy. The run's `settings.json` sets
  only the permission mode and `tui`.

### Safety rules

- Every `hc` and `crook` call goes through `scripts/e2e/hc.sh` or `scripts/e2e/crook.sh`. Each sets the run's
  `HC_SESSION` and `XDG_*`, removes 0020's whole variable list, and refuses to run when the result does not point
  at the run. A harness started from a herdr-app terminal has `HC_TERMINAL_SOCKET` set to the real core, so the
  removal is what keeps it off that core. How hc ranks `HC_SESSION` against `HC_TERMINAL_SOCKET` is not checked;
  removing the second makes the order moot.
- The scripts are bash, with the `env -u` list built as a bash array. Spike 23 records the pitfall (Q37): zsh
  does not split an unquoted `$var`, so a list of `-u NAME` flags kept in one string reaches `env` as one
  argument and removes nothing. The Bash tool's shell here is zsh, so no scrub may be typed inline.
- `core_env.sh` checks the result, not the command: it reads a probe terminal's `env` after the core starts.
- Trust is answered only for paths under `$RUN` (v1 `scripts/e2e/trust.sh` rule, and the maintainer's standing
  permission for `$TMPDIR/crook-e2e-*`).
- Processes are ended only when their working folder is under `$RUN` (v1 `kill_in_run.sh`).
- A step passes on what `roster.json`, `results/.delivered.json`, `hc ls --all`, `hc get`, `git` and `ps` show,
  never on what a model says (v1 skill, "Steps").

### Models

- Children run on Haiku: `crook spawn --model haiku` (0025). Without `--model`, `spawn` uses
  `default_child_model`, `sonnet` when unset (Q9).
- The coordinator runs on `coordinator_model` from the run's config file (0026, Q32). `setup.sh` sets it to a cheap
  model, so the maintainer's default model is never used for a test run.
- Tier 2 uses a stub `claude`: a script that logs its argv and environment, writes `<session-id>.jsonl` where a
  real session would, and pipes hook JSON into `"$CROOK_BIN" hook <event>`. Whether `hc get` reports a stub as
  app `claude-code` is not checked. If it does not, the stop hook's status check (0009) skips the send, and the
  delivery scenarios stay Tier 3.

### Tiers

| Tier | Where | Uses | Covers |
| --- | --- | --- | --- |
| T1 | CI, every PR | Rust tests with stub `hc` and `claude` on `PATH` (binary plan) | Argument order, roster writes, preflight, hook exit codes |
| T2 | Local only (Q34) | Real scratch hc core, stub `claude` | Core start, labels, spawn argv, resume under the original name, worktree rules |
| T3 | Local only, driven by Claude with the skill | Real scratch hc core, real Claude on Haiku | Everything, including hooks inside live sessions |

hc and herdr-app run on Linux (Q31), so T2 and T3 can run on a Linux machine too. Whether hc installs on a GitHub
Linux runner is not checked; Q34 says CI cannot run it, so T2 stays local.

### Scenarios

| # | Scenario | ADRs, spikes | Tier | Pass when |
| --- | --- | --- | --- | --- |
| S1 | Core start with the scrub | 0016, 0020; spike 23 | T2, T3 | No core runs for `HC_SESSION`; `crook new` starts one; a probe terminal's `env` has none of 0020's variables; Claude children show no "Transcript saving is off" line |
| S2 | New and open | 0004, 0015, 0018, 0020, 0021, 0024; Q1, Q2, Q7, Q8, Q18, Q21 | T2, T3 | `crook new <slug>` starts it, and a second `new` with the same slug is refused; `roster.json` matches 0024, validates against its schema and has `started` set after the first launch; the coordinator's argv has `--add-dir <notes root>/<date>-<slug>` and `--settings` before `--agent crook:coordinator`, and the configured `coordinator_model`; `hc ls` shows the coordinator with `crook.flock`, `crook.version`, `herdr.space`, `herdr.tab`; `crook open` succeeds with no herdr-app watching the core, and on a live coordinator only redoes the renames |
| S3 | Spawn | 0007, 0013, 0020, 0021, 0025, 0027; Q3, Q11, Q14, Q15, Q36, Q46; spikes 10, 16, 27 | T2, T3 | Roster entry with name, model, session id, brief; `hc ls` shows the child, named `<slug>.<child>`, under the coordinator with `crook.flock` and no `herdr.*` label; `ps` argv has `--plugin-dir <data>/crook/current/plugin`, `--agent crook:child`, `--model haiku`, `--session-id <id>` and the prompt last, and the prompt names the brief and the result path; `<id>.jsonl` exists; a launch that fails leaves no roster entry; the coordinator's `crook spawn`, called as the literal path, runs with no Bash permission prompt under `--permission-mode default`, because `crook open` passed `--settings` allowing that path (0027, Q46). Low priority: a trust dialog answered with `hc keys <t> down enter` still delivers the first prompt (spike 16, Q36); `--add-dir` on an already trusted root starts with no prompt (spike 10, 0013) |
| S4 | Stop-hook delivery | 0008, 0009; spike 14 | T3 | The child writes `results/<child>.md`; the coordinator's screen (`hc read`) shows an `<hc from=…>` line with that path; `.delivered.json` holds the SHA-256 of the whole file (Q27); a turn that changes nothing sends nothing |
| S5 | Blocked coordinator, then rescan | 0008, 0010; spike 14; Q28 | T3 | With the coordinator on an AskUserQuestion, a child's update is sent and the test records whether hc holds or drops it; the mark matches that outcome (held: moves on delivery; dropped: stays old); after the answer, the next prompt's hook output names the result and the mark updates. Until the result is in, the coordinator asks about a model step-up in plain text |
| S6 | Core stop and resume | 0017, 0018, 0020; spike 15 | T2, T3 | `hc server stop` mid-task; `crook open` resumes the coordinator with `--resume <id>`; its SessionStart hook resumes each `active` child under its original name and skips `done` ones, and starts no core (Q25); `roster.json` has no new entries; the child's checklist continues |
| S7 | Upgrade self-reload | 0019; spikes 18 to 20; Q16, Q26 | T3 | Two staged versions; a session starts on A; `active` moves to B and B runs once; `current` points at B's install folder; at the next turn end the session gets `/reload-plugins`; the first prompt after that gets a one-line nudge to load the skill again; a skill that prints its version answers B; running A again leaves `current` on B, and so does a debug build |
| S8 | Worktree rules | 0023; spike 26; Q13 | T2, T3 | `spawn --repo` makes `<repo>/.worktrees/<flock>-<child>` on `crook/<flock>/<child>`; `stop` keeps a dirty worktree, keeps an unpushed one, removes a pushed or merged one; the branch always remains; each refusal says why. Pushed means a remote-tracking branch contains the tip; merged means the tip is an ancestor of the main worktree's branch, and a branch with no new commits counts as merged (Q13) |
| S9 | Model change on resume | 0025; spike 26; Q10 | T2 (argv), T3 (memory) | `crook resume <child> --model sonnet` is refused while the child works; otherwise it ends the child's terminal without marking it done, updates the roster's model and resumes; `ps` argv has `--model sonnet --resume <id>`; the child recalls a word from before |
| S10 | Stop and close | 0018, 0020, 0023; Q6 | T2, T3 | A stopped child is `done` and is not resumed by S6's path; `crook close` kills the coordinator, marks the roster done, removes clean worktrees and prints a `warning:` line for each worktree it keeps (S8's rules) |
| S11 | Blocked child | 0010; spike 3 | T3 | A child briefed to ask with AskUserQuestion shows `blocked` in `hc ls`; the coordinator gets hc's blocked note, names the child and the question to the maintainer, and does not answer for them (its transcript has no `hc send` to that child that carries an answer) |
| S12 | Step-up question | 0025; Q28 | T3 | A child's result shows `status: failed`; the coordinator asks the maintainer in plain text, not with AskUserQuestion, before it runs `crook resume <child> --model`; nothing is resumed on a new model until the maintainer says yes; after a yes, S9's checks hold |
| S13 | Mod install and upgrade | 0019; Q22; Mod M5.2 | T2 | The six steps in Mod M5.2 pass against a scratch `HERDR_APP_HOME`: fresh setup; refusal on an unmarked `mods/crook/`; replace over an older marker; refusal over a newer marker; other commands leave the mod alone; `herdr-app check` passes (local only, Q34). The steps live in the mod plan, not here |

### Plugin e2e steps to scenarios

The plugin plan's steps E1 to E7 (P6.1) run as these scenarios. The plugin plan keeps each step's own checks, such
as the README listing; the scenario supplies the run.

| Plugin step | Scenario |
| --- | --- |
| E1 Plan and spawn two children, one with `--repo` | S2, S3, S8 |
| E2 Both children finish | S4 |
| E3 A child asks with AskUserQuestion | S11 |
| E4 A failed child, step-up after a yes | S12, S9 |
| E5 Core stop, then `crook open` | S6 |
| E6 `crook stop` with uncommitted work, then after a push | S8, S10 |
| E7 `current` moves mid-session | S7 |

### Local checks in the real app

S2's space and tab names, the Agents tree (0021) and the mod's dialog need the maintainer's real herdr-app,
because only that core is watched by the app (spike report, introduction). These stay a manual checklist in the
skill, run only when the maintainer says so, with throwaway labels and `space.close` afterwards, as spikes 24 and
25 did.

## Order against the other plans

| Other plan's milestone | Needs from this plan first |
| --- | --- |
| Binary: first crate builds and tests | M0, M1 |
| Plugin: first agents, skills and hooks | M0, M2 |
| Mod: first `mod.json` and script | M0, M3 |
| Any claim about the archive layout or the shim | M4 |
| Binary: `new`, `open`, `spawn`, `resume` work against hc | M5 (T2), then M6 (T3) |
| First release | M4, M7 |

## Dependencies on the other plans

- Binary plan:
  - `Cargo.toml` with `name = "crook"`, a `version`, `rust-version` equal to the mise Rust pin, `repository`,
    and v1's `[profile.release]` settings.
  - `crook --version` prints exactly `crook <version>`.
  - The `current` link sits under `${XDG_DATA_HOME:-~/.local/share}` and points at the install folder (Q16).
  - `crook setup` honours `HERDR_APP_HOME`, and its trust step only prints commands (Q22).
  - `crook doctor --json` reports the resolved `plugin/`, `mod/` and app home, and a missing shim (Q23).
  - The config file keys `coordinator_model` and `default_child_model` (Q32, Q9).
  - The committed JSON Schemas for `status --json` and `roster.json` (Q5).
- Plugin plan: one version field; hook commands as 0019 writes them; agent files that name `crook-<role>`
  skills.
- Mod plan: `mod/mod.json`; whether it uses npm and tests; the crook-owned stub that replaces `herdr.d.ts` (Q34).

## Risks

- hc is at 0.1.x (0002, 0016). A flag change breaks T2 and T3 runs before it breaks anything in CI.
- 0020's variable list follows Claude Code's internals. A new session variable would leak into scratch children
  until the list grows. `core_env.sh` catches names from the list only; it should also flag any `CLAUDE_CODE_*`
  name that is not a known user setting.
- T3 depends on model behaviour. As in v1, a step where the model does not follow the prompt is recorded as "not
  exercised", not as a crook failure.
- A T3 run needs an access token with an hour left, because the copy cannot refresh (D122).
- If `coordinator_model` is unset in a run's config, the coordinator runs on the maintainer's default model and
  the run costs more. `setup.sh` sets it (Q32).
- `hc send` can wait on a draft longer than a hook's time budget (120 seconds for session-start, 30 for the
  others) (Q25). S4 may then show a timeout, not a delivery.
- The first install smoke may find that mise makes no shim for a packslip install. 0019 would then need
  revisiting; the plan cannot work around it.
- `release.yml` needs the `release` environment, its variable `GH_APP_CLIENT_ID` and its secret
  `GH_APP_PRIVATE_KEY`. Whether they still exist on the repository is not checked; M7.0 checks it before the first
  release.

## Open questions

The maintainer decided all of them. The answers are in [open-questions.md](open-questions.md). This plan's own
numbers map to it as follows.

1. Platforms. Decided: see Q31 in open-questions.md.
2. `crook setup` and the trust step. Decided: see Q22 in open-questions.md.
3. Where `current` lives. Decided: see Q16 in open-questions.md.
4. Finding crook's own folders in tests. Decided: see Q22 and Q23 in open-questions.md.
5. A cheap coordinator for e2e. Decided: see Q32 in open-questions.md.
6. The first changelog. Decided: see Q33 in open-questions.md.
7. hc on CI runners. Decided: see Q34 in open-questions.md.
8. Answering trust dialogs through hc. Settled: see Q36 in open-questions.md.
9. herdr-app's `herdr.d.ts`. Decided: see Q34 in open-questions.md.
10. First version number. Decided: see Q33 in open-questions.md.
11. The zsh pitfall. Settled: see Q37 in open-questions.md.
12. Release secrets. Decided as a to-do before the first release: see M7.0 and the end of open-questions.md.
13. A non-default mise data folder. Decided: see Q23 in open-questions.md; `doctor` reports a missing shim and
    names `CROOK_BIN` as the fix.
