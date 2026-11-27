# Contributing

## Tooling

Every tool is pinned in `mise.toml`. Run `mise install`, then `prek install` to add the git hooks. The hooks
run typos, rumdl, gitleaks, shellcheck and `adrs doctor`, among others. Run them all with
`prek run --all-files`.

## Design decisions

The design is a set of architecture decision records (ADRs) in MADR form, in `docs/architecture/decisions/`.
[`docs/architecture/README.md`](docs/architecture/README.md) indexes them, with each one's status, and lists the
open questions. The ADRs are the spec: code and plans follow them, and a change of direction starts with an ADR,
not with code.

The [`adrs`](https://github.com/joshrotenberg/adrs) tool manages them. `.adrs.toml` at the repository root
points it at the folder and at the template, `docs/architecture/decisions/.adr-template.md`.

- `adrs new "<title>"` creates the next numbered ADR from the template, with status `proposed`.
- `adrs link <new> Amends <old>` or `adrs link <new> Supersedes <old>` records a link in the frontmatter of
  both ADRs. Keep a one-line note under the amended ADR's title as well, saying what changed.
- `adrs list` and `adrs doctor` list and check the folder. `adrs doctor` also runs as a pre-commit hook.
- Add a row for every new ADR to the index in `docs/architecture/README.md`, and change the row when its
  status changes.

An ADR moves from `proposed` to `accepted` or `rejected` only when the maintainer decides it. A rejected ADR
stays in place, with a line saying why and what replaced it.

## Evidence and plans

- Spikes and other research go in `docs/architecture/research/`. An ADR cites the spike that backs it.
- Implementation plans go in `docs/plans/`, one per area. Each task cites the ADRs it implements.
  [`docs/plans/README.md`](docs/plans/README.md) shows which plans cite each ADR.

## Markdown

All Markdown must pass `rumdl` with the settings in `.rumdl.toml`: lines of at most 120 characters, and no code
blocks inside lists. ADRs carry their title in the frontmatter and repeat it as the H1, so MD025 ignores the
frontmatter title.
