---
status: Accepted
date: 2026-11-26
decision-makers: maintainer
---

# Write plain GitHub Markdown for herdr-md

## Context and problem statement

Effort documents are read in a Markdown reader inside herdr-app (the herdr-md mod), not in a particular
notes app.

## Decision outcome

Documents are GitHub-flavoured Markdown limited to what herdr-md renders:

- tables, task lists, strikethrough;
- GitHub alerts (`[!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`);
- footnotes;
- `$$` display maths;
- Mermaid fenced blocks.

Links between documents are relative Markdown links. No wikilinks and no notes-app-specific syntax.

The coordinator can open a document beside itself with `mdh open FILE`.

### Confirmation

Feature list read from the herdr-md mod source: `lex.js` (footnotes, maths, GFM tables), `main.js` and
`render/render.mjs` (Mermaid through mermaid-cli, maths through MathJax), `mdh` usage text.

### Consequences

- Good, because documents render the same in herdr-md and on GitHub.
- Bad, because Mermaid rendering needs Chrome installed (mermaid-cli drives it).
