# Three notes for the core crate (issue 5, 1.0.7)

Notes for the core crate that live here, because `crates/theseus-core/AGENTS.md` takes no new prose (the owner's ruling of 2026-11-14 14:58).

## The capture pack is `directive.v2`

`capture::PACK` names `directive.v2`: `directive.v1` with the `none` and `order` answers reworded so that a request for one act,
done once, is no standing order. It is live from the day it landed (`judge::WIRED` makes the named pack live; no ladder step). v1
stays embedded, byte for byte, its judgments under its own text. The core guide's capture entry names v2. The rule for any new pack version is in `docs/development.md`, under Self-improvement.

## The route pack is `route.v11`

Since issue 46 the acting route pack is `route.v12`, and `route.v11` is retired: see
[`retired-judge-questions.md`](retired-judge-questions.md), "Issue 46". What follows is v11's own record.

`inbound::ROUTE_PACK` names `route.v11`: `route.v10` word for word, with the `fable` option's description saying Fable 5.2 at its
catalog prices ($8 in, $40 out, $0.20 a cache read per million) where v10 said Fable 5.1. The owner chose it ("1 A; 2 A", 2026-11-14
17:55 MST): a new version, live when it landed, v10 and its golden untouched. `judge::WIRED` takes the constant, and the ladder
adopts v11 beside v10's row (`ladder::adopt`, `WHY_ROUTE_V11`). Its golden request is v10's with the pack's name and that one
description. The labelled cases are written for v10 and read for v11 without a word changed (`route_cases::read_as`).
`routing::tests_model::the_packs_fable_description_names_the_model_the_option_runs_at_its_catalog_prices` holds the description to
the router's option and the catalog row, so the pack and the router cannot name two models again.

**What does not carry over from v10.** A pack's standing on the ladder, a rollback, a `[judge.packs."route.v10"]` key and any
learned rewrite stand on a version's own name. On a live daemon, check `theseus packs` (it lists the wired packs and learned ones: v11 `live`, and v10 no longer) and the config
file for a `[judge.packs."route.v10"]` table, which now does nothing: rename it to `route.v11`. Rows the ledger holds under
`route.v10` stay readable under that name.

## A cut call booked from the provider's own usage events (theseus-k48c)

The core guide's `/stop` entry says the cut is booked from the provider's usage events when the stream had sent one; in detail: when the stream had already sent a usage event
(`Delta::Usage`: the prompt at `message_start`, the output at a `message_delta`), `settle_cut` (`turn/settle.rs`) books the call from
the last one instead: the prompt exactly, with the cache read and write as the provider split them, and the output as the larger of
what it reported and the characters streamed (`split: provider_events`). The `provider.cut` row's `estimated` is false only when the
reported output was not the smaller. A cut before any event, and a stream of another shape than Anthropic's (OpenAI's emits no
`Usage`), stay the estimate the guide describes. Tests: `tests_money` (a cut that streamed nothing; one that streamed more than it
was told), `provider::tests`, and the post-mortem line's two readings (`postmortem_facts_tests`).

## The effort pack is told how the last turn ran (theseus-8euum)

`effort.v1`'s state carries `previous_turn: {effort, failed}` when the session's previous turn is known (`routing/effort/previous.rs`
reads the previous `route.decided` row and a `turn.failed` row of the same turn; the row carries `class`, and `is_attempt` leaves out
the refusals no effort could address). Nothing post-processes an effort answer: the owner ruled "tell it, don't force it" (2026-11-14
13:54). Proof: `tests_effort_memory` and `theseus_judge`'s `the_effort_state_carries_the_previous_turn_and_only_when_there_is_one`.
