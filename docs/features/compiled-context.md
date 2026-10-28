# Context compiled for each turn, not piled up

*One page on the context compiler. Numbers come from the data files and checks it cites. Updated 2026-10-28.*

## What it is

Most agents send the model a transcript that grows until it is cut. Theseus sends a request compiled from its store
for this turn, and the conversation is only one part of it: the standing instructions and personas, the context files
you listed, the guidance you wrote for the thread's topics, the conversation so far with a summary in place of what
no longer fits, the notes recall found for this message in the order Jev put them, and, for a session attached to a
project, the project's slice at the end. Every request has a manifest that names each node, file and summary that
went in, so "why did it say that?" has an answer you can read.

## How it works

- **Four layers, in the order the provider caches them** (`crates/theseus-core/src/compiler/layers.rs`): the
  header (the built-in persona, the tools note, the profile's system prompt, your standing block for the place), the
  personas' context files, the project layer (system context files and the guidance of the session's categories, a
  thread's topics among them), and the messages. Sessions and threads share their first layers byte for byte, so the
  cache is warm for all of them.
- **A budget, and what was left out.** The compiler fits the layers to the place's budget
  (`compiler/budget.rs`), compacts the conversation into a summary when it no longer fits (`compiler/compaction.rs`),
  and records each drop with its reason. The spec is [§4.4 The context compiler](../spec/05-part1-s4.md#44-the-context-compiler)
  and [§4.5 Prompt caching layout](../spec/05-part1-s4.md#45-prompt-caching-layout).
- **Recall's notes** go in the order Jev's rerank put them, each with its date and why it was recalled; a thread
  keeps other threads' notes out unless they are about the same subject ([It remembers everything](memory-across-sessions.md)).
- **A project's slice.** `theseus project attach` ends every request with the project's goal, its open milestones
  with their dates, its next steps, and the first of its "done when" lines, in at most 300 tokens, and the slice
  survives compaction.
- **The manifest** names every block with its tokens and source, and `context.compiled` is persisted with the turn,
  so a request can be replayed and checked against the digest of what was sent.

## What you see on each surface

- **Terminal:** `theseus why` prints a turn's route, each block with its tokens and where it came from, each
  recalled note with its date and reason, what was left out and why, and which tools were gated.
  `theseus why --short` is the one-screen form.
- **Cockpit:** the Context page, and a turn's view under its session.
- **Discord:** a turn that ran on a detour says so in its footer.

## The proof

- **Tests:** a request is three layers, a session block and the messages
  (`crates/theseus-core/src/compiler/tests_layers.rs`); threads and sessions share their first layers byte for
  byte, and a thread's topics make its third layer (`crates/theseus-core/src/rpc/tests_thread_context.rs`); an
  attached conversation keeps its project's slice through three compactions
  (`crates/theseus-core/src/tests_projects.rs`); and `theseus why` prints a turn's route, recalls, drops and gated
  tools (`crates/theseus/tests/why.rs`).
- **Live, on the owner's daemon** (2026-10-19): a session attached to a project quoted the project's goal and first
  "done when" line from its context, while `theseus why` listed the slice (2 lines, 177 tokens), five context files,
  five recalled notes with their reasons, and 30 more left out for the budget.
- **The cache holds across personas:** switching personas changes no byte of the cached prompt
  (`crates/theseus-core/src/tests_persona.rs`).
- **The bytes of the first request** on the stand-in speed bench (2026-10-18): 45.9 KB against Claude Code's 66 to
  79 KB for the same task.

## The limits today

- A turn Jev detours to a small model as *trivial* carries no recall and no context files; a *quick* detour carries
  its recall and the project's slice since v1 patch1.
- The project slice shows the first "done when" line and a count of the rest.
- The manifest is checked against the sent digest in tests; a live check of the digest on the owner's daemon is not
  yet published.

## What is coming

- **1.0.4:** the context-size work rows in the patch bundle, with the memory-spike bench whose harness shipped in
  1.0.3 and whose measurement did not.

Related: [Jev in the core](jev-in-the-core.md) (who orders the notes),
[It remembers everything](memory-across-sessions.md) (where the notes come from),
[Fast, as a contract](fast-as-a-contract.md) (why shared layers matter).
