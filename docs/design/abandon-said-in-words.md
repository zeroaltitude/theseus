# Abandoning on the owner's word: the mechanism, and the four questions that stop it being built

**This note decides nothing.** It is the readable state of one mechanism plus the questions that stop a build
starting, written so that the questions can be answered once instead of guessed at three times. It is not a plan,
it is not a decision record, and nothing below is a recommendation.

_Written 2026-11-09 against base `f4a37668`; every `file:line` was re-read in that tree, and where a read
disagreed with the brief behind the note, the read is what is written and the disagreement is named. Paths are
under `crates/theseus-core/src/` unless given whole._

## What landed, and what did not

> **CORRECTION, 2026-11-10 (theseus-3i6d1): sub-items (2) and (3) are BUILT, and everything below this line
> describes the base before they were.** This note landed at `ea656f0d` (2026-11-09 17:16) and
> `rpc/answer_in_words.rs` landed at `e4283567` (2026-11-09 17:29) off a sibling branch — thirteen minutes later and
> not a descendant of this one — so the table and the searches below went stale the same evening. **Do not build (2)
> or (3) again.** What landed: (3) first, because it is what makes (2) safe — a yes resolves a question only when
> there is exactly one it can mean, and with more than one it resolves nothing while a notice where approvals go
> names each and asks which; then (2) — a bare yes in a place only the owner writes in answers the one question
> waiting there, through `Core::confirm_action`, read at `turn.submit` before the turn runs. The four questions
> below were answered by that build rather than by a ruling: Q1 any question waiting at the place, Q2
> `capture::owner_alone`'s surfaces and not a guild channel bound private, Q3 a notice rather than a card, and **Q4
> needed no authority widening at all** — a words-yes goes through the same `confirm_action` as a typed one, so
> `ran_at == Posture::Approve` and `said_yes` comes out true with no change to `toolrun.rs`. The `Heard` gap below
> was not in the way either: the words are read at the inbound point and never by `directive.v1`. **What is still
> open is the residue only** — the turn the yes started still runs, with the word in its input and nothing in that
> turn telling the model not to act on it. See `crates/theseus-core/AGENTS.md`'s "A yes said in words" entry for the
> mechanism as built.

The checklist line is bucket row `theseus-3i6d1` (1.0.5 B6), three sub-items under one heading. It stays open.

| Sub-item | State on this base |
|---|---|
| (1) abandoning follows `[policy.tools] task.close = notify` rather than a hard-coded ask | **landed** 2026-11-08: `abandon_follows_posture` is a parameter of `authority_for` (`task_graph/tools.rs:318`) and clears the plan's authority at `:323` |
| (1a) a consent row is not written for a non-consent event | **landed**: `&& said_yes` at `task_graph/tools.rs:905` |
| (2) a verbal yes in the same conversation counts as the answer | **not built** |
| (3) ask WHICH task when a bare yes is ambiguous | **not built** |

(2) and (3) are not built because four things about them are the owner's to decide, and the fourth widens what
counts as authority. The mechanism to build them on is otherwise entirely present, which is what this note sets out.

## The mechanism, as it stands

**The owner's recent messages are already held, with a lifetime.** `capture/expect.rs:14` is
`const LIFE: Duration = Duration::from_secs(60)`. `Expected::note` (`:37`) pushes `(session, text, Instant::now())`;
`Expected::take` (`:44`), documented *"Whether this message was noted; the note is spent."*, finds and removes it.
Both call `v.retain(|(_, _, at)| at.elapsed() < LIFE)` first (`:39`, `:46`), so expiry is lazy, on read, with no
timer. The store is in memory only — `Expected` is a field of `standing.rs:465` — so a restart forgets every note.
It holds **two** note families: `note`/`take` (`:37`, `:44`) for capture, and `note_owner`/`take_owner` (`:61`,
`:68`) kept apart because *"capture's note is made only while `[standing] capture` and the judge are on"*
(`:59-60`). Which family a words path would ride is part of the build, not settled here.

**The rule for whose words these are is written and pure.** `capture/mod.rs:66-68` is
`owner_alone(class, place)` = `class == PlaceClass::Private && place.is_none_or(|p| p.starts_with("discord:dm:"))`,
under the comment at `:63-65`: *"A guild channel bound private is not one: anyone it lets in writes there."*
Held by `capture/tests.rs:295-301`, five assertions, including that a `discord:channel:` place in a `Private`
class is refused. A session-id form already exists — `card/observe.rs:175` `owner_alone_in(&self, sid: &str)`,
reading `class_of(sid)` (`:176`) and `outbox.try_target(sid)` (`:177`) before calling `owner_alone` — and that is
the shape a words path needs, because an answer arrives carrying a session id, not a class and a place.

**The outstanding cards are readable one way.** `theseus-kernel/src/kernel.rs:1820` `pending_confirms(&self)`,
documented at `:1817-1819` as *"the one derivation of pending confirms (theseus-0g4)"*, a budget question first;
and its per-session form `rpc/confirms.rs:197` `pending_confirms(&self, session_id: &str)`.

**`directive.v1` rides the inbound batch, off the turn path.** `capture/mod.rs:4-5`: it *"rides the inbound request
(`judge::inbound`, batched with `classify.v1` and `role.v1`: no call of its own)"*; the pack name is `PACK` at
`:50`. `crates/theseus-core/AGENTS.md:587-592` carries the same rule and the never-a-task-session clause.

**There is already one answer verb, and it already has an ambiguity rule.** `rpc/answer.rs:1-13` is `work.answer`:
*"one answer verb for every question… An answer from any surface closes the question for all"*. `question_in`
(`:249`) resolves a partial id against the open questions, and when more than one matches it **refuses** rather
than guessing, with the count and the ids (`:266-267`): ``"`{given}` names {n} questions ({}): give more of
its id"``. `theseus-protocol/src/answer.rs:99` `approves(option)` is an option's yes/no. So question 3 below is not "invent an
ask": it is whether that existing refusal-with-a-list becomes a card or stays a message.

**Two existing rules disagree about a bound private channel.** Answering today does not go through
`capture::owner_alone`. `rpc/confirms.rs:759`'s default arm sends every answer through
`places::owner_in_private` (`places.rs:453`), whose refusal names its set (`:464-469`): *"it came from a shared
place, and only a private one counts (the CLI, the web UI, a DM with you, **or a channel bound private**)"* — and
whose doc at `:450-452` says the same. `capture::owner_alone` refuses exactly that last member. Side by side:

| Rule | Site | A guild channel bound private |
|---|---|---|
| `places::owner_in_private` | `places.rs:453`, used at `rpc/confirms.rs:759` | **counts** |
| `capture::owner_alone` | `capture/mod.rs:66-68` | **does not count** |

That disagreement is question 2's whole substance. Both are correct for what they currently guard; nothing here
says which governs a words path.

## What nothing does today, and what was searched to say it

**No path matches the owner's words to an outstanding confirm.** The searches, at `f4a37668`:

- `git grep -n -i 'confirm' -- crates/theseus-core/src/capture` → **3 lines**, all `Band::Confirm` in
  `capture/tests.rs` (`:87`, `:103`, `:191`). Those are judge bands, not cards.
- `git grep -n 'Expected\b' -- crates` → **9 lines**: `capture/expect.rs` (5), `standing.rs:465`,
  `crates/theseus-core/AGENTS.md:590`, and two in an unrelated Discord modal label
  (`theseus-discord/src/runtime/start_task.rs:493` and `theseus-discord/AGENTS.md:373`). Nothing outside
  `capture/` and `standing.rs` reaches the capture type.
- The strong form of the negative does not need a caller census. `git grep -n 'fn pending_confirms' -- crates`
  returns **exactly two definitions**: `kernel.rs:1820`, which takes no argument, and `rpc/confirms.rs:197`,
  which takes a session id. Neither accepts text, so no caller of either can select a card by what was said.

Widening each search by a layer corrected two enumerations this note was briefed from, without disturbing either
claim: the `Expected` search returns two Discord-modal hits beyond the one named, and the `pending_confirms`
caller list omitted several production callers (`rpc/routines_watch.rs:146`, `rpc/session_find.rs:103`,
`rpc/sessions.rs:112`, `rpc/methods.rs:626`, `:706`, `:1319`, `theseus-discord/src/runtime/grant.rs:48`). Both
lists were narrower than the claims resting on them, which is why the two-definitions form is written above.

## The four questions

Stated as questions. This note answers none of them.

1. *"May the owner's words answer **any** outstanding card, or only the `task.close` abandon kind?"*
2. *"From which surfaces — `capture::owner_alone`'s set only (CLI, web, an owner's DM), or also a guild channel
   bound private?"*
3. *"When a bare yes is ambiguous across more than one outstanding card, is the 'ask WHICH' itself a card, or a
   plain message?"*
4. *"Does a yes said in words set `said_yes`?"*

## Question 4 is the crux

`said_yes` is a **distinct parameter from `approved`**, and the distinction is documented rather than incidental.
`task_graph/tools.rs:650-654`: *"`approved`: the operator approved the call, which a layer-1 change needs;
`said_yes`: it was really asked and answered (an abandon that only follows a notify posture is approved, not
asked)."* It is destructured at `:660`, both are passed to `close` at `:665`, it is `close`'s parameter at `:857`,
and `:905` is the only thing it gates — `if i.outcome == Outcome::Abandoned && owners && said_yes {`, the write
of the `task.change_accepted` row. It is computed in exactly one place, `toolrun.rs:1476`,
`let said_yes = ran_at == Posture::Approve;`, and `:1477-1478` make `approved` the strictly wider of the two by
or-ing in `abandon_follows_posture()`. A grep for `said_yes` over all of `crates` returns those sites and the
existing test, and nothing else.

The `&& said_yes` was added precisely because **a consent record for a non-consent event is the wrong thing to
leave lying around.** So making words set it is an **authority widening and the owner's call, not a build
detail.**

## The `Heard` gap: a second reason no build can start

`capture/mod.rs:72-78` declares `Heard` with exactly `Nothing, Order, Preference, Fact, Correction`, and
`Heard::parse` (`:81-89`) maps exactly `"order" | "preference" | "fact" | "correction"`, else `Nothing`. **An
answer is none of those.** A build therefore needs a new `directive.v1` choice — a **pack change**, which is a
shadow-first judge item of its own with its own ladder. Designing that pack change is not this note's to do.

## The red tests a build would need

As prose, not as code; no test is added by this note. The existing consent test is the model and the neighbour:
`tests_task_graph.rs:604` `an_abandon_the_operator_said_yes_to_records_one_acceptance`, a `#[tokio::test]` on
`rig_on` that holds tasks, closes with `outcome: "abandoned"`, answers through `confirm_action` (`:628`) and
asserts `rows(c, "task.change_accepted").len() == 1` (`:634`) and `rows(c, "task.closed").len() == 1` (`:635`).
A build's tests would sit beside it.

1. **An answer within 60 s on an `owner_alone` surface clears exactly one named card and nothing else.** One card
   outstanding, the owner's message noted; the card the words name resolves. The bar is a **positive count**:
   exactly one `task.change_accepted`, exactly one `task.closed`, and the pending-confirm count down by exactly
   one. "No other card moved" alone does not meet it, because an empty set satisfies it.
2. **An answer from a bound guild channel clears none.** The same words in a `PlaceClass::Private` place whose
   target does not start with `discord:dm:` move nothing. Assert the pending count **unchanged and non-zero**, so
   an empty set cannot pass. Which way this test reads depends on question 2.
3. **Two outstanding cards and a bare yes clears neither, and asks.** Both cards still pending, and the asking
   happened. `rpc/answer.rs:267` already produces the refusal-with-a-list for the typed path, so the expected text
   depends on question 3 and **cannot be written until it is answered.**
4. **The 61st second clears none.** No real sleep, and no reading of the wall clock. `LIFE` is compared against
   `Instant::elapsed()` (`expect.rs:39`, `:46`), which is **not** the injectable `theseus_protocol::time` seam, so
   this case needs a seam in `Expected` or another way to drive expiry — and **adding that seam is part of the
   build, not of this note.** This is the one case whose mechanism does not yet exist.

## Adjacent, and not applied

The owner decided at 14:28 on 2026-11-09, about MCP memory: *"Let's go with 1 for now — it's more consistent with
our general stance. Indeed, Theseus is a meta agent in many ways, without true subagents or rigidly distinct
sessions."*

That stance is **adjacent** to question 2 and arguably argues against a per-surface withhold. It was a decision
about MCP memory, not about consent, and **this note does not read it across.** Flagged, and left unresolved.
