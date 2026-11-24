# The exchange ledger, and the owner's mark

The exchange ledger began as step 1 of the context refactor (issue 44), recording beside an unchanged
transcript prompt. That off-mode experiment is historical: every turn now uses the view and its required
exchange ledger. The code is `crates/theseus-core/src/exchange.rs` and `turn/exchange_step.rs`.

## What is written

| Record | Key | Written | Holds |
|---|---|---|---|
| The owner's mark | `said.<node>` | with the message's node, in the turn's next frame | whether `places::owner_in_person_of` held for it; on a chat transport, whether its author holds an owner handle; the process behind it (`Peer`: a person, an agent by name, a job, no terminal, untraced, a transport); the surface; the place it arrived in and that place's session; the session it ran in |
| An exchange | `exchange.<turn>` | its opening half once the input is written, its end in the turn's last frame (the same key again) | what opened it, the place, private or not, the person, its start and end, the ask and the final reply and every message of the person's in it, by node id |
| A touch | `touch.<root>␁<u64::MAX - ms>␁<turn>` | with the exchange | that exchange touched the root (`place:`, `person:`, `thing:<type>:<value>`) |
| The measuring row | ledger `context.turn` | the turn's last frame, private places only | ids, kinds and counts (below) |

Nothing here is a field on an existing record, so no store-format step is needed. No `derived_from` edge is
written: the keys carry the node and turn ids, and `retract` walks edges to node ids only.

**A place** is the binding's target (`discord:dm:<id>`, `discord:channel:<id>`), or `local:<dir>` for the terminal
and the web UI, by the directory the session works in. **A person** is `owner` when the mark proves it (in person,
or an owner handle on a transport), a chat author by id otherwise, and none for a process that is no person.

**The newest `n` of a root** is one read: the time is inverted in the key, so they are the first `n` keys under
the root's prefix (`Store::first_with_prefix`, which stops its walk at `n`). `newest_before(root, ms, n)` pages
back: the walk starts at the key that `ms` sorts to (`Store::first_with_prefix_from`).

**Things** are the index's own extractor's terms (paths, files, Beads ids, commits, hosts, crates, mentions), asked
of the tender off the turn's path: the message's when it is heard, collected at the turn's end if they are in by
then. Each naming writes its own touches when the index answers, riding any writer's next frame, keyed at the
exchange's start, so an ask and a reply that name one thing make one touch. Without an index, none.

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
When the previous exchange was missed, the row says why where it can (`prev_why`): `new`, the place's `/new` lay
between the two (the view leaves out what came before it, on purpose), or `not_measured`, the first call was never
measured. A key added in its free data, written only when set; an older row has none and still reads.

`theseus ledger exchanges [--since <age|date>]` answers three questions first, counts and shares only, then sums
every group of the rows, with the places bound now by kind (DMs, channels bound private, those of them people
besides the owner can view, shared channels), read from the place rule as health is: a row at the binding's start
would be a frame of its own there.

- **Was the place's previous exchange in what the model was sent?** Over the turns his own message opened (the
  exchange's person is the owner: never a wake, a report, a guest or an agent's process): shown, held after a
  `/new`, first here, and missed, as a share of those whose first call was measured; the rest are counted as not
  measured, never as a miss. A miss the row cannot explain is called a miss (held by a seal or a gate, or a real
  miss): the report reads no seal and no gate. A row from before `prev_why` is checked against the place's latest
  `/new` only, since only the latest is kept, so an older `/new` leaves its turns counted as missed.
- **How often does he name something Theseus can recognise?** Over the same turns: those whose message named at
  least one of the index's things, of those the index answered for in time, and for each kind the turns that
  named it.
- **What is the context made of?** Over the turns whose first call was measured: the mean of each part in tokens
  (est.) and its share, the compiler's own estimate, and the cache read and written a turn.

`--since` counts only rows whose own time is at or after it, wherever they lie in the store: rows from before
the view became the only path measured the transcript. The report never holds a message's text, a name, a place,
an exchange or a task: the judge's `addressed_task` is counted as `a_task` or `none`, a message's length by its
band, and a field of the row the counter does not name is counted as present, never by its value. A stored word
is counted by name only when it is one its producer makes (the index's kinds of thing, a situation's kind, a
compile's trigger, the classify pack's choices); any other is counted as `unlisted`. A daemon from before the
answers is told to update, and none of its groups are printed.

The totals count rows, not every message he ever sent. Turns in shared places, older history that was never
marked, and turns that ended without writing a row are outside them.

## The rebuild

`theseus ledger rebuild-exchanges [--dry-run]` writes the exchanges of turns that have none, from their nodes (in
conversations that are not a task's and not imported, for turns that hold a message of the operator's or a reply), and
keeps every exchange and mark written as it happened, writing only the touches it lacks (a restart or a late index
can leave one unwritten). A new exchange's place and person come from its message's own mark alone: a session's
binding is where it posts now, not proof of where a message arrived or who typed it, so history from before the
mark says `unknown` and names no person. A touch an older build keyed off its exchange's start is moved, not
re-derived: its own record is written at the start's key and the old key ended in the same frame, with no index
asked; the index only ever adds touches. The rebuild names things in the same 64 KiB of text as a live turn. No
mark is ever made backwards. A reply's counts for the next row carry the exchange that made them, so a naming that
finishes late never stands for a newer reply's. A touch whose place or person holds the key's separator is refused,
and a read keeps only keys that are exactly its own root's.

## Settings and bounds

`[context] ledger` and `assembly` are retired: both are ignored with a startup warning. The ledger cannot be
switched off; it is what the single view path reads. The mark rides its own submission (its event sink,
and its steer when it joins a running turn) to the node it becomes, so no other submission can take it, and a
refused submission takes it with it. In memory: one count of things per place for the next row (at most 256).

The outbox's load used to read every META record at a start; it reads its own three prefixes now, so these
families, which grow with history, add nothing to a start.
