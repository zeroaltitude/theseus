# Cloud report: judge-tests (theseus-02vo, bhn2, daiz, fi5n)

## theseus-02vo: a reservation's sentences (f9c5bea, fix 5th commit)
- Found: the code is as the brief says; `reserve` is private, so the tests live in `judge/tests_reserve.rs`.
  One thing the brief did not say: a day's unsettled `in_flight` carries into the next day, so a resume test must
  `settle` the first reservation (a real call does) or the new day is refused too.
- Tests (5): the pause row and line once however many refusals; the resume row and line naming both days, once, and a
  fresh process says nothing; a META block reserved past settled books its rest, with row and line, once; a breaker
  opened by the fake `Down` (5 turns) says its line once; a shed report (max_in_flight 1, slow call) says its line.
  "Only after its frame" is not proved by order (the row and the line are written in one call; no store failure can be
  injected): the tests check that row and line agree and neither precedes the other's count.
- Plants (each restored, `touch`ed, `git status` clean): `said.iter().for_each(announce)` removed from `write_budget`:
  3 tests fail (`left: []`, `right: [the line]`); sink's circuit `announce` removed: breaker test fails ("never: the
  breaker's line"); shed `announce` removed: shed test fails ("never: the shed line").
- Load: the shed test failed 5/5 under load (the client's 1 s timeout freed the permit); fixed (total_secs 25 vs Slow 60).
  After the fix all 9 new tests passed 5/5 at nice 19 beside four busy loops (killed by pid).
- Cosmetic: the pause line at a 200-micro limit reads "today's $0.00 is spent".

## theseus-bhn2: slow Jev at compile (0e7716f, `tests_continue_slow.rs`)
- Test: continue.v1 alone on, dormancy 0; second turn spawned, wait until the fake has seen the continue.v1 request,
  await the turn's answer, then `calls_today == 0` and no `judge:continue` row; then wait on events for the row
  (a `timeout` failure). Margin: Jev 30 s, client `total_secs` 8 s, so the turn's rest must take under about 8 s.
  No new `FakeMode` was added (siblings may touch fake.rs).
- Plant: `rt.spawn(..)` -> `block_in_place(|| rt.block_on(..))`: fails at tests_continue_slow.rs:85 "the judgment had
  settled before the answer" (the turn took 8.3 s).

## theseus-daiz: links in a rerank's repack (e65d743, `tests_rerank_links.rs`)
- Test: two notes, newer linked `same_entity` to older, Jev puts the older first, `recall_max_items = 1`. Live: the
  request holds only the newer text and the manifest/row admit it. Shadow: `reranked_admitted` (and `fused_admitted`)
  are the newer alone.
- Plants: `links: &[]` in `Memory::refill`: the live test fails (request lacks the newer note). `links: &[]` in
  `Recalled::asker`: the shadow test fails (it also fails the live row assertion, so the live arm also uses it).
  Both restored and touched.

## theseus-fi5n: the class at inbound and compile (8b9681e)
- Rules and why. Compile: `loop_end::class(task, loop_index)`: task, else tools after the first loop (a later loop
  follows tool calls), else reply; `AtCompile` gains `task` (one line at its call). Inbound: the turn has not run, so
  a task's message is `task` and every other is `unknown`, written on purpose (`inbound::class`). Inbound runs only
  for a person's message, so wake/job_result turns never reach it. classify, role and route share the context.
- rerank.v1 (not changed): its recall sits at a loop's compile, so the same rule as compile fits (task / tools / reply);
  it needs the turn's task flag and loop index passed into `Recalled`. Left to the maintainer: rerank.rs/recall.rs are
  in soul-import's area.
- Test: `telemetry/tests_judge.rs::the_inbound_and_compile_judgments_carry_their_class_and_the_metrics_count_it`
  (rows' `context.class`: classify/role `unknown`, continue `reply`; metrics counted under them) and a unit test of
  `inbound::class`. Not covered by a whole-core test: a task's turn (`task`).
- Plants: class removed from compile's context: fails (`[Null]` vs `reply`); from inbound's: fails (`[Null, Null]` vs
  `unknown`). The metric count alone would not catch it (absent already reads `unknown`); the rows do.

## Gate (TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1) on the tree holding all four steps, before the commits
fmt, shape, features, clippy, cockpit, test build, reader rule passed; deny passed (run by hand, `--offline`, fetch ok);
protocol types clean (no diff in cockpit/src/protocol.gen). Suite failed only on: the L1 sandbox tests (theseus-sandbox
contract, theseusd sandbox; root VM, theseus-pv6i) and `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one`
(theseus-ynia; this clone lacks the fix). The commits were made while the gate ran (a stop hook asked for them), and the
5th commit's test-only change was checked by the 68 judge/rerank/continue/telemetry tests (all pass), not a second full gate.
Suites run: tests_judge, tests_judge_surfaces, tests_continue, tests_rerank*, telemetry judge tests, `judge::`: 68 passed.

## Live check for the maintainer
1. Scratch daemon, fresh state dir, `[judge.signals] dormancy_minutes = 0`; two messages a minute apart; then
   `theseus judge log --json -n 20`: classify/role/route rows show `context.class` `unknown` (`task` for a task's
   message); the continue.v1 row shows `reply` (`tools` if it came after a tool loop).
2. `narrative = true` and `[judge] shadow_limit_usd_per_day = 0.001`: a `narrative.watch` subscriber sees the pause
   line once; `theseus health`'s judge line reads paused.
