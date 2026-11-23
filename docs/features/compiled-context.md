# Context compiled for each turn, not piled up

*One page on the context compiler. Numbers come from the data files and checks it cites. Updated 2026-11-23.*

## What it is

Most agents send the model a transcript that grows until it is cut. Theseus builds each turn's request afresh from
its store, through one shared memory (the view, `docs/design/context-point-of-view.md`), and the
conversation is only one part of it: the standing instructions and personas, the context files you listed, the
guidance you wrote for the conversation's topics, the place's recent exchanges with older ones folded into short lines, the
notes recall found for this message in the order Jev put them, and, for a session attached to a project, the
project's slice at the end. Every request has a manifest that names each node and file that went in, so "why did it
say that?" has an answer you can read.

There is nothing to set and nothing to recompile. The old way (a transcript kept across turns, cut by a ring,
summarised by compaction, kept warm between turns) is gone, and so are the settings that tuned it: a settings file
that still names one loads, says so once at startup, and `theseus health` lists it under "settings keys ignored".
A change to what a session carries (a membership, guidance, a context file) applies at its next turn.

## How it works

- **Four layers, in the order the provider caches them** (`crates/theseus-core/src/compiler/layers.rs`): the
  header (the built-in persona, the tools note, the profile's system prompt, your standing block for the place), the
  personas' context files, the project layer (system context files and the guidance of the session's categories, the
  topics among them), and the messages. The view renders these sections afresh; identical sections can share the
  provider's cache, with no saved prefix library.
- **A budget, and what was left out.** The view fits its pieces to the turn's budget (`view/budget.rs`) and records
  each drop with its reason. A long turn that outgrows the model's window clears its own old tool results and makes
  room (`turn/view_step/room.rs`); one that still does not fit fails, saying so. There is no transcript-compaction fallback. The spec is [§4.4 The context compiler](../spec/05-part1-s4.md#44-the-context-compiler)
  and [§4.5 Prompt caching layout](../spec/05-part1-s4.md#45-prompt-caching-layout).
- **Recall's notes** go in the order Jev's rerank put them, each with its date and why it was recalled.
  Place and category labels do not withhold recall ([It remembers everything](memory-across-sessions.md)).
- **A project's slice.** `theseus project attach` ends every request with the project's goal, its open milestones
  with their dates, its next steps, and the first of its "done when" lines, in at most 300 tokens.
- **The manifest** names every block with its tokens and source, and `context.compiled` is persisted with the turn,
  so a request can be replayed and checked against the digest of what was sent.

## What you see on each surface

- **Terminal:** `theseus why` prints a turn's route, each block with its tokens and where it came from, each
  recalled note with its date and reason, what was left out and why, and which tools were gated.
  `theseus why --short` is the one-screen form.
- **Cockpit:** the Context page, and a turn's view under its session.
- **Discord:** the turn's route is reported; choosing a small model does not select a reduced-context detour.

## The proof

- **Tests:** a request is three layers, a session block and the messages
  (`crates/theseus-core/src/compiler/tests_layers.rs`); recalled sources enter the view beside the message
  (`crates/theseus-core/src/turn/view_step/recall.rs`); a
  settings file naming every retired key loads and names each (`crates/theseus-core/src/config/tests_retired_keys.rs`);
  and `theseus why` prints a turn's route, recalls, drops and gated
  tools (`crates/theseus/tests/why.rs`).
- **Live, on the owner's daemon** (2026-10-19): a session attached to a project quoted the project's goal and first
  "done when" line from its context, while `theseus why` listed the slice (2 lines, 177 tokens), five context files,
  five recalled notes with their reasons, and 30 more left out for the budget.
- **The cache holds across personas:** switching personas changes no byte of the cached prompt
  (`crates/theseus-core/src/tests_persona.rs`).
- **The bytes of the first request** on the stand-in speed bench (2026-10-18): 45.9 KB against Claude Code's 66 to
  79 KB for the same task.

## The limits today

- Every routing pick carries the working context through the same view. Recall and context files are subject to
  that view's budget, not a separate *trivial* or *quick* request builder.
- The project slice shows the first "done when" line and a count of the rest.
- The manifest is checked against the sent digest in tests; a live check of the digest on the owner's daemon is not
  yet published.

## The 1.0.4 plan (historical)

- **1.0.4:** the context-size work rows in the patch bundle, with the memory-spike bench whose harness shipped in
  1.0.3 and whose measurement did not.

Related: [Jev in the core](jev-in-the-core.md) (who orders the notes),
[It remembers everything](memory-across-sessions.md) (where the notes come from),
[Fast, as a contract](fast-as-a-contract.md) (why shared layers matter).
