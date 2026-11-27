---
status: Proposed
date: 2026-11-27
decision-makers: maintainer
---

# File formats for the roster, briefs, results and delivery marks

## Context and problem statement

The flock's files are shared by `crook`, its hooks, the coordinator and the children
([0008](0008-messages-over-hc-send-results-folder-inbox.md), [0009](0009-child-stop-hook-reports-completion.md),
[0017](0017-resume-children-from-a-roster.md)). Each file needs one agreed shape, chosen by who writes it and
who reads it.

## Decision drivers

- Files only crook writes are for programs: easy to parse, safe to rewrite.
- Files a model writes and another model reads are for people too: Markdown.
- crook parses as little model-written text as it can.

## Considered options

- JSON for crook's own files; Markdown for model-written files, with a small frontmatter where crook must read
  a value.
- Markdown everywhere, parsed by crook.
- TOML for crook's own files.

## Decision outcome

Chosen option: JSON for crook's files, Markdown for the models' files. All of them live in the flock's default
location ([0012](0012-split-child-traffic-from-curated-notes.md)).

| File | Written by | Read by | Format |
| --- | --- | --- | --- |
| `roster.json` | crook only (`spawn`, `stop`, `close`, `resume`) | crook, the mod through `crook status --json` | JSON, below |
| `briefs/<child>.md` | The coordinator | The child | Markdown, fixed headings |
| `results/<child>.md` | The child | The coordinator, `crook status` (frontmatter only) | Markdown with YAML frontmatter |
| `results/.delivered.json` | `crook hook` | `crook hook` | JSON map from child name to the SHA-256 of the result last delivered |

crook writes its JSON files to a temporary file and renames it into place, so a reader never sees half a file.

`roster.json`:

```json
{
  "version": 1,
  "flock": "<slug>",
  "coordinator": { "session_id": "<uuid>", "folder": "<path>", "state": "active" },
  "children": [
    {
      "name": "<child>",
      "role": "child",
      "model": "sonnet",
      "session_id": "<uuid>",
      "folder": "<path>",
      "repo": "<path or null>",
      "branch": "<branch or null>",
      "brief": "briefs/<child>.md",
      "state": "active"
    }
  ]
}
```

`state` is `active` or `done`. `version` lets a later crook migrate the file.

A brief has these headings, in this order: `Goal`, `Why`, `Scope` (in and out), `References`, `Done when`.
The coordinator skill carries the template.

A result:

```markdown
---
status: working
---

## Progress

- [x] Step done
- [ ] Step to do

## Summary

## Links
```

`status` is `working`, `done` or `failed`. The checklist is what a resumed child reads (0017).

### Consequences

- Good, because crook never parses model prose: it reads one frontmatter field and hashes the rest.
- Good, because the mod reads JSON with no extra parser.
- Good, because hooks write delivery marks without touching the roster.
- Bad, because a child that breaks its frontmatter shows no status; `crook status` reports such a result as
  unreadable rather than guessing.

## Pros and cons of the options

### Markdown everywhere

- Bad, because crook would parse model-written text to find session ids and states.

### TOML for crook's files

- Good, because it is easier to read by hand.
- Bad, because the mod's JavaScript has no TOML parser, and nobody edits these files by hand.
