# The board's memberships are read when asked, not held (issue 21)

The people-and-topics board is the ontology's categories and the lists of categories each session holds. On the
owner's store that is 414 categories and about 71,000 memberships over 51,000 sessions, and the daemon kept all of
it in memory for as long as it ran: 15 to 25 MiB of the 41 MiB it builds from the store at start, and it grew with
every import. A write cloned the whole of it too (`Board::write` and `Board::stage` check against a copy), so a
membership change moved the daemon's memory by the board's size and left it to the allocator to give back.

## What is held, and what is read

The snapshot (`theseus_ontology::Ontology`) holds the kinds, the categories, the guidance, and for each category how
many sessions hold it and the newest time it was held. It holds **no membership list**. A list is read from the
store (`onto:member:<session>:<kind>`) when something asks (`MemberSource`, implemented over the store by
`theseus_core::ontology::StoreMembers`). The snapshot built with `Ontology::load` still holds every list: tests,
trial copies and callers with no store use it, and it answers the same.

| Reader | What it reads | Cost now |
|---|---|---|
| a turn's walk, recall's category scene, the judges' per-session reads | one session's lists, `memberships(session)` | one range read of the store's index; the last `KEPT` (2,048) sessions read are kept, and a write to a session forgets it |
| a category's use: refusing to retire or merge one something holds, the split candidates, `ontology.list`'s counts | the counts | none: in memory |
| when each category was last held (the judge's prompt) | the newest times | none: in memory (each category's membership times, so a dropped newest one is answered too) |
| a sweep, a merge, `held_by`, `ontology.list` with every membership, the lists of one kind | every list | one scan of the store's `onto:member:` records, as the load makes |

## What is the same, and what moved

Every answer is the held snapshot's: the same lists, in the same key order, the same counts. Three things differ,
none of them a person's answer:

- A list the load trimmed (an entry whose category is not held) or dropped (a kind that is not held) stays trimmed or
  dropped: the snapshot holds the trimmed list and hides the dropped one, so the store's untrimmed text is never read.
- A write puts its lists in the snapshot it is checking until the store has them (`Ontology::settle`, called by the
  board after the append), so a write that puts two lists of one session sees the first when it checks the second.
- A reader that holds an older snapshot across a later write reads that write's lists, not the old ones: the store is
  the one copy. A turn's walk takes its memberships once, when it is made, and keeps them, so a turn is unchanged.

A store that cannot be read answers "no list" and warns; the held snapshot could not fail.

## How it is proved

`theseus-ontology`'s `tests_lean.rs` builds a held and a lean snapshot from the same records and asks both everything
a reader asks, after the load and after each write: lists, counts, newest times, records, merge pairs, a session's
memberships and lists, retire checks. The core's own tests run on the lean snapshot (the board loads with it), among
them the one that holds the kept snapshot equal to a restart's.
