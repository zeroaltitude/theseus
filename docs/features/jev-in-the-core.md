# Jev in the core: small, fast judgments steer every message

*One page on judgment in the loop. Numbers come from the data files beside each report they cite. Updated 2026-10-28.*

## What it is

Before the model answers a message, Theseus asks [TypeSafe](https://typesafe.ai)'s Jev a handful of small questions
with typed answers (a choice, a score, a yes or no): what kind of message this is, which model should answer it and
with how much effort, which persona speaks, which thread it belongs in, and what the message carries (open text, a
secret, something personal, an attempt at injection). Jev also orders the notes recall found, and during the turn it
judges each tool call for risk and the loop for whether it has reached its stopping point. So a thank-you goes to a
small model and a hard refactor to a large one at high effort, and the answer to "why did it do that?" is on record.

## How it works

- **Packs of questions, versioned.** Each judgment is a pack (`route.v8`, `classify.v4`, `rerank.v1`,
  `recall_cross.v1`, `content.v3`), asked over a typed state and answered with probabilities and a confidence
  (`crates/theseus-judge/src/pack.rs`, `builders/`). The spec is
  [§3.7 Judgments (Jev) as a core service](../spec/03-part1-s3.md#37-judgments-jev-as-a-core-service).
- **Beside the compile, never before it.** The route question starts as the message arrives; the turn compiles on
  the session's profile and waits at most `[routing] max_wait_ms` more (`crates/theseus-core/src/turn/route_step.rs`).
  A late verdict applies to the next message. Jev slow or down, its breaker open, a slash command or a pinned turn:
  no wait, and the turn keeps its own model and recall's own order.
- **Every judgment is recorded** with its pack, state hash, answers, probabilities, latency and the action taken
  (`crates/theseus-core/src/judge/`). `theseus why` says which model a turn ran on and why; `theseus judge log`
  lists the judgments; `theseus judge` shows their cost.
- **The learning loop.** Your labels on a judgment ("should have asked", "not this one", a removed content label)
  go to the learning ledger. Each night it reports against frozen holdouts, and a pack version's wording is
  rewritten from them, nightly, under a holdout gate ([§3.10](../spec/04-part1-s3.10.md#310-learning-ledger-and-the-learning-channel)).
  Since 1.0.3's first rows, every content judgment the daemon ever made replays, so a reworded classifier is
  measured against the owner's real traffic before it ships.

## What you see on each surface

- **Terminal:** `theseus why` and `theseus why --short`; `theseus judge log`; `theseus content SESSION` lists the
  labels Jev raised and `theseus content remove` takes one away.
- **Discord:** a label on a message shows as a chip you can tap away; a routed task says which model it runs on.
- **Cockpit:** the Judgment page, and a turn's route in its session view.

## The proof

- **On Terminal-Bench 2.0 at high effort** ([the report](../benchmarks/2026-10-26-terminal-bench-theseus-1.0.3-high-k3.md),
  2026-10-26, 89 tasks × 3): Jev ran 41 of 267 trials entirely on Haiku 5.5, 151 on Sonnet 5.5 alone, and the run
  solved 209 of 267, 78.3%.
- **The classifier's new version** (landing this week): replayed against the owner's own 28 recorded corrections,
  `content.v3` scores precision 0.956 against `content.v2`'s 0.875, with recall held.
- **Tests** drive each judgment with a scripted Jev: a hard question goes to Opus, a quick one detours to Haiku
  without changing a byte of the next cached prompt, a failing Jev leaves the request as it was, and Jev's order of
  recall is the order in the request (`crates/theseus-core/src/tests_route.rs`, `tests_rerank_live.rs`,
  `crates/theseus-judge/src/tests_route_v5.rs`).

## The limits today

- **It sometimes calls a hard task routine.** On the 30 hardest tasks at top effort every turn it called routine
  failed. Since v1 patch1 a routed turn whose loops keep failing asks Jev again and moves up.
- **A refusal costs money.** On `stop_reason == "refusal"` the turn switches to a fallback model and keeps the rest
  of the turn there (`crates/theseus-core/src/turn/fallback_step.rs`). On the 2026-10-27 MAX run 35 of 90 trials
  hit it, and 71% of the cell's spend came after the switch. The fix is in 1.0.4.
- The persona pick runs in shadow: it is recorded, and the persona that answered last keeps answering.
- Each judgment is about a tenth of a second and a small fraction of a cent; a turn makes several.

## What is coming

- **1.0.3 (this week):** `content.v3`, and a content verdict that raises a label you can remove rather than a
  refusal ([It trusts its environment](trusted-environment.md)).
- **1.0.4:** the refusal fallback fixed at its cause, measured on the same 30 tasks.

Related: [Context compiled for each turn](compiled-context.md) (where Jev's recall order goes),
[Many characters, one memory](personas-and-threads.md) (the persona and thread questions).
