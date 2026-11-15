# Two notes for the core crate (issue 5, 1.0.7)

Notes for the core crate that live here, because `crates/theseus-core/AGENTS.md` takes no new prose (the owner's ruling of 2026-11-14 14:58).

## The capture pack is `directive.v2`

`capture::PACK` names `directive.v2`: `directive.v1` with the `none` and `order` answers reworded so that a request for one act,
done once, is no standing order. It is live from the day it landed (`judge::WIRED` makes the named pack live; no ladder step). v1
stays embedded, byte for byte, its judgments under its own text. The core guide's capture entry names v2. The rule for any new pack version is in `docs/development.md`, under Self-improvement.

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
