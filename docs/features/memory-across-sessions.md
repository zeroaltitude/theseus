# It remembers everything, across sessions and tools

*One page on memory. Numbers come from the data files beside each report they cite. Updated 2026-10-28.*

## What it is

Something you said last week, in another channel, about another project, comes back when it matters, with a link to
the message it came from. Nothing is copied into a notes file: one store holds every message with its time, place and
position in the log, and recall references sources instead of copying them. So a reply can cite the message it
remembers, a wrong fact can be traced everywhere it spread, and your history from another agent can come along on
day one.

## How it works

- **One graph.** Every message, tool call, result, task, summary and note is a node in the store
  ([§4 The context graph](../spec/05-part1-s4.md#4-the-context-graph)), and recall is a query over it, not over a
  separate memory file.
- **Recall's pipeline** (`crates/theseus-memory/src/recall.rs`, [§5.3](../spec/05-part1-s4.md#53-recall)): the
  index finds candidates; filters drop each with its reason (another place the session may not read, a node you
  labelled wrong or stale, the older side of a supersedes edge); the science ranks them; Jev's rerank orders them;
  and the pack fits the budget. In a thread, other threads' notes stay out unless they are about the same subject.
- **The memory pass** (`crates/theseus-memory/src/activation.rs`, `consolidate.rs`): after a turn, notes are
  written with their kind, durability and what they are about, with retention modelled on spaced repetition
  (`fsrs.rs`), and a nightly consolidation writes syntheses whose citations are checked before they may be recalled.
- **Imports** (`crates/theseus-core/src/import/`): `theseus import claude-code`, `codex` or `openclaw` reads another
  agent's history into the same graph as episodes with their times and sources, and `theseus persona import` brings
  a whole workspace over as a persona whose own past ranks a little higher while it speaks.
- **Place-safe.** A note made in a private place is never recalled into a shared one.

## What you see on each surface

- **Everywhere:** a reply that draws on memory cites the message, with its date and place.
- **Terminal:** `theseus why` lists each recalled note with why it was recalled and what was left out;
  `theseus import` and `import list`; `theseus card observed` shows what live learning kept from your own lines.
- **Discord:** up to three question cards a day in your DM (Add, Pattern, Stale) from the nightly pass, answered
  Keep, Edit, Not this or End it. Nothing becomes a record without your answer.

## The proof

- **Head to head** ([the report](../benchmarks/2026-10-18-recall-full-run.md), 2026-10-18): over 600 scripted
  turns in three sessions and 204 probes an arm, Theseus answered 73.1% of recall probes, Pi 56.9%, Claude Code 50.7%.
- **How much is the seed** ([the report](../benchmarks/2026-10-26-recall-seeds-1.0.3.md), 2026-10-26, the frozen
  1.0.3 build): two fresh seeds scored 68.8% and 77.8% on 144 recall probes each; across sessions 34 of 48 and 44 of
  48; confidently wrong 10.3% and 5.4%. So about five points either side of any one number is the seed, and the
  gap to the other harnesses is larger than that. Like for like on one price table the run cost about twice the
  2026-10-18 run, and about half of that rise is the bench's larger context window, not the build.
- **Imports live:** the owner's earlier assistant history, 21,779 episodes, imported in 2 minutes 4 seconds with
  none rejected (the [memory design](../design/m6-memory.md) and its report).
- **Tests:** `crates/theseus-memory/src/tests_threads.rs`, `tests_persona.rs`, `tests_category.rs`, and the
  core's recall tests.

## The limits today

- Dates are the weak kind: 36.7% right on one 2026-10-26 seed, against 91.3% for paths and 81.8% for hosts.
- Asked something it was never told, it says so and then says what it *does* hold about a neighbouring
  component. The "it gave a value 51 of 60 times" that stood here was **withdrawn by its own report**
  ([2026-10-26](../benchmarks/2026-10-26-recall-seeds-1.0.3.md), correction 2): the grader read the check
  over the whole reply, so the neighbour named in the explanation counted as an answer. Read over the
  answer itself it abstained 60 of 60 on both 2026-10-26 seeds. The scorer now records both reads
  (`abstention_accuracy` and `abstention_in_answer`), and what the explanation after an abstention should
  carry is still open.
- Recall is on by default but its vector half has a 250 ms deadline it does not always meet on a large store.
- The head-to-head is one run per harness; the seed study is Theseus alone.

## What is coming

- **After 1.0.4:** the recall bench runs again on the 1.0.4 build, with Claude Code and Pi rerun only if a model
  or a harness changed.
- A thread's recall across threads (`recall_cross.v1`) is live and not yet measured on the real model.

Related: [Context compiled for each turn](compiled-context.md) (how recalled notes reach the model),
[Many characters, one memory](personas-and-threads.md) (one memory under every persona).
