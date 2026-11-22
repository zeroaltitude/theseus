# Releasing one private piece in company: the trigger and the look (issue 44, step 3b)

The owner's decision 2 (`context-point-of-view.md` §15): in company, Theseus is shown nothing he said privately.
When a message of his there seems to lean on something that is not in that place, Theseus says so from the visible
message alone, without confirming that anything matching exists. Separately, in his private conversation and under
that place's own rules, a card can offer to release one piece; nothing crosses without his press. The plan calls
this step 3b ("code lays a second view anchored at his private conversation for the same message; it searches there
and posts there, with the card that can release one piece. The shared view never learns what was found").

This note says what is built and in what state: the trigger and the look, both in shadow. The card and the release
are not built; the last section says what they take.

## 1. The trigger: `leans_on.v1`, a new pack, in shadow

A new pack of its own, not a version of an existing one (`crates/theseus-judge/packs/leans_on.v1.toml`; its builder
`builders/leans_on.rs`; asked by `theseus-core`'s `judge/leans_on.rs`).

- **Asked of** the owner's own message only (his mark's person is the owner: in person, or a chat author who holds an
  owner handle), at a turn whose presence the gate made a private channel or shared (`leans_on::asked_of`). Never
  asked of another person's message, an agent's process, a message with no mark, or his message where he is alone.
- **Asked from** `turn::gate_step`, right after the turn's presence is made, when `[context] ledger` is on.
- **Its state** is the message and the last eight lines of the turn's own session, each `person` or `assistant`,
  and nothing else: no other place, no memory, no recall, no author, no names. It is the place's own point of view.
- **One Choice**: `stands_alone`, `said_here`, `not_said_here`, `unclear` (also its `no_match`). The design's own
  answers (nothing / what was just said / something earlier / a named thing / unclear) are asked as one question
  about this place.
- **Off the turn's path.** It returns at once; the session's read, the state, its blob, the shadow budget's
  reservation and the call are a spawned task's, as `memory.v1`'s are. A slow, failing or absent judge changes no
  turn. Its row is `judge.call`, scoped `judge:leans_on`.
- **In shadow.** `judge::WIRED` names it at `Shadow`, so its ladder line is shadow (and every test rig that
  switches off the wired packs but its own switches it off too): the design's §6 says a new acting question is promoted the way the build already requires (a frozen holdout of 200 labelled
  judgments, or the owner's forced promotion). Nothing acts on its answer: the company turn is the same turn,
  byte for byte, whatever it says. `[judge.packs."leans_on.v1"] rung = "off"` switches it off.
- **Labelled cases**: `packs/leans_on.v1.cases.toml`, sixteen invented, with the traps (a reference satisfied in the
  place, a general question). `builders::leans_on::tests::leans_on_cases` holds each label to the pack's options and
  each state to the message and the place's lines. Measuring the pack against them is a replay against the judge,
  the ladder's step before any promotion.

## 2. The second look, under his private conversation's rules, in shadow

`theseus-core`'s `release_card.rs`. Once the trigger is judged, its answer is handed to the look, in a task spawned
from the trigger's own task (never the turn's).

- **Only on `not_said_here`.** Any other answer, or none, is no look.
- **His private conversation** is the first direct conversation the bindings bind, as the ledger names places
  (`discord:dm:<id>`). None bound: no look (`no_private_conversation`).
- **Its own rules, never the company turn's permit.** The look is a history-tool lens (`history_tool::page::Lens`)
  anchored at his private conversation with the owner alone there; the company turn's presence and permit are not
  read. A candidate is an exchange of that conversation, of his own words (`person` is the owner), that the lens
  admits: the place rule, and the seal failing closed (`Core::ladder_hidden`: a session not proven unsealed, or an
  imported one, is held out). The company place's exchanges are never candidates; neither is another person's
  words, or another conversation's.
- **By the means that exist**: the words index, scoped first to the sessions the lens admits on that conversation's
  timeline (`history_tool::found`, the history tool's own words search), with the message as the query.
- **What it decides.** One candidate is the one case a card would offer. Several are no card: the design's §6 asks
  which of up to three, and a card that asks which is a later step; the smaller safe thing is no card. None is none.
  Words that match are a candidate, not a supported piece: before a card is offered for real, the rung-2 support
  check of §6 (a small model on the answering provider says whether the passage supports the reference, and code
  checks it named a candidate it was given) belongs between the search and the card.
- **What it records**: one `context.release_look` ledger row for EVERY judgment of the trigger, whatever it
  answered and whatever the look found, under no session and no turn: `company_turn`, `node`, `answer`, `mode`
  (`shadow`), `outcome` (`not_asked`, `no_private_conversation`, `no_index`, `nothing`, `one`, `several`), `found`
  (a count) and `ids` (at most three exchange ids, newest first). Never a piece's words.

## 3. What the company turn learns: nothing

- The turn's request is built before and apart from both: neither the trigger nor the look writes anything the
  turn, its compile or its tools read. Nothing waits on either, so no delay depends on them.
- The look's row is under no session and no turn, so nothing that reads a session's or a turn's rows finds it, and
  one row is written for every trigger judgment, so even the number of rows does not depend on what was found.
- The look reads the owner's private conversation only in the trigger's spawned task, and hands nothing back.

## 4. State, and what turning it on takes

| Piece | State | To change it |
|---|---|---|
| `leans_on.v1`, the trigger | **shadow**: asked and recorded, acted on by nothing | the owner's promotion (`theseus packs promote leans_on.v1 --live`, his forced promotion with the numbers), or a holdout of 200 labelled judgments; switched off by `[judge.packs."leans_on.v1"] rung = "off"` |
| the look | **shadow**: runs after a `not_said_here`, records ids and counts only, shows and sends nothing | nothing to turn on until the card exists |
| the card | **not built** | below |
| the release | **not built** | below |

## 5. Not built: the card and the release

- **The card**, by the existing path the card questions use (`card/dm.rs`): one outbox post of a new kind through
  `Outbox::stage_to_operator`, to his direct conversation, drawn with two buttons by the Discord binding
  (`theseus-discord`'s `runtime/card_question.rs` is the model), saying where he was asked (the place by the name he
  knows it by), the piece's words in full, and that what is released cannot be unsent. Only for `one`, and only
  after the support check above.
- **The release**: an answer method beside `card.answer`, judged by `Core::judge_act` (the presser is the owner, in
  a private place; an agent's process or a forged answer is refused), writing a record of the piece released to that
  place by him, which the gate reads (`view_gate`: that piece, and nothing else, admissible at that place,
  marked as released by the owner), and an end of that record for a withdrawal. A sealed thread's piece is never
  offered (the look's lens already holds sealed sessions out) and never released (the release checks the seal
  again).
- **Red-team tests the step closes on**: the company model's request never holds the piece or any sign of the look;
  another person in the place cannot trigger or press anything; a forged answer from an agent is refused; a sealed
  piece is never offered; a released piece is admitted in that place alone, and a withdrawal ends it. The first,
  second and fourth are held for the trigger and the look by the tests named in the code; the others need the card.
