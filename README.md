# shepherds-crook

crook is being redesigned on top of [herdr-core](https://herdr.dev) (`hc`) and herdr-app: one coordinator
Claude Code session per piece of work, delegating to child sessions that report back.

There is no code on `main` yet. The design lives in [`docs/architecture/`](docs/architecture/README.md) as
MADR decision records.

The v1 Rust CLI, its specs and its tests are on the
[`backup_crook_v1`](https://github.com/s0undt3ch/shepherds-crook/tree/backup_crook_v1) branch.

## Development

Tooling is pinned in `mise.toml`. Install it and the git hooks:

```sh
mise install
prek install
```

## Licence

See [LICENSE](LICENSE).
