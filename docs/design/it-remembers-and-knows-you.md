# It remembers and knows you: what 1.0.7 changed in memory, recall and the ontology's own writes

_Issue 9. Notes for the core crate that would have gone in its AGENTS.md, which this release leaves alone._

## What each part is, and where it lives

- **`self.open_work`** (`rsi/backlog/open_work.rs`). A read-only chat tool that lists the repository's open issues with their
  numbers, titles and links, and says how old the file it reads is. It reads only `[self_issues] file` through the night's own
  parse (no outside call, no token), is registered whenever `[self_issues] enabled` is true whatever `[self] mode` is, and is
  no shared place's tool. Its own description says Theseus's development is tracked in the repository's issues and that no
  other tracker applies. The `self` family's one-line summary names it, so the families judgment and `tools.find` can load it.
- **Fusion by coverage** (`theseus-index`, `tender.rs`, `vectors/status.rs`). While vectors are still being made, the vector
  source's weight is scaled by the share of chunks that have one, squared; a weight the query names is left alone. A busy
  table is read by its last counts, never as covered. The exact match leads only while the scaled weight stays under 1, that
  is below about 41% coverage.
- **The notice and the question** (`judge/onto/words.rs`, pinned word for word in `tests_onto_notice.rs`). The owner's agreed
  wording: Theseus acts, the conversation is named by its title, the kind of thing is a folder, file, link, person or topic in
  plain words, the whole name is shown, nothing says "tagged", "filed", "categorized" or "resource". A message Theseus wrote
  to itself (`Inbound.own`, an author `self:`) is never asked about; a path inside the state directory is no name
  (`OntoInput.private_dirs`); a name over the limit is dropped, not cut.
- **A kind named for its class** (`judge/onto/class.rs`, `tests_onto_class.rs`). With the sweep's model part on, each cluster
  the sweep is about to make a kind for is asked one cheap text call, paid under `[ontology] sweep_usd_per_day` and held on
  the day's ceiling, written as an `onto.named` row of kind `class`. The kind is the one lowercase word it gives; every name
  of the cluster is an entry under it. A failed, over-cap or unusable call, or a word that is a name itself, makes no kind
  that night. With the model part off the kind is named after the thing as before, and Jev's `name_it` pick names nothing now.
- **The one-time cleanup** (`judge/onto/tidy.rs`, `tests_onto_tidy.rs`; the owner's yes of 2026-11-14 to its rule). The sweep
  may retire an entry of the kind that holds folders and files, that Theseus made (`jev:`), with no guidance, every session
  filed under it a conversation of the self-improvement loop's own, as one undoable write (`sweep-<ms>-<n>-tidy`) that
  also takes those conversations off it. The ontology's writer rule moved for it (`theseus-ontology`'s `machine.rs`): the
  sweep may retire a category a machine wrote once no session is filed under it, and only `retired_ms` may change. A marker
  (META `onto.tidied`) is written once something is retired. The morning note says each one with its undo id.
- **A retired kind is shown nowhere.** `ontology.list` leaves it out, which both `theseus ontology kinds` and the cockpit read.
  Adding a kind under a retired kind's name is refused with words that say so.

## Notes not taken (review r3)

A cluster whose class word is already a kind is refused every night and the thing is never filed under the existing kind;
an unusable class answer is re-asked every night; `Mark::classed` is never pruned; no test pins the class call's stop at the
cap; the cleanup stays armed on a store with nothing to retire. All are bounded and none writes anything wrong.
