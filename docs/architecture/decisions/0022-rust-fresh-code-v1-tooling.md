---
number: 22
title: "Rust, fresh code on main, v1's tooling reused"
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
---

# Rust, fresh code on main, v1's tooling reused

## Context and problem statement

[0019](0019-ship-crook-as-a-binary.md) and [0020](0020-mechanical-steps-run-through-crook.md) make crook a
binary again. [0002](0002-build-on-herdr-core-and-herdr-app.md) retired v1's Rust code rather than porting it,
but v1's build, release and test tooling (`backup_crook_v1`) does not depend on that code.

## Considered options

- Rust; fresh code on main; v1's release, CI and e2e tooling reused and adapted.
- Another language for the binary.

## Decision outcome

Chosen option: Rust, fresh code, v1's tooling.

- The `crook` binary is written in Rust, from scratch, on `main`. v1's source is a reference, not a base: a
  piece of it is copied only when it does what an accepted ADR asks.
- Reused from v1 and adapted:
  - `.github/packslip.toml` and the release workflows (`release.yml`, `_build-binary-archive.yml`,
    `_smoke-binary-archive.yml`, `install-smoke.yml`), with the archive laid out as 0019 says: `crook`,
    `plugin/`, `mod/`;
  - `ci.yml` and the shared actions under `.github/actions/`;
  - the `crook-e2e` skill, rewritten for hc and herdr-app.
- Out of scope: children on other machines (`hc run --on`). 0019's note on remote machines stays as the
  only rule until this is taken up.

### Consequences

- Good, because packaging, signing and smoke tests already worked for v1.
- Good, because no v1 design comes back by accident.
- Bad, because the copied workflows must be checked against the new archive layout before the first release.
