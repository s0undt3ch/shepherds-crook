# Contributing

## Tooling

[mise](https://mise.jdx.dev) pins every tool: `mise install` gives you the Rust toolchain, Node, `prek`, `toolr` and
the linters. Install the git hooks with `prek install`.

Repo automation lives in `tools/` as [toolr](https://github.com/s0undt3ch/ToolR) commands, and the prek hooks and CI
call them. A fresh clone needs `toolr project trust` once, because toolr runs the repository's own Python.

Change the tools venv with `toolr project venv add`, `remove` and `lock`, never with `uv` directly. The `toolr-py`
floor in `tools/pyproject.toml`, the `aqua:s0undt3ch/ToolR` pin in `mise.toml`, `TOOLR_VERSION` in `.github/env` and
the version comment in `.github/actions/setup-toolr/action.yml` are one release. Renovate bumps them in one PR, and
`tests/tools/precommit/test_toolr_pins.py` fails when they drift.

## Checks

Run these before you push:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo nextest run
npm test
npm run typecheck
toolr project venv run -- coverage run -m pytest tests/tools
prek run --all-files
```

`cargo llvm-cov nextest` runs the Rust tests with coverage, as CI does. The release-only tests need
`cargo nextest run --release --run-ignored only`.

## What CI does not run

- **The mod's type-check.** `npm run typecheck` runs `tsc -p plugin/`, which needs Claude Code's own declaration file
  (`claude-code.d.ts`). It is not vendored, because of its licence, and a runner has none. It appears only when a
  logged-in Claude Code session loads the plugin-authoring skill. CI's pre-commit job sets `SKIP: tsc`, so a type error
  in `plugin/` only shows up locally. Run `npm run typecheck` before you push.
- **`claude plugin validate` and `claude plugin test`.** Whether they work on a runner without a login was not
  checked, so CI leaves them out. Run them locally against `plugin/` when you change the mod.

## Private words

`toolr pre-commit private-words` fails when a tracked file matches a regular expression in
`~/.config/crook-dev/private-words` (or the file named by `CROOK_PRIVATE_WORDS`), one per line. That file stays outside
the repository. When it is missing the hook passes, so other contributors are not affected.
