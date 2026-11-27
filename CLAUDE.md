# Agent notes

Read README.md for what this repository is.

- Never change an ADR's status yourself. Write new ADRs as `proposed`; the maintainer accepts or rejects them.
- Create and link ADRs with `adrs`, not by copying the template. If mise is not active in the shell, run it as
  `mise exec -- adrs …`.
- Work on `main`: commit and push. Do not open pull requests.
- Test against your own hc core (your own `HC_SESSION` and XDG folders) and folders under `$TMPDIR/crook-e2e-*`.
  Never touch the maintainer's real hc core, herdr-app or crook state without asking first.

@CONTRIBUTING.md
