# The exchange ledger, and the owner's mark

Step 1 of the context refactor (issue 44): records written beside the conversation so that later steps have
something true to read. It changes no prompt and calls no model; a test holds a private request byte for byte
with the ledger off. The code is `crates/theseus-core/src/exchange.rs` and `turn/exchange_step.rs`.

## What is written

| Record | Key | Written | Holds |
|---|---|---|---|
| The owner's mark | `said.<node>` | with the message's node, in the turn's next frame | whether `places::owner_in_person_of` held for it; on a chat transport, whether its author holds an owner handle; the process behind it (`Peer`: a person, an agent by name, a job, no terminal, untraced, a transport); the surface; the place it arrived in and that place's session, taken before thread routing; the session it ran in |
| An exchange | `exchange.<turn>` | its opening half once the input is written, its end in the turn's last frame (the same key again) | what opened it, the place, private or not, the person, its start and end, the ask and the final reply and every message of the person's in it, by node id |
| A touch | `touch.<root>␁<u64::MAX - ms>␁<turn>` | with the exchange | that exchange touched the root (`place:`, `person:`, `thing:<type>:<value>`) |
| The measuring row | ledger `context.turn` | the turn's last frame, private places only | ids, kinds and counts (below) |
| The places bound | ledger `places.bound` | each binding start, riding the next frame | how many DMs, private channels and shared channels are bound |

Nothing here is a field on an existing record, so no store-format step is needed. No `derived_from` edge is
written: the keys carry the node and turn ids, and `retract` walks edges to node ids only.

**A place** is the binding's target (`discord:dm:<id>`, `discord:channel:<id>`), or `local:<dir>` for the terminal
and the web UI, by the directory the session works in. **A person** is `owner` when the mark proves it (in person,
or an owner handle on a transport), a chat author by id otherwise, and none for a process that is no person.

**The newest `n` of a root** is one read: the time is inverted in the key, so they are the first `n` keys under
the root's prefix (`Store::first_with_prefix`, which stops its walk at `n`).

**Things** are the index's own extractor's terms (paths, files, Beads ids, commits, hosts, crates, mentions), asked
of the tender off the turn's path: the message's when it is heard, collected at the turn's end if they are in by
then; the final reply's after the turn, touched by the place's next exchange. Without an index, none.

## The measuring row

Per private turn: whether the place's previous exchange was in the context sent (in the transcript, carried by
issue 43's place tail, or missed) and whether it ran in the same session; the person's newest exchange in each
other private place and whether it was in view; the first call's context by kind in tokens (est. at 3.6 bytes,
after clearing: header, tools, project files, replies, tool calls, the person's words, tool results whole and
cleared, recall notes, the rest); the compile's situation and trigger and the build's own cold check, with the
turn's cache reads and writes; the things in the message and in the reply before it, by kind; the classify pack's
`fragment`, `mentions_other_conversation`, `wants_fresh_look`, `addressed_task` and `kind` as recorded for the
message by then; the gap since the place's previous exchange; whether another session of the place was mid-turn;
whether this message answered, within ten minutes, a turn no person opened; how the peer was classed.
`theseus exchanges report` sums the rows.

## The rebuild

`theseus exchanges rebuild [--dry-run]` writes the exchanges and touches of turns that have none, from their
nodes, and keeps every exchange written as it happened. A place is filled only from the binding's record of a
place's session, a `/new`'s link to the place it moved, or a thread's shown place; a person only in a direct
conversation with an owner. Everything else says `unknown`. No mark is ever made backwards.

## Settings and bounds

`[context] ledger` (default on) writes none of it when false. In memory: the arrivals staged between
`turn.submit` and the inbound point (at most 64, each for ten minutes) and one pending reply per place (at most
256), both lost at a restart, which loses only what the rebuild restores.

The outbox's load used to read every META record at a start; it reads its own three prefixes now, so these
families, which grow with history, add nothing to a start.
