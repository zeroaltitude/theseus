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
exchange dependencies; recall checks its source session. Code and fact ranges are accepted directly. Unsupported
ranges and unproven sources are not shown, and text is read only after that check.

Since 2026-11-22, seals, veils, moves and the reader's presence withhold nothing. `session_marks` supplies no
seal, and the source check asks whether the place reads, not whether it is private. Totals count shown pieces;
the reader type still supports a restricted form, but the live view readers use its unrestricted form.

## The readers

- `view.show {turn_id, text, tally}` and `theseus view show TURN [--text] [--tally]`: read the turn's newest
  32 views, optionally with text. The current `rpc/views.rs` read path does not require an owner-in-person
  check. This describes a read, not authority to approve, publish or change policy.
- `look.theseus` with `what: ["view"]`: Theseus reads this turn's newest view from its widest point of view,
  rechecking the sources. At most 60 pieces, then how many more readable pieces remain. Never text.
