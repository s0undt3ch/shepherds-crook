# shepherds-crook

crook runs a piece of work as a *flock*: one coordinator Claude Code session that plans with you and delegates to
child sessions, each with one brief, each reporting back through a result file. It is built on
[herdr-core](https://herdr.dev) (`hc`), which runs the sessions, and herdr-app, which shows them.

The release ships one archive: the `crook` binary, the Claude Code plugin (`plugin/`) with the coordinator and
child skills and hooks, and the herdr-app mod (`mod/`) that lists, creates, opens and closes flocks.

The design lives in [`docs/architecture/`](docs/architecture/README.md) as MADR decision records, and the build
plans in [`docs/plans/`](docs/plans/README.md).

## Install

No release is published yet, so these steps work from the first release on. crook installs with mise, through its
packslip backend:

```sh
mise use -g packslip:github.com/s0undt3ch/shepherds-crook
crook setup
```

`crook setup` installs the herdr-app mod, points crook's `current` link at this version and prints a command to
trust each root folder in Claude Code once. `crook doctor` checks the install.

To open the crook dialog from a key, bind the mod in herdr-app's `keys.json`. Pick a key that no program needs:

```json
{
  "global": { "<your key>": "mod.open:crook" }
}
```

## Development

Tooling is pinned in `mise.toml`. Install it and the git hooks:

```sh
mise install
prek install
```

How to change the design, the ADRs and the docs: [CONTRIBUTING.md](CONTRIBUTING.md).

The v1 Rust CLI, its specs and its tests are on the
[`backup_crook_v1`](https://github.com/s0undt3ch/shepherds-crook/tree/backup_crook_v1) branch.

## Licence

See [LICENSE](LICENSE).
