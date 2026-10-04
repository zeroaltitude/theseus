# The Ship of Theseus: its chapters

This directory is *The Ship of Theseus*, Theseus's design document, in chapters. It is both the specification and the
record: Part I is the specification, what Theseus is meant to be and why; Part II is the build plan; and Part III, as
built, records what each step actually built, how it was proven, where it diverged from the plan, and what it left
open. When the code and Part I disagree, Part III says so, and one of them gets fixed.

It was one file until v0.79. Since v0.80 it is split along its own headings into chapters of under about 150 KB, so
that an agent can read a chapter whole instead of reading a megabyte by line ranges. No word was dropped or changed in
the split.

**The version** is the first heading of chapter 1 (the line after its title).

## The chapters, in order

| # | Chapter | What it holds |
|---|---|---|
| 1 | [01-front.md](01-front.md) | The front matter: the version, what the three parts are, how the document was put together |
| 2 | [02-part1-s0.md](02-part1-s0.md) | Part I: §0 Thesis, §1 Settled decisions, §2 Principles as constraints |
| 3 | [03-part1-s3.md](03-part1-s3.md) | Part I: §3 Architecture, from §3.1 Bindings to §3.9 Policy, authority, and safety (channels, sessions, the harness and model loops, roles, tasks, providers, Jev's judgments, MCP) |
| 4 | [04-part1-s3.10.md](04-part1-s3.10.md) | Part I: §3.10 to §3.25 (the learning ledger, voice, plugins, budgets, the web UI, executions, external actions and completions, hooks, the wire protocol, configuration and secrets, telemetry, self-extension, lifecycle, toollets, the tool surface, AWS) |
| 5 | [05-part1-s4.md](05-part1-s4.md) | Part I: §4 The context graph, §5 Memory, §6 Storage and tenders, §7 Shells, §8 Verification, §9 Efficiency targets |
| 6 | [06-part1-s10.md](06-part1-s10.md) | Part I: §10 Open questions, and the appendices: A, C, D, E and F (the responses to the external reviews, the proposals, and the openrig and herdr research), then B (what is at stake in the default shell class) |
| 7 | [07-part2.md](07-part2.md) | Part II, the build plan: P0 to P12 |
| 8 | [08-part3-a0.md](08-part3-a0.md) | Part III: A0 to A3, the milestones M0 (with M0.5) to M3 |
| 9 | [09-part3-a3b.md](09-part3-a3b.md) | Part III: A3b, the build chain from the 2026-09-27 decisions (steps 2a to 2b, the reversal, the complexity cuts, narration) |
| 10 | [10-part3-a3c.md](10-part3-a3c.md) | Part III: A3c, M3.5 Fast (steps F1 to F4b, K1, O1, J1, Z1 and B1) |
| 11 | [11-part3-a4.md](11-part3-a4.md) | Part III: A4, M3.6 Daily Driver, and its Items 1 to 20 |
| 12 | [12-part3-item-21.md](12-part3-item-21.md) | Part III: A4's Items 21 to 47 |
| 13 | [13-part3-item-48.md](13-part3-item-48.md) | Part III: A4's Items 48 to 66 |
| 14 | [14-part3-item-67.md](14-part3-item-67.md) | Part III: A4's Items 67 to 78 |
| 15 | [15-part3-item-79.md](15-part3-item-79.md) | Part III: A4's Items 79 to 85, the joins of 2026-10-03's afternoon (C2's bootstrap applied, the third cloud batch, the smalls, Tier 7's store, voice, the repeating wake, Tier 7's defences) |
| 16 | [16-part3-item-86.md](16-part3-item-86.md) | Part III: A4's Items 86 to 96, the joins of 2026-10-03's evening (the cockpit at `/`, the load flakes, Tier 7's kernel, the trusted guild, the sparse note, the ledger's reads, the WAL's synced mark, the benchmark plumbing, 18e, `security.v3`, the LSP client) |

## How the chapters fit together

- **A chapter's first line is its own title, and is not part of the document.** Everything after it is the chapter's
  slice of the document, unchanged. So the chapters, each without its first line, concatenated in this index's order,
  are the whole document, byte for byte. In this directory:

  ```sh
  for f in [0-9][0-9]-*.md; do tail -n +2 "$f"; done > /tmp/the-ship-of-theseus.md
  ```

  The two-digit numbers make the file names sort in the index's order.
- **To change the text, edit the chapter that holds it**, and leave its first line as it is unless what the chapter
  holds changes.
- **A new Item goes at the end of the last chapter**: update that chapter's title and its row here. When a chapter would
  pass about 150 KB, split it at one of its headings (an Item's, or in Parts I and II a section's). The new chapter is a
  new file, numbered so that the names still sort in the index's order, with its own title line and a row here.
- **A new version** changes the first heading of chapter 1, and the chapters whose text changed.
- **The PDF** is rendered from the concatenation (marked, then headless Chrome), so it stays one document. It is sent to
  the operator with each version, and not committed (`docs/*.pdf` is git-ignored).
- `docs/the-ship-of-theseus.md`, the one file until v0.79, now points here. Its path is kept, since other files link
  to it.
