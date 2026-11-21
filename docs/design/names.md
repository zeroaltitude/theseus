# Recognition: topics per exchange, numbered items, scoped names, and a thing's card

Issue 44, step 5's recognition half. The code is `crates/theseus-core/src/names.rs` and its modules. Nothing
here is sent to a model yet and no request changes: these are readers and an index, for the selector that
calls a named thing in.

## Topics per exchange (`names::topics`)

The judge's `onto.v1` already rides the request sent for every message a person sends, and files the
*session* under categories. The main conversation is one long session, so a topic could not find the exchanges
that were about it. Now, in the sink's frame that writes the judgment's row, each category the answer chose at
`[ontology] act` writes a touch to the exchange of that message: `touch.thing:<category id>␁…␁<turn>`, at the
exchange's own start time. A category id is already `<kind>:<local>` (`topic:rust-harness`), and no ontology
kind is a type of the index's extractor, so the id is the term.

- A listed candidate touches whether or not the filing writes anything (a session already filed under it
  writes no membership; the exchange was still about it). A retired or merged category does not.
- A category minted at the act band touches the exchange that minted it.
- `project` is not a category and touches nothing here.
- The same judgments the filing acts on, and no others: answered by the model the pack pins, in a mode that
  acts. Nothing while `[context] ledger` is off, and a task's turn is never judged by `onto.v1`. When the
  judgment's frame is written before the exchange's (the sink writing beside a running turn), the touch takes
  the judgment's turn and session at the message's own time.
- No judge call is added; the request is byte-identical.

The read is `names::topics::timeline(store, id, n)`: one bounded read of the touch index, newest first.

## Numbered items (`names::items`, the index's `entity::item`)

The index's extractor gains an `item` kind: `issue 33` and `issue #33` are `item:issue-33`, `PR 12` and
`pull request 12` are `item:pr-12`, a bare `#33` is `item:33`, and `owner/repo#33` is `item:owner/repo#33`. A
`#` after a word (`C#3`), anything inside a URL (every URL is blanked before the patterns run), Discord's
`<#id>` and a number of more than six digits are none.

Items are kept **apart from the entity field**: `entity::entities` is unchanged, so the index's exact arm and the
memory pass's labels (what recall and the gate read) see nothing new. The core asks for items by a method of
their own, `index.items` (the params and result of `index.entities`); a tender older than this build answers "no
method" and the exchange simply has no items.

Items are asked for apart from the other things, each with its own receiver, and given up after five seconds
(`names::items::WAIT`): a slow or stuck `index.items` never costs the things named beside them.

A bare number never makes an identity by itself. When the session the message *arrived at* (its owner's mark's
`arrived_session`; the running session only when there is no mark) serves a project, the scope is kept with the
exchange for the reply's items too, and an
unqualified item is scoped by it (`item:project:<id>#issue-33`); with none, it stays `item:issue-33`, an
unscoped candidate that a reader may offer and never treats as one thing. `owner/repo#33` names its repository
and is kept as said. The touches are written as for the other things: the message's when the exchange ends,
the reply's by the place's next exchange.

The exchange rebuild (`exchange.rebuild`) does not yet ask for items, so a rebuilt exchange has none.

## Scoped names (`names::table`)

A table of names built from the stores that hold them, with no word list and no pattern:

| Thing | Key | Names |
|---|---|---|
| a line of work (a thread record, through the threads' board) | `thread:<slug>` | its title, its slug |
| a task | `task:<id>` | its title; its bare id is qualified too |
| a topic (a category of a kind `onto.v1` files under, `project` apart) | its id, `topic:<local>` | its name |

The table is built for a caller (`table::build_for(core, gate)`): a thing whose record the gate refuses
(`card::Admits::thing`, with the thread, task or category record) is left out before any alias is resolved, so
it is no match, no candidate and makes no visible alias ambiguous. A sealed thread's same-thread exception is
the gate's to decide. `table::build` is `build_for` with `table::Whole`, which admits everything.

The matcher reads a person's words. A **qualified name** (a key, or a task's id, as a whole token) wins and names
one thing. An **alias** is matched as whole words, case ignored, at most eight words and at least three
characters; a longer name said wins over a shorter one inside it, and each word belongs to one match. An alias is
accepted only when exactly one *live* thing holds it (an active thread not merged away, an open task, a category
not retired or merged); otherwise every holder is returned as a candidate, the live ones first.

## A thing's card (`names::card`)

One read: the title (`Topic: Ledger (topic:ledger)`, with `(closed)` for a thing no longer live), and under it at
most 12 lines: what it is about (a thread's goal, a task's objective, a category's description), up to five
current facts, up to two open tasks, the owner's guidance (a category's), and the newest three exchanges of its
timeline with the words that opened each, cut by code. A cut list says how many it left out.

- **Facts** are the owner's standing records whose scope is the thing's own place or session (a line of work's
  `thread:<slug>` place and its session, a task's session), live ones only (`standing::live`: a revoked,
  replaced or lapsed record is dropped). No stored record names a fact's subject yet, so a topic has no facts of
  its own until one does.
- **The gate is a parameter** (`card::Admits`) and covers the whole card: the thing's own record first (refused,
  there is no card at all: no title, goal, task, guidance or id), then every open task, the guidance, every
  standing record and every exchange, before it is read. A method a gate does not give denies.
- **Facts are decided inside the admitted history**: the gate filters the standing records first and only then
  is what is current resolved, so a refused correction ends nothing the card shows.
- **Exchanges are paged** back through the timeline until three are admitted or it ends: refused exchanges crowd
  out none it admits, and no hidden count is reported. The view's gate (`view::gate`) is meant to stand behind it.
