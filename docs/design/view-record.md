# The view record, and its readers (issue 44)

What one turn was shown and why, kept so that "what did it see, and why?" has a one-command answer
(the design's risk 5: a view is harder to see than a transcript).

## What is stored

Three records, each written by `view::record::keep(store, turn, compilation, &view)`, the one call the
compiler makes for each view it builds. All three ride the turn's next frame (`Store::defer`); none is a
field on an existing record, so there is no store-format step.

| Key or kind | What it holds |
|---|---|
| META `view.<compilation>` | `ViewRecord`: the point of view, every piece shown by reference (its range, slot, band, reasons, form, marks, sizes, handle), every piece dropped and why (`merged`, `over_budget`, `unit_cut`), each band's residue, the tokens. No text. |
| META `view_turn.<turn>\u{1}<20-digit ms>\u{1}<compilation>` | which compilations a turn built views for, in the order built. |
| ledger `context.view` | `Tally`: per slot, per reason and per drop cause, pieces and tokens; ids and counts only. Summed over turns, it says which selector earns its tokens. A piece with two reasons counts under each. |

Reasons and drop causes are enumerated values (`view::Reason`, `budget::DropWhy`), never free text.

## Who reads it, and what each is shown

`view::record::show::Reader` is the one rule for every reader.

- **By default**: ids, kinds, forms, tokens, reasons, labels and counts.
- **Text** only through a `PieceText` the caller passes. `view.show` passes one only when `text` is asked
  for AND the caller is the owner's own shell (the CLI on this machine, no Theseus job's marker, no
  agent's process behind it); anywhere else `--text` is refused in words. Today the store's reader
  (`StoreText`) reads an exchange's ask and reply through its exchange record; other spaces give none.
- **In company** (`Reader::place`): only pieces whose mark names that place, and code's own text; drops
  by the place their range lies in; residues (which name no place) are not shown. The rest are counted
  in `held_out`, never listed.

The readers:

- `view.show {turn_id, text}` and `theseus view show TURN [--text]`: the terminal or the web UI only; never
  MCP. Every piece (the owner reads his own record).
- `look.theseus` with `what: ["view"]`: Theseus reads its own record for this turn. Who is present is the
  gate's own function, `view_gate::present`, from the turn's place, where its input arrived and that
  input's owner's mark; no argument of the call claims it. The owner alone: every piece. Anyone else
  present: this view's place's pieces only. Presence that cannot be read: nothing. The look's wall
  (`config_change::wall`) still holds first: the look is not offered in a shared place at all. Never text:
  a piece opens by its handle, through the history tool.
