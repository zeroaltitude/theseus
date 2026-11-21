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
