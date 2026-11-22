# The gate: what of memory may reach a model (issue 44, step 2)

Issue 44's context design builds every view from one memory, for one point of view. This step builds the gate that
view stands on: the rule, the narrowest value of who is present ("owner alone"), the turn's presence carried to
the readers that run today, seals on every path, and the view's permit.

## The rule

`theseus_memory::gate` is pure, beside today's place rule (`theseus_memory::recall::Place`), and returns a decision
only (`check(pov, marks) -> Result<(), Denied>`):

- `PointOfView { anchor, place, present }`: the exact arrival place, today's reading of it, and who is present.
- `Present`: `OwnerAlone`, `PrivateChannel`, `Shared`, `Open`, `Unknown`, ordered widest first.
- `Marks { place, class, time_ms, seal, veil }`: what the gate reads of one piece.

In order: unknown presence admits nothing; a piece whose place, seal or veil cannot be read is denied; a sealed or
veiled piece is denied; today's place rule (`Place::may_draw_at`) must hold; a piece from another place than the
anchor needs `OwnerAlone`, believed only where today's rule reads the anchor as private. No rule is by model
provider (the owner's decision 8).

`theseus_core::view_gate` is the rule's one caller (`Gate::check`). What a model is shown is
`view::gate::Admitted`, which only the view's gate makes (`view::permit::Permit`), from that answer.

## A turn's presence

`turn::gate_step` computes it once, after the turn's input is written, when `[context] ledger` is on:

- a person's message: its owner's mark (`exchange::mark`, written by step 1). `OwnerAlone` only when the mark
  says the owner typed it in person and it arrived at the terminal, the web page (`local…`) or a DM. A channel is
  never alone, whoever is in it (decision 5); an agent's process is not alone; a message with no mark is not
  alone.
- a turn no message opened: the presence the turn that set it up recorded. Each turn records its own as META
  `presence.<turn>`; a wake's `wake.at` call, a task's `task.create` call (for the task's own turns and for its
  report) name the turn that made them. Of several, the narrowest; none recorded is `Unknown`.

It is held per session (`TurnRunner::presence`) for the tools the turn runs, and lost at a restart: a session with
none reads as not the owner alone.

## Recall's places

`view_gate::RecallPlaces` reads places for recall, `memory.lookup` (words, a session by id, a span's listing),
`memory.search` with a session, and `thread.read`:

- a channel bound private draws on itself alone, whatever the ledger says;
- with the ledger on, a turn that is not the owner alone, proof absent included, draws on its exact arrival place
  alone (the ledger's `local:<dir>`, or a binding's target). Its listing names no imported episode, and it opens
  no other thread;
- with the ledger off, the terminal, the web page and a DM read places as before.

The MCP server's memory tools (`[mcp_server] memory`) read as before: they are another agent by design, behind the
owner's own switch, kept to private sessions and now to seals.

## Seals on every path

`view_gate::seal_out` reads the seal of every index hit, before anything ranks, reranks or sends it, on the turn's
recall, its rerank, `memory.lookup` and the MCP server's search, and `memory.search`, whatever `[threads] enabled`
says. A hit not proven clear is held out; the thread's own sessions are not held from it. When the thread lens is
on, a `Held` hit is left to it, so its drop is counted `sealed`.

The seal's resolver (`view_gate::seal_of`) fails closed: a session, ancestry or binding record that does not
decode, a task chain too long to follow, a thread the board does not hold, or a thread record that did not decode
when the board loaded (`threads::Board` keeps those) is `Unread`. A thread binding that did not decode makes every
session with no proven thread `Unread`.

## The view's permit

`view::permit::Permit` is made for one turn from `view_gate::Verified` (the turn's computed presence, today's
reading of its place, its exact arrival place). The view's own `PointOfView`, which `PointOfView::compute` makes
from a caller's `Arrival`, may only narrow it, and a view anchored elsewhere than the verified turn passes nothing.
`Admitted::code` admits only a piece with no source place and no seal, veil or outside mark.

## What stays, and the registry

The core decides by place in many places today (`place_of`, a target's `class_of`, `may_draw_on`, `may_draw_at`,
`owner_alone`). They stay. `tests_view_gate::no_new_decision_by_place_outside_the_gate` names each file's count as
it was when the gate landed and fails when one grows; `only_the_gate_calls_the_rule` fails on another caller of
the rule under any name a file gives it. Both are heuristics over the sources, not proofs.

## Left for the steps that follow

- The byte test (the bytes sent are identical with the forbidden records deleted) lands with the assembler (step 6).

## After the second review

- **Seals.** `seal_out` takes out every hit not proven clear, whatever the thread lens is; the drops are said in
  the recall's manifest as `sealed`, ids and scores only. A binding to a thread whose record is absent, and a native
  session with stored nodes and no record, are `Unread`. A node moved or claimed into a thread
  (`moved.of.<node>`) is checked against that thread's seal on a search and on every message a read by id or by
  session returns (`node_seal`, `readable_nodes`); a `moved.of` that does not decode, or names a thread the board
  does not hold, is `Unread`. A seal follows a merge chain. The ladder's `ladder_hidden` asks the same rule.
- **thread.read** holds the call for the owner when the board cannot be read at planning, and at execution reads a
  sealed thread only on a call the owner approved.
- **Presence** is made only by the gate (`view_gate::begin`, from the input's mark or what set the turn up, and
  `steered`, which only narrows). `Asking`'s fields and the table's writer are private. A steer the running turn
  absorbs narrows it at once; what the turn sets up inherits the narrowest of its exchange's messages' marks.
  `memory.lookup` takes the turn's presence whole when the call starts.
- **A task's report** whose place has moved or narrowed since the task started is delivered as a content-free
  notice (`report_shown`, `withheld_report`); its body stays in the task's session.
- **The transcript path**: an earlier `Recall` node's sources are admitted again under the current turn
  (`readmit_sources`); the place digest names and counts other threads only for the owner alone
  (`digest_threads`); look.work gives another conversation's words only where the gate admits it; a standing record
  kept from another conversation's message is held where the gate does not admit that conversation
  (`standing_held`).
- **`view::gate::Admitted::new` is private to its module**; `admit_checked` beside it is the gate's construction
  (the contract's authorized amendment), with a `cfg(test)` constructor for the view's own tests.

## The last readers on the old path

- **Earlier tails** (`turn::sent_tail`): who is present, and where the turn arrived (`presence_key`), is part of
  the key a kept tail is put back under, so a tail made where the owner was alone is not sent to a narrower
  audience. Each tail keeps the threads held from its reader when it was made (`held_threads`); a tail is let go
  when a thread is held now that was not then, or when the threads cannot be read. The keep-warm read applies the
  same seals. A tail does not record which thread's words it holds, so a new seal lets every older tail go, not
  only those that held that thread's words: it may cost a cache read, never a leak.
- **The place digest** is given no thread held from a reader that is not the owner alone (`digest_threads`,
  whatever the ledger says), so it neither names nor counts one: a count says a thread exists. The owner alone is
  still told how many, as the digest's own rule (`a_place_digest_never_shows_a_sealed_thread`) has it.
- **The session listing** shows no title for a session with a message moved into a thread held from the reader
  (`title_shown`): a title is made from a session's first inputs. The session is still listed.
- **Health** counts the thread bindings that do not read (`threads.unread_bindings`, never which), the condition
  under which recall holds out every session whose thread it cannot prove. Binding the thread again clears it.
- **Not done: a turn whose presence is `Unknown`** (a wake set by a wake) still draws on its own place through
  recall's places. Reading nothing for every `Unknown` turn also blanks every turn whose input carries no mark (a
  surface or a path that does not write one), so it needs `Proof::SetUp` to tell "set up by a turn with no message"
  from "no mark written" before recall narrows to nothing on the first alone.
