# Design documents

Theseus's phase designs and the re-cut v1 roadmap, checked in on 2026-09-30 from the design lanes. They are the
record of what was designed, before it was built. [The spec, The Ship of Theseus](../the-ship-of-theseus.md),
stays the source of truth, and its Part III records what was built. Each document was scrubbed while the repository
was public (it has been private since 2026-09-30); the line under its title names the kinds of detail replaced.

- [roadmap-v2.md](roadmap-v2.md): the v1 roadmap re-cut into one spine on `main` and parallel lanes, with a timeline estimate and the conflicts found across the six designs. It orders every step below, from Stage 1's remainder (steps 4b to 8) to step 45.
- [roadmap-v1.1.md](roadmap-v1.1.md): the week after v1. The `post-v1` backlog, Review 2's deferred items, the designs' open questions, and Part III's open gaps, cut into seven themes, 12 spine steps, and 12 lanes, with what each theme proves, and what waits past v1.1 and why.
- [stage2-operator-surfaces.md](stage2-operator-surfaces.md): Stage 2, the operator's surfaces: the protocol push, the TUI, herdr, the first edges, telemetry, and caching. Roadmap steps 9 to 13.
- [aws-toolset.md](aws-toolset.md): the AWS toolset for an owner-Theseus: the dynamic client, the guardrails, credentials, stacks, durability, and hands. Roadmap steps 14 to 16 and 40, and AWS credentials in L1.
- [m4-boundaries.md](m4-boundaries.md): M4, boundaries apart from AWS: the L1 sandbox, verified cancellation, egress and credential brokering, labels, the ontology's first slice, and control-plane separation. Roadmap steps 17 to 22.
- [m5-judgment.md](m5-judgment.md): M5, judgment: the Jev client, its packs in shadow, the learning ledger, and the ladder from shadow to live. Roadmap steps 23 to 28.
- [m6-memory.md](m6-memory.md): M6, memory as an experiment: the memory exam, the index tender, recall, and the ablation harness. Roadmap steps 29 to 35.
- [m7-surface.md](m7-surface.md): M7, the surface: the MCP client and server, recurring wakes, bindings, the task graph, the web UI, self-extension, and voice. Roadmap steps 36 to 39 and 41 to 45.
- [thinking-binding.md](thinking-binding.md): why a turn on the newest model failed at its second step (issue 20): a thinking block is bound to everything before it, the tail that did not stay, the one repeat with the provider's remedy, and how to read the mismatch counts (`theseus ledger -k provider.call`).
- [threads.md](threads.md): the threads lane's rules and decisions: the router's order, the directory rule, the ask's `new` offer, the bench's clock, deferred tools past the budget and the `raised` field, the families call, and the home claim beside a split.
- [self-gates-inventory.md](self-gates-inventory.md): every gate, wait and stop on Theseus's own work, one row each, and which can become notify-then-work without losing a money backstop (theseus-nmod9). States as of install #91; the money and AWS lines and the keel list are all marked keep.
- [it-remembers-and-knows-you.md](it-remembers-and-knows-you.md): what 1.0.7 changed in memory, recall and the ontology's own writes (issue 9): the open-work tool, fusion weighted by coverage, the notice's plain words, a kind named for its class, the one-time cleanup, a retired kind shown nowhere.
- [ontology-members-on-demand.md](ontology-members-on-demand.md): what the daemon holds of the people-and-topics board (the categories and their counts) and what it reads from the store when asked (a session's membership lists), and what each reader of it now costs (issue 21).
- [onto-sweep-pace.md](onto-sweep-pace.md): why the onto sweep's wait between questions hung a chain test on a loaded host, the two loaded arms and the asymmetry between them, the gdb stack that named `pressure::quiet` rather than a deadlock, and the four sibling sites with the same latent shape, now on the same wait (theseus-75t4q, theseus-qc5r6).
- [owner-proof.md](owner-proof.md): the owner's second proof (issue 22): a code from their authenticator app for the changes that matter most; where the secret lives, how a code is checked and limited, the window a good code opens, how each surface carries it, and why neither the secret nor a code ever passes through a turn.
- [imported-surface.md](imported-surface.md): what reaches an imported session's content (theseus-a7pr): the protected thing, where the surface check lives, what fail-closed means for a method nobody classified, and what a fail-closed default refuses.
- [abandon-said-in-words.md](abandon-said-in-words.md): what would have to be true for the owner's words to answer an outstanding abandon card (theseus-3i6d1): where the owner's recent messages are already held and for how long, the rule for whose words they are, the two rules that disagree about a bound private channel, and the four questions that are still open.
- [stale-edit.md](stale-edit.md): the stale edit (`theseus-w2cs` sub-item 2): a read record is noted when it exists and checked when it exists, never required; the required-prior-read shape that was built, measured and rejected, and why `rsi` self-sessions made it unacceptable; where the record lives, its cap, and why a `DefaultHasher` value may not cross a process.
- [undo-v1.md](undo-v1.md): where `undo.v1`'s verdict is consumed (row B6, `theseus-2yowd`): why the layer cannot go before `Layer::Mode`, the ask-once-consume-twice shape, and why `irreversible_final` is applied to what the gate's order returned rather than raised inside it.
- [agent-notes-rule.md](agent-notes-rule.md): the durable-versus-run-state rule for an agent's own notes (`b4-theseus-holds-its-own-notes`, row B4): why there is a rule, the three questions the code asks and in what order, the two numbers and why they are judgement rather than measurement, the five shapes the rule gets wrong, and what the two default-off switches hold back until the owner has seen it.
- [recall-warm.md](recall-warm.md): how recall meets an unloaded embedding model (issue 21 of 1.0.7): the message's arrival loads it, recall's wait covers a load up to `LOAD_BOUND`, who never waits, and what it does to time to first word.
- [self-loop-failure-handling.md](self-loop-failure-handling.md): what stops the self-improvement loop's failures now (issue 6 of 1.0.7): the cap on tries, the rules the builder is told, formatting before the gate, no start on a red main, a tree that stays under its gate, a stale item settled, set-aside beside drop, a join that leaves no tree, and the install's reap; and what it does not do.
- [self-dependencies.md](self-dependencies.md): what a self build may download and what stops at the owner's card (issue 20): the builder reaches the cargo and npm registries and keeps its own package cache; the pipeline fetches exactly what a branch's changed lock files name, checksum-checked, for a gate that keeps no network; a dependency from anywhere else is refused; a change of dependencies is held for the owner with each package named.
- [self-builder-build-folder.md](self-builder-build-folder.md): the one directory a self builder's commands keep while its item is open (issue 20), so it can build and run the tests of what it writes: whose it is, what the view holds, why the gate never reads it, its bound and its removal; and what it does not do.
- [import-rest.md](import-rest.md): why the daemon rested about 100 MiB higher after the nightly import (issue 21): the heat cache filled by the import's one-time reads, the heap profile that showed it, the read that keeps nothing, and the guard in `bench/memory/` that fails when an import leaves the daemon resting higher.
- [memory-peak-record.md](memory-peak-record.md): the daemon's memory peak, named (issue 21, step one): the window of activity every entry point feeds, the `resident.peak` row a trim over 512 MiB writes, how to read it, and what it found (the AWS cross-check's 100,000-row ledger tail, an hour after each start).
- [index-vector-files.md](index-vector-files.md): the index's vector table in two mapped files (issue 21 of 1.0.7, step one): the scan file as a projection of the vector file and the nameless rows file, what stays in memory and why, what the files are never allowed to do under a running search, and where a hot set and cold groups have room.
- [index-hot-cold.md](index-hot-cold.md): the index's hot set and cold groups (issue 21 of 1.0.7, step two): the budget (`[index] resident_mb`), what makes a record hot, the tier file as a derived projection of the scan file, what a search reads and why a forgotten record cannot come back, how a record moves, the self-check and the fallback to the exact scan, and the replay tool.
- [self-own-removals.md](self-own-removals.md): why the reversibility layer's holds are lowered for a self session's own removals (issue 20): the three kinds of call the code can read, what persists from an L1 call, what "its own" is, why the self builder no longer carries the operator's network list, and what a self session still cannot remove without a hold.

And one review:

- [review-2.md](review-2.md): Review 2 (2026-09-30), a read-only review of the code as built through the Daily
  Driver, in three layers: complexity, speed (the lifecycle budgets and the runtime's threads), and hardening (the
  web UI, the spool, the git tools, the secret broker). Its accepted findings became the roadmap's fix batches and
  lanes. The spec's Part III records each one's fix. Checked in on 2026-10-01.

_Index written by Tabitha/Claude, 2026-09-30; Review 2 added 2026-10-01, and the v1.1 roadmap 2026-10-02._

## Crates merged ahead of their reader

Each says so in its manifest (`reserved_for` under `[package.metadata.theseus]`, Item 32). Part III Items 16, 18, and 20 record them.

| Crate | What it is | Wired in at |
|---|---|---|
| `theseus-ontology` | The fungible ontology's first slice (§4.1a) | row 26 (21b): **wired in 2026-10-04**, its `reserved_for` removed (Part III Item 100) |
| `theseus-judge` | Jev: the typed client, bands, batching, the breaker, the question packs | row 37 (23a): **wired in 2026-10-04**, its `reserved_for` removed (Part III Item 105) |
| `theseus-memory` | FSRS-6 and spreading activation, pure | row 52 (30a): **wired in 2026-10-04**, its `reserved_for` removed (Part III Item 99) |
| `theseus-exam` | The memory exam. _Since 2026-10-04 (34b's wire-in; Part III Item 124) a tool run by hand beside `theseusd`, never in a shipped binary: its `reserved_for` became `tool = …`, and it drives one scratch daemon per arm (`theseus-exam run`, `report`, `replay`)_ | row 55 |
| `theseus-mcp` | MCP, client and server, written by hand | row 66 (36b) and row 72 (41b): **both sides read since 2026-10-04**, its `reserved_for` removed (Part III Items 106 and 110) |
| `theseus-lsp` | The LSP client, written by hand: framing, push and pull diagnostics, navigation, the stop, a scripted fake server (Part III Item 96); since 2026-10-04 it never offers to watch files for a server, so each server watches its own and sees what a job wrote (card 6, Item 151), and a saved document waits for its server's check after the save, so rust-analyzer's pushed rustc errors join its pulled analysis (theseus-c6hv, Item 153) | L2 (theseus-n88g.8): **read since 2026-10-04** by theseus-core's board and tools, its `reserved_for` removed (Part III Item 114) |
