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
| 17 | [17-part3-item-97.md](17-part3-item-97.md) | Part III: A4's Items 97 to 107, the joins of 2026-10-04's first hours (C3's curated tools, tasks that set wakes, recall in shadow, the ontology wired in, the judge's prove, the user unit's restart limits, the gate's speed and flakes, Jev wired in, MCP tools, the hands) |
| 18 | [18-part3-item-108.md](18-part3-item-108.md) | Part III: A4's Items 108 to 117, the joins of 2026-10-04's night (the durability tender, terminals, the MCP server, the Ontology view, recall in front of the model, the task arrangement, the LSP tools, MCP prompts, the hands' cancels and reservations, bindings' second format) |
| 19 | [19-part3-item-118.md](19-part3-item-118.md) | Part III: A4's Items 118 to 128, the joins of 2026-10-04's early morning (`extend.propose`, the judgment surfaces, the security check in shadow, class and role, continuation, restore from S3, the exam's memory arms, the task record, topics, an edit's diagnostics, the rerank arm) |
| 20 | [20-part3-item-129.md](20-part3-item-129.md) | Part III: A4's Items 129 to 138, the joins of 2026-10-04's morning and afternoon (the learning ledger, budgets and policy, compaction, independent checks, extensions that load, the hands' network, the cockpit without WebGL, the memory pass, the logo, one-command setup) |
| 21 | [21-part3-item-139.md](21-part3-item-139.md) | Part III: A4's Items 139 to 148, the joins of 2026-10-04's late afternoon and evening (routing live, gliding on the place rule, the rerank live, the security notices, the sea's swell, replay, the ladder, the smalls, B5's run) |
| 22 | [22-part3-item-149.md](22-part3-item-149.md) | Part III: A4's Items 149 to 158, the joins of 2026-10-04's late evening to 2026-10-05's first hours (a job's start without a fork, the route and bench fixes, Linux's IO, the job cgroup, rust-analyzer's checks, the refusal fallback, files, the assembled context, retention, speed) |
| 23 | [23-part3-item-159.md](23-part3-item-159.md) | Part III: A4's Items 159 to 170, the joins of 2026-10-05's night (activation, consolidation, the detour's recall, tiering, the task board, the learning loop, the cockpit's tabs, a restart's results, three bench harnesses, the kernel simulator's second pass) |
| 24 | [24-part3-item-171.md](24-part3-item-171.md) | Part III: A4's Items 171 to 180, the joins of 2026-10-05's morning (the seventh cloud batch's fixes: durability, notifications serialized once, the kernel, AWS, `proc.run`'s steps, the WAL's failed sync, health's words, telemetry, the prove wired in, resumed calls in telemetry) |
| 25 | [25-part3-item-181.md](25-part3-item-181.md) | Part III: A4's Items 181 to 193, the joins of 2026-10-05's afternoon and evening (a routed session's base, the reader rule's holes, situations, the bench harnesses' fixes, the memory checks, consolidation's headed entries, the timing flakes, telemetry's third pass, the CLI's tests, the turn's stack, history's pages) |
| 26 | [26-part3-item-194.md](26-part3-item-194.md) | Part III: A4's Items 194 to 203, the joins of 2026-10-06's first hours and morning (each queue's row in its frame, the smalls, the WAL's sync skip, a crash's held money, durability on, the bench's hygiene and recall plans, the importer, voice turns, the learning fixes) |
| 27 | [27-part3-item-204.md](27-part3-item-204.md) | Part III: A4's Items 204 to 215, the joins of 2026-10-06's late morning and midday (escaped secrets withheld, the gate's tests, a declined batch, Discord's live bindings, the judge's tests and reads, memory and telemetry tests, the core's waits, routing's tests, the daemon's proofs, voice's echoes) |

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
