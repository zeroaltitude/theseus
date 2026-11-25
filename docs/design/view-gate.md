# The gate: what of memory may reach a model (issue 44, step 2)

> **Superseded on 2026-11-22: anything goes.** Nothing is withheld from a model request because of who is
> present, where it is, a seal or a move; every turn is assembled from the one memory by the same slots. What
> follows is the record of the gate as it was built. Authority (whose words a piece is) and the act gates
> (approvals, a place's tool ceiling, publishing into a shared place) are unchanged.

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

`view_gate::RecallPlaces` reads places for recall, `memory.lookup` (words, a session by id, a span's listing) and
`memory.search` with a session (`thread.read` was a third until issue 46 removed it):

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

What stands today beside anything goes is one scope rule of the self loop, not a withholding rule
(`view_gate::Verified::own_place_only`), decided by the role the loop records for each session it opens
(`rsi::roles`). The BUILDING sessions, the builder of a backlog item (role `builder`, written in the frame that
opens it) and a fixer, read at the widest, as any turn does. Every other self session lays its own place only:
its own exchanges there and the run it was handed, and its history tool pages, opens and searches nothing else,
imports included. That is a reviewer and the approver of a fix, whose independence from the builder's words and
everyone else's is their job; a mint's writer (no role), from whom the instances that check its tool are held
back; and any self session whose role is not recorded. A fork is given its source's role in the frame that opens
it. A self session whose execution or role cannot be read is `Unread`: its view is not assembled, never laid at
the widest by default.

## What stays, and the registry

The core decides by place in many places today (`place_of`, a target's `class_of`, `may_draw_on`, `may_draw_at`,
`owner_alone`). They stay. `tests_view_gate::no_new_decision_by_place_outside_the_gate` names each file's count as
it was when the gate landed and fails when one grows; `only_the_gate_calls_the_rule` fails on another caller of
the rule under any name a file gives it. Both are heuristics over the sources, not proofs.

## Left for the steps that follow

- The byte test (the bytes sent are identical with the forbidden records deleted) has not landed. Two properties
  stand in its place, and neither is about bytes: over generated stores, for a point of view that is not the owner
  alone, what the gate admits is the same with every other place's sessions left out of the store
  (`other_places_change_nothing_the_gate_admits_over_generated_stores`); and the view the selectors and the
  budgeter make is the same with exchanges the gate denies added (`view/select/tests.rs`). What the renderer and the
  compiler send is not tested this way.
- **Not behind the gate yet**: the place digest's count of held threads (with the ledger off, or for the owner alone, it says how
  many are sealed), session listings, and the text a turn was already sent before a steer narrowed its presence.
  (Kept tails, `turn::sent_tail`'s earlier tail a transcript turn sent again, went with the transcript on
  2026-11-22.)

## Seals at the message

A message a split or a claim moved into a thread (`moved.of.<node>`) is that thread's. `view_gate::Moved` reads it
for the readers that show exchanges: an exchange with any message or reply moved into a thread that is sealed,
seal-pending, unknown to the board or unreadable (every moved node, when the board cannot be read) is held whole,
at read time, from the view's window and selectors, the fold ladder's read, the history tool and `view.show`; the
reader's own thread is clear to it. A frozen view asks again after all of a call's preparation, before every send
(`view_step::view_readmitted`): a difference compiles the call again, at most twice, and a third declines the turn
(`moved`). The summary writers use `moved_at_all`: any move into any thread, whatever its seal (`ladder.md`).

## After the second review

- **Seals.** `seal_out` takes out every hit not proven clear, whatever the thread lens is; the drops are said in
  the recall's manifest as `sealed`, ids and scores only. A binding to a thread whose record is absent, and a native
  session with stored nodes and no record, are `Unread`. A node moved or claimed into a thread
  (`moved.of.<node>`) is checked against that thread's seal on a search and on every message a read by id or by
  session returns (`node_seal`, `readable_nodes`); a `moved.of` that does not decode, or names a thread the board
  does not hold, is `Unread`. A seal follows a merge chain. The ladder's `ladder_hidden` asks the same rule.
- **Presence** is made only by the gate (`view_gate::begin`, from the input's mark or what set the turn up, and
  `steered`, which only narrows). `Asking`'s fields and the table's writer are private. A steer the running turn
  absorbs narrows it at once; what the turn sets up inherits the narrowest of its exchange's messages' marks.
  `memory.lookup` takes the turn's presence whole when the call starts.
- **A task's report** whose place has moved or narrowed since the task started is delivered as a content-free
  notice (`report_shown`, `withheld_report`); its body stays in the task's session.
- **What every turn reads besides the view**: an earlier `Recall` node's sources are admitted again under the
  current turn (`readmit_sources`); the place digest names and counts other threads only for the owner alone
  (`digest_threads`); look.work gives another conversation's words only where the gate admits it; a standing record
  kept from another conversation's message is held where the gate does not admit that conversation
  (`standing_held`).
- **`view::gate::Admitted::new` is private to its module**; `admit_checked` beside it is the gate's construction
  (the contract's authorized amendment), with a `cfg(test)` constructor for the view's own tests.
