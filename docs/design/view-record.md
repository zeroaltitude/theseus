# The view record, and its readers (issue 44)

What one turn was shown and why, kept so that "what did it see, and why?" has a one-command answer
(the design's risk 5: a view is harder to see than a transcript).

## What is stored

Three META records, each written by `view::record::keep(store, turn, compilation, &view)`, the one call the
compiler makes for each view it builds, with the turn's own store handle. All three ride the turn's next
frame (`Store::defer`); none is a field on an existing record, so there is no store-format step. A turn or
compilation id holding the key's separator is refused.

| Key | What it holds |
|---|---|
| `view.<compilation>` | `ViewRecord`: the point of view, every piece shown by reference (its range, slot, band, reasons, form, marks, sizes, handle), every piece dropped and why (`merged`, `over_budget`, `unit_cut`), each band's residue, the tokens. No text. |
| `view_turn.<turn>\u{1}<20-digit u64::MAX - ms>\u{1}<compilation>` | a turn's views, newest first (within one millisecond, by compilation id). |
| `view_tally.<compilation>` | `Tally`: per slot, per reason and per drop cause, pieces and tokens. A piece with two reasons counts under each. Not a ledger row: `ledger.tail` gives any terminal caller its rows. |

## A record is a map, not a permission

Every reader checks each piece's provenance again before showing its reference (`view::record::source`).
An exchange range must resolve to readable sessions; a card or waiting line must carry readable session and
exchange dependencies; recall checks its source session. A fold is found again by its handle: today's node
must be of the range's place, level and ends, and every exchange it stands for must resolve (the rule the
model's own assembly holds a fold to), be of that place and come from a readable session (two checks the
assembly does not make). A task's card needs the task's record today and a readable home session. Code and fact
ranges are accepted directly. Unsupported ranges and unproven sources are not shown, and text is read only after
that check; a fold or a card has no text a reader is given.

This proof is not the assembly's whole scope check. A self-loop's permit (`view/permit.rs`) and a fork's cut
(`fork/window.rs`) are kept by what the assembly lays; the reader does not apply them again, and is given the
point of view only.

What a listing promises: this reader is a read-only listing, and its source lives for one call (one `view.show`,
one preview, one look). Every answer in one call's listing was true at some moment during that call. A write
that lands while the call runs may or may not be reflected, as with any read that races a write. Nothing proved
in one call is reused by another, so a write that finished before a call began is always in it. Within a call
the source proves a fold or a card once however many views hold it, and keeps each answer with the store's last
log position; a later view of the same call proves it again when that position has moved. That stamp is a
best-effort freshener, not a guarantee: the store's index publishes a write after the log position moves, and a
place binding is published after its record is written, so an answer proved in either gap can be kept under a
position that no longer moves. The listing equals the stored tally's slots only while every source still reads;
the tally is history.

What a proof costs is counted in units: a fold's node and each exchange it stands
for, and the task list once. These are not the store's own reads: a node is found through its handle's record,
each session's place is read, and the task list reads every task. The owner's `view show` proves everything, on
the blocking pool. The model's own look also reads on the blocking pool, and spends at most `look::view::PROOF`
units (16,459: a settled ladder's rungs 1 and 2 whole, and the task list); a fold or a card past that is not
listed, and the look says how many in one line.

Since 2026-11-22, seals, veils, moves and the reader's presence withhold nothing. `session_marks` supplies no
seal, and the source check asks whether the place reads, not whether it is private. Totals count shown pieces;
the reader type still supports a restricted form, but the live view readers use its unrestricted form.

## The readers

- `view.show {turn_id, text, tally}` and `theseus view show TURN [--text] [--tally]`: read the turn's newest
  32 views, optionally with text. The current `rpc/views.rs` read path does not require an owner-in-person
  check. This describes a read, not authority to approve, publish or change policy.
- `look.theseus` with `what: ["view"]`: Theseus reads this turn's newest view from its widest point of view,
  rechecking the sources. At most 60 pieces, then how many more readable pieces remain. Never text.
- `view.preview {place, message, text}` and `theseus view preview PLACE [--message TEXT] [--text]`: the view a
  next turn of the place would be given, listed through the same reader. It lists proved fold drops with their
  residue, and names each fold's rung (`level`, `folds@n`).
