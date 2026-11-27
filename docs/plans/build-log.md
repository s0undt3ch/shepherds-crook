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

## Milestones

| Milestone | Status | Commit | Review findings |
| --- | --- | --- | --- |
| Foundation: release M0, binary 0 | done | (this commit) | 1 blocker (runner drain could hang), 3 should-fix (grandchildren survive timeout, bypassable env guard, hook exit codes), 4 nits; all fixed |
