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

Every reader checks each piece's source again today before showing its reference (`view::record::source`):
the source session's marks through `view_gate::session_marks` (its place, today's reading of it, its seal,
fail-closed) against the reader's point of view through `view_gate::Gate::check`. A sealed, seal-pending or
unreadable source is not shown. Only exchange ranges resolve to sessions today; fold nodes, facts, cards and
tasks are not shown until their provenance can be read. Text is read only after that check.

Nothing counts what is left out. Totals are of what is shown; a restricted reader (not the owner alone) gets
no residue, no at-hand reference, and one fixed sentence, so two records that differ only in what it may not
see read the same, byte for byte.

## The readers

- `view.show {turn_id, text, tally}` and `theseus view show TURN [--text] [--tally]`: the owner in person
  alone (`places::owner_in_person_of`: the CLI or the web UI with no agent's process behind it; a job is one),
  and no job's marker. Anyone else is refused, the record as well as its text. The owner's point of view at
  the record's anchor, with no asker: inspecting lifts no seal. Its newest 32 views.
- `look.theseus` with `what: ["view"]`: Theseus reads this turn's newest view. Who is present is the gate's
  record of this turn (`view_gate::Verified`), for this turn alone; unknown is shown nothing. The turn's
  point of view, narrowed by the record's (`view::permit::point_of_view`), is the source check's. At most 60
  pieces, then how many more it may read. Never text.
