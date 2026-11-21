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
  acts. Nothing while `[context] ledger` is off, nothing for a task's turn (it has no exchange), and nothing
  when the exchange is not in the store.
- No judge call is added; the request is byte-identical.

The read is `names::topics::timeline(store, id, n)`: one bounded read of the touch index, newest first.

## Numbered items (`names::items`, the index's `entity::item`)

The index's extractor gains an `item` kind: `issue 33` and `issue #33` are `item:issue-33`, `PR 12` and
`pull request 12` are `item:pr-12`, a bare `#33` is `item:33`, and `owner/repo#33` is `item:owner/repo#33`. A
`#` after a word (`C#3`), a URL's fragment, Discord's `<#id>` and a number of more than six digits are none.

Items are kept **apart from the entity field**: `entity::entities` is unchanged, so the index's exact arm and the
memory pass's labels (what recall and the gate read) see nothing new. The core asks for items by a method of
their own, `index.items` (the params and result of `index.entities`); a tender older than this build answers "no
method" and the exchange simply has no items.

A bare number never makes an identity by itself. When the session the message ran in serves a project, an
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

The matcher reads a person's words. A **qualified name** (a key, or a task's id, as a whole token) wins and names
one thing. An **alias** is matched as whole words, case ignored, at most eight words and at least three
characters; a longer name said wins over a shorter one inside it, and each word belongs to one match. An alias is
accepted only when exactly one *live* thing holds it (an active thread not merged away, an open task, a category
not retired or merged); otherwise every holder is returned as a candidate, the live ones first.

