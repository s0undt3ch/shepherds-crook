---
number: 26
title: "crook's settings live in an XDG config file"
status: accepted
date: 2026-11-27
decision-makers:
- maintainer
---

# crook's settings live in an XDG config file

## Context and problem statement

crook has settings: `notes_root` ([0004](0004-flock-storage-location.md)) today, more later. Every subcommand
reads them, so they need one fixed place. On macOS, Rust's usual config helpers point at
`~/Library/Application Support`, while crook already keeps its data under `${XDG_DATA_HOME:-~/.local/share}`.

## Considered options

- `${XDG_CONFIG_HOME:-~/.config}/crook/config.toml` on every platform.
- The platform's native config folder.

## Decision outcome

Chosen option: `${XDG_CONFIG_HOME:-~/.config}/crook/config.toml`, on macOS too.

- TOML, because the maintainer edits it by hand.
- A missing file means every default; crook works with no config ([0004](0004-flock-storage-location.md)).
- An unknown key is an error that names the key, so a typo never passes silently.
- `crook doctor` reports the path it read and any error in it.

### Consequences

- Good, because config and data sit under the same XDG roots on every platform.
- Good, because the file is easy to keep in a dotfiles repository.
- Bad, because crook resolves XDG paths itself instead of using the platform default.

## Pros and cons of the options

### The platform's native config folder

- Bad, because on macOS it splits crook's config from its XDG data folder, and dotfiles tooling expects
  `~/.config`.
