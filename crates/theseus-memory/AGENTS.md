# theseus-memory

Theseus's memory science (M6), as pure logic with no I/O: every input is explicit, so each result replays from the
record. It has no dependencies.

Key modules: `science.rs`, `recall.rs`, `rerank.rs`, `retention.rs`, `fsrs.rs`, `activation.rs`, `access.rs`. Read
by: theseus-core (`recall.rs`, the turn's recall step, `memory.search`, and the judge's rerank).

## What's here

- `science.rs` (step 30a): the `MemoryScience` trait (design §2.3: `gate`, `schedule`, `activate`, `decay_sweep`,
  `rank`, with `id` and `min_score`) and `Baseline`, its answers by §2.3's table. A science names its parameter set
  by digest (`baseline@<16 hex>`), and a recall's row says it. 30a reads `id`, `min_score`, and `rank`; the memory
  pass (31a), the retention and adjacency projections (32a, 32b), and tiering (33) read the rest.
- `recall.rs` (step 30a): the pipeline after the index. Each candidate is dropped by the first filter that takes it,
  in this order, with its reason: `place` (the place rule: a turn in a shared place draws only on that place's own
  sessions, a private place's on any session (theseus-1is6), a session whose place cannot be read only in a
  private place, and an open request (`Place::Open`, theseus-a1wvq.7) only an open session's nodes from before its
  taint, `may_draw_at`), `in_context`, `untrusted` (external text, unless `include_external`), `labeled_wrong` (the operator
  labeled it wrong or stale; 30b), `recursion` (a harness line or a recall), `arm` and `unchecked` (a synthesis
  under an arm whose science admits none, or one not checked: 31b's `MemoryScience::synthesis`), `threshold`;
  then the science's rank,
  and a greedy pack under the tokens and items (`budget`). A second chunk of an admitted node is `in_context`. The
  core reads each candidate's place (`TurnRunner::place_of`).
- `threads.rs` (theseus-f2g0): `ThreadLens`, what the threads do to a recall (the core builds it; `Asker.threads`).
  `judge(session, fused)`: a candidate of a held thread (sealed, or a seal proposed) is `sealed` for every asker but
  its own thread; in a threaded session (`distance`) another thread's candidate is `thread` unless its fused score is
  above `bar` of `top_fused` (the index's best), else it pays `penalty` in the pack's rank; naming the thread
  (`waived`) waives the distance and never a seal. Reasons `sealed` and `thread` join `Reason::ALL`.
  Tests: `tests_threads.rs` (a property over generated recalls: a sealed note is never admitted).
- `recall.rs`'s `PersonaBoost` (theseus-a8n7), in `Params.persona`: while a persona is engaged, a candidate whose session
  the core read as an imported episode of the persona's own agent ranks `boost` (1.0 to 2.0) times its score; the
  filters see the index's score, so the boost reorders and admits nothing. Tests: `tests_persona.rs`.
- `recall.rs`'s `CategoryBoost` (theseus-jrf4c), in `Params.category`: a candidate whose session shares a category with the
  asker's is worth `1 + gain * weight * (K+1)/(K+rank)`, rank from 1 among the sharers by score (K 60, the fusion's),
  `weight` the heaviest shared kind's (the core reads which). It runs after the place rule and every filter, so it
  reorders and admits nothing. Tests: `tests_category.rs`.
- `rerank.rs` (step 32c): the `+rerank` arm's pure half. `eligible` is what passed every filter, in the science's
  order (only it may reach Jev); `reorder` re-sorts the top 20 by Jev's probabilities (an unanswered item keeps its
  fused place; the rest follow in the fused order); `repack` runs `recall`'s filters and pack ranked by that order
  (a `Reranked` science). The core's rerank (`judge/rerank.rs`) calls all three, and since 32d a live
  rerank's turn packs again in Jev's order with `repack` too (`Memory::refill`), so the row and the request agree.
- `consolidate.rs` (step 31b): consolidation's pure half. `clusters` (pairs admitted together in 3 distinct turns,
  components of 3 to 8, none a synthesis or a recall, none synthesized before by digest), `check` (every sentence
  cites, every cited number is a source, at most 120 words, on `entry`'s text: a leading heading set aside, a
  Markdown or bold first line of at most 8 words, or a plain first line or sentence of at most 8 uncited words all of
  which its next sentence restates; a sentence that says something new is never set aside), and `score` (a
  synthesis scored as its best admitted source, ranked just ahead of it: rows keep no query). `science.rs`'s `WithSyntheses` is the `+synthesis` arm.
- `fsrs.rs` (32a's math), `access.rs` (what happened to a node, and the review it is: the operator's four labels,
  `should_have` graded Easy as `useful` is), `activation.rs` (32b's math).
- `retention.rs` (32a's wire-in): `RetentionRank`, the `+retention` arm's science: `baseline` in every verb but
  `schedule` (FSRS-6's step) and `rank`, `fused × ((1 − w) + w × R(now))` with `w` = 0.5; a node with no retention
  keeps its fused score. The form, the weight, FSRS-6's parameters and the baseline's line are in its digest
  (`retention@<16 hex>`). `RankCtx::retention` carries the candidates' retention by node, filled by the core
  (through `Asker::retention`) only for a science whose `reads_retention` says so; `Reranked` forwards it, so a
  repack in Jev's order keeps the arm's science.
- `activated.rs` (32b's wire-in): `Activated`, the `+activation` arm's science: `baseline` in every verb but
  `activate`, which spreads with its `SpreadParams`; and its numbers as a fusion source (`weight`, its term
  `weight / (60 + rank)` as the tender fuses, the 10 seeds, the 20 nodes it may add), all in its digest.

## Invariants

- **No I/O, no clock.** A time is an argument (`now_ms`).
- **The place filter is first, and total.** Every candidate a turn may not draw on is dropped for `place`, whatever
  else would drop it: the property tests in `recall.rs` and `rerank.rs`, and theseus-core's `tests_recall.rs` and
  `tests_rerank.rs`, hold it.

## Tests

- In each module. `recall::tests::the_place_rule_holds_for_every_pack` is the place property test over generated
  candidates; the core's `tests_recall::the_place_rule_holds_over_generated_stores` runs it through a whole core.
