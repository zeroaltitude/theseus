# Threads: the router's rules and the lane's decisions

> **Superseded on 2026-11-22: threads are not containers.** No message is sent to a thread's own session, and
> every thread command is gone (the read-only `theseus thread list` and `thread show` too); a seal, a move or a
> merge no longer holds anything back. Old thread records still decode, and are kept.
> What follows is the record of the threads lane as it was built.

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

**The order, as built** (`rpc/thread_flow.rs`, `addressed`). First, a message that carries an explicit word naming no
live thread is checked: if it names a paused or done thread the reader may be told of, or it is a `/t word` naming
nothing the reader may be told of, it is refused at once and runs nowhere (`thread_route::failed_explicit`). Then
the place and the words (`thread_route::address`: the place a thread alone lists, then the explicit word). Only
then the replied-to turn: a reply to a turn that ran in a live thread goes there; a reply to a turn whose thread is
paused or done is refused when the reader may be told of that thread, and runs at home otherwise.

**What is refused, and what it says.** A refusal ends the submission with one line (`REFUSED`; Discord posts it as a
warning); no turn starts and nothing is written in the place's session. "There is no thread <word>" for a `/t`
naming nothing; "the thread <word> is paused" (with `theseus thread resume`) or "is done" (with `theseus thread
reopen`) for a thread the reader may be told of; and "could not be opened" for an addressed thread whose session
does not open, which names the thread by the word the sender typed, or by its title only when the reader may be
told of it.

**A thread the reader may not be told of is no thread, in every form.** Whether a reader may be told of a thread
(its name, its state, that it exists) is the gate's: only when its seal is proven clear for that reader
(`view_gate::may_name_on`; its own thread is clear to it). A sealed or seal-pending paused or done thread, or one
whose records cannot be read, gives `/t` the "no thread" line, and a `#word`, "in the … thread:" or a reply runs
at home, as for a thread that does not exist. This is about threads that are not live: an **active** thread, sealed
or not, is addressable by name and by reply as before, and its own session keeps its wall. In a shared place no
thread is nameable, so a `/t word` there is refused as "no thread" and nothing else is.

**A reply finds its thread after a restart, within bounds.** The router's memory of which turn ran in which thread
holds the newest 512 turns and is a cache (`thread_flow.rs`). The record is the `thread.routed` row each placed turn
writes in its first frame: a reply to a turn the memory does not hold reads the newest 4,096 such rows
(`routed_thread_of`), and a turn found to have run at home is remembered as such. Discord finds the turn a reply
points at from its own newest 5,000 `discord.message.out` rows when its in-memory map has lost it. A reply older
than those bounds runs at home. **A reply does not follow a turn a split moved**: it goes to the thread the turn
ran in.

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
