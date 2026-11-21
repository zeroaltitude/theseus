# The gate: what of memory may reach a model (issue 44, step 2)

Issue 44's context design builds every view from one memory, for one point of view. This step builds the gate that
view stands on: the type that only the gate can make, the narrowest value of who is present ("owner alone"), the
property tests that hold the rule, and two repairs that stand whatever happens to the rest of the design.

## The type and the rule

`theseus_memory::gate` is pure, beside today's place rule (`theseus_memory::recall::Place`):

- `Admitted<T>`: a value the rule let through. Its fields are private and `gate::admit` is its one constructor,
  so a reader that takes an `Admitted<T>` cannot be handed a piece nobody checked.
- `PointOfView { anchor, place, present }`: the place a turn is anchored at, today's reading of it, and who is
  present.
- `Present`: `OwnerAlone`, `PrivateChannel`, `Shared`, `Open`, `Unknown`.
- `Marks { place, class, time_ms, seal, veil }`: what the gate reads of one piece.

The rule (`gate::check`), in order: a point of view whose presence is unknown admits nothing; a piece whose place,
seal or veil cannot be read is denied (a missing or uncertain mark denies); a sealed or veiled piece is denied;
today's place rule (`Place::may_draw_at`) must hold; a piece from any place other than the anchor needs
`OwnerAlone`. `OwnerAlone` is believed only where today's rule reads the anchor as private. There is no rule by
model provider (the owner's decision 8).

`theseus_core::view_gate` is the rule's one caller (`Gate::admit`). It reads the point of view from the turn and
a piece's marks from the store:

- `present(place, target, input, marks)`: a shared place is `Shared`, the open view `Open`, an unreadable place
  `Unknown`; a private place is `OwnerAlone` only on the terminal, the web page or a DM, for an input whose
  owner's mark says the owner typed it in person. A channel is never alone, whoever is in it (decision 5). An
  agent's process at the terminal is not alone: its mark says it was not the owner in person. An input with no
  mark is not alone. A mark that cannot be read is `Unknown`. A wake or a task's report takes the presence of the
  turn that set it up (`Input::SetUp`); none recorded is `Unknown`.
- `session_marks(runner, asker, session, veil)`: a session's place (its target, `local` for the terminal and the
  web page, `imported` for imported history), today's reading of it, and its seal (`seal_of`).
- The owner's mark is read through the trait `OwnerMarks`. Step 1 (the ledger) writes it as META `said.<node>`;
  until step 1's reader `exchange::mark` is on the same line, the store's impl reads that key itself, taking only
  its `in_person` field.

## The repairs

- **A session read by id holds out a sealed thread's.** `memory.lookup`'s read of a session (alone or with
  words), its listing of a span's sessions, and the MCP server's `memory_get` refuse or leave out a session that
  serves a sealed thread or one whose seal is proposed (`view_gate::seal_of`), unless the asker is that thread.
  Recall's own pipeline already held them out (`theseus_memory::threads`); a read by id did not.
- **A channel bound private draws on itself alone** (decision 5: "someone could join after and read the
  history"). `view_gate::RecallPlaces` reads places for recall: for an asker in a channel bound private, the
  asker reads as that channel and its own sessions as it, so today's place rule drops every other session for
  `place`. The turn's recall and its rerank, `memory.lookup` (words, a session by id, a span's listing, which
  then lists no imported episode), `memory.search` with a session, and `thread.read` follow it. The terminal, the
  web page, a DM and a shared place recall as before.

## What stays, and the registry

The core decides by place in many places today (`place_of`, a target's `class_of`, `may_draw_on`, `may_draw_at`,
`owner_alone`). They stay as they are. `tests_view_gate::no_new_decision_by_place_outside_the_gate` names each
file's count as it was when the gate landed and fails when one grows or a new file starts deciding by place: a new
reader reads places through the gate. `only_the_gate_calls_the_rule` fails on a second caller of the rule anywhere
in the workspace.

## Left for the steps that follow

- Today's recall at the terminal does not yet narrow for an agent's process: that needs the owner's mark on every
  turn's input, which step 1 writes. Once it does, the turn's recall can read its presence through `present` and
  an agent's session draws on its own place alone, as decision 5 says.
- The selectors and the renderer of the view call `Gate::admit` for every piece they bring; the byte test (the
  bytes sent are identical with the forbidden records deleted) lands with the assembler (step 6).
