# Threads: the router's rules and the lane's decisions

What the threads lane (theseus-98bz and its follow-ups) decided, in one place, beside the code that holds it. Since
2026-11-21 (the owner, theseus-rha2) a message goes to a thread **by its address only**: the router's guesses were
removed, not switched off. The module docs of `crates/theseus-core/src/thread_route.rs` (the address) and
`rpc/thread_flow.rs` (the flow) are the code's own account; dated history is in `docs/status.md`.

## The router: address, else home

A message in a private place with live threads (`rpc/thread_flow.rs`, `route_to_thread`; `may_route` decides whether
the flow runs at all: not for a thread's own session, a task, a job's message, a prompt or a slash command) meets one
rule, first match wins, certain, before any judge (`thread_route::address`):

1. the **place** a thread alone lists in its `surfaces`;
2. an **explicit** word: `--thread`, a leading `#slug`, `/t slug`, or "in the <title> thread:";
3. a **reply** to a message a thread's turn wrote.

**The home arm.** A message with no address runs in the place's own conversation ("home"), always and at once:
nothing guesses from what it says, waits for a verdict, or asks which thread was meant. There is no pre-router (entity
hits, stickiness, the reply chain as evidence), no learned correction, no fusion with `classify`'s `thread` answer, no
ask, no directory rule, and no misroute guard; the settings that tuned them (`sticky_alone_asks`, `ask_wait_secs`,
`ask_every_thread`, `unsure`, `single_offers_new`, `claim_home_turns`) load, are named once at startup and are ignored
(`ThreadsConfig::retired`). With two or more live threads the inbound point still begins early and the turn takes it
over (`Handoff`), so `classify`'s request is the one it was, its `thread` question still asked; nothing reads that
answer to place a message, and a later pack version drops the question. `thread.move` (`/move`) stays as the owner's
own act: "that was for X" moves the message and runs it again there.

**An address that cannot be honoured is refused, and runs nowhere.** `/t <word>` naming no thread; a `#slug`,
`/t slug` or "in the <title> thread:" naming a thread that is paused or done; an addressed thread whose session does
not open: each ends the submission refused, with one line that says why and how to send it again (Discord posts it
as a warning). No turn starts and nothing is written in the place's session. A thread's state is told only where the
gate proves its seal clear for the place (`view_gate::may_name_thread`); a sealed or seal-pending thread, or one
whose records cannot be read, is refused with exactly the line for a thread that does not exist. In a shared place
only `/t word` is refused, always as "no thread". A `#word` or "in the … thread:" that names no thread at all is no
address and runs at home.

**A reply finds its thread after a restart.** The router's memory of which turn ran in which thread is a cache
(`thread_flow.rs`). The record is the `thread.routed` row each placed turn writes in its first frame: a reply to a
turn the memory does not hold reads that row (`routed_thread_of`), and Discord finds the turn a reply points at from
its own `discord.message.out` rows when its in-memory map has lost it.

The entity readers (`entity_hits`, `new_entities`) are not routing: they name what a message says of a working set,
for Jev's line about a thread, a working set's growth and the generic-folder check.

## The bench's clock

`TurnSubmitParams.route_at_ms` carries the clock the threadbench planned a message at, so stickiness and the chain
decay as they would have between real messages. It is read only under `[threads] bench_clock = true` (a test hook,
false by default), and a submit carrying it to a daemon without the switch is refused, not ignored.

## Deferred tools past the budget

For a model without the provider's tool search, a first request is held to its place kind's budget. A family the
message names, or one `families.v1` judges relevant, may load past it by exactly its own size while the whole stays at
or under `budget × [tools.defer] admit_ceiling` (2.0; `config/defer.rs` says why 2.0). What passed the budget rides
`TurnSubmitResult.raised`: the families, each with its tokens and who admitted it (`named` or `jev`), and a line the
surfaces show after the turn's harness lines. `admit_ceiling = 1.0` and `judge_families = false` are the rollbacks.

The `families.v1` call is made on every routed prompt, a model with tool search included, unless the route pack is
below canary. That is the owner's decision of 2026-10-21: always try to categorize each toolset. It is not narrowed.

## The legacy home claim and its undo

Until 2026-11-21 a turn the router ran at home, whose files all fell in one live thread's working set, was claimed for
that thread (theseus-98bz.6): each node keyed `moved.of.<node>` to the thread, with a claim record (META
`thread.claim.<id>`) and a `thread.claimed` row. **Nothing claims a turn now.** Claims already in a store still read
(the lens and the history read the keys as they read a split's), and `thread.unclaim` (`rpc/home_claim.rs`) is their
undo: it writes each key back to null where it still holds the claim's value, marks the record undone and writes
`thread.unclaimed`.

A split beside the undo (theseus-l4n39): a split writes its keys in a kernel frame under the source session's
execution lock. The undo reads the nodes' keys and writes in one frame under the same lock, so a split lands wholly
before the read, and its value is kept, or after the write, and it takes the nodes, as a split may. No new lock.
`thread.unclaim`'s `kept` counts only keys holding another claim's or a split's value; a key an unsplit nulled is
neither restored nor kept.
