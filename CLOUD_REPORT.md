# Cloud report: route-tests (theseus-biy3, theseus-zvpl, theseus-490i, theseus-3urn)

Branch `cloud/20261005-route-tests`, cut from main at 4a44946 (store format 22; no format change here). The
session started at 03:06 UTC and ended at about 06:25, inside its 5-hour deadline. Four steps, each one commit
that passed the gate before it was pushed:

| step | commit | what |
|---|---|---|
| theseus-biy3 | 69b4b80 | route.v1's tests made load-proof |
| theseus-zvpl | 26be535 | tests for a second switch's base and a switch back to it |
| theseus-490i | 676a0b4 | a routed turn's trace root and metrics name the model it ran on |
| theseus-3urn | 4ab3132 | a switched turn's compilation keeps its recall drops |

The load recipe for every loaded run below: the test under `nice -n 19`, beside four nice-0
`sh -c 'while :; do :; done'` loops on this 4-core VM, each killed by its pid. Lesson: build first at normal
priority (`cargo nextest run --workspace --no-run`). My first loaded run of step 2 rebuilt the workspace at nice 19
under the loops, hit the 30-minute background limit and was stopped. Its loops died with it, and I checked that no
process was left. I reran it after a normal build.

## Where the code differs from the brief

- `route_to` is the one place a turn moves, for a switch and a detour alike. Before 490i nothing there touched the
  trace, as the brief says.
- The modes: `sophisticated` is opus then fable, `deep_coding` opus then sonnet, `routine_coding` glm53 then glm,
  and `trivial` detours to `cheapest` (glm, GLM 5.3 Flash, in the rig). `chat` lists nothing, so it keeps the
  session's own. `decide` picks against the turn's current profile (`t.target.profile`, already moved by
  `route_base`). So a session already on Opus is switched a second time by `routine_coding`, which picks glm53. A
  pane whose base is glm53 is switched back to it by the same verdict, so the tests use `-P glm53`. The brief's
  `pane_moved_to_opus` has base `glm`, which no mode lists first.
- The rig's `[judge] total_secs` is 1 (`tests_judge::judge_config`). The old `Slow(3 s)` case of the failing-Jev test
  was `late` only while the first compile took less than the 1 s client limit. Under load the call failed first and
  the turn read `no_verdict`. The late tests now raise the limit to 30 s.
- A failed turn's `theseus.turns` point is not labelled from the trace root. `rpc/methods.rs`'s `count_failed_turn`
  takes the profile, provider and model the client's request resolved before routing. So a routed turn that
  **fails** is still counted under its base model. I left this alone: it is the client path, outside route's
  files. See "Left" below.

## theseus-biy3: route's tests made load-proof (69b4b80)

**Found.** On main, the four route test files (33 tests) failed 16 times in 5 loaded runs
(`/tmp/w/before_*.log`):

| run | failed |
|---|---|
| 1 | `a_late_verdict_applies_to_the_next_message_alone`, `a_late_verdict_applies_from_the_next_message`, `a_late_trivial_verdict_never_applies_to_the_next_message` |
| 2 | the same three |
| 3 | the same three |
| 4 | the same three, and `a_failing_jev_or_the_judge_off_leaves_the_request_unrouted` (`Slow(3s): 2.111008339s`, its 2 s bound) |
| 5 | the same three |

Each late test failed the way the brief predicted on this machine. The first compile ran past 300 ms, so the
`Slow(500 ms)` verdict landed inside the 200 ms wait. The late tests then read `("opus", "verdict")`,
`("glm", "detour")` or `"verdict"` where `("sonnet", "late")` or `"late"` was wanted. In one run the second turn's
own verdict came in time, so `d[1]["judgment"]` named a different judgment than the late one. On 4 cores the other
direction, a verdict a test wants in time landing late, never showed in these runs. The 5 s wait still covers it.

**Changed.**
- `tests_route::rig_on` sets `[routing] max_wait_ms = 5_000` before the test's tweak, as tests_route_base's rig
  already did. A verdict ends the wait as it lands, so a quick one costs nothing. Every test whose subject is not
  the wait gets it.
- Lateness comes from the order of events, not from a delay. **A small hunk in theseus-judge's `fake.rs`:** a
  `FakeMode::Held` variant at the enum's end. Each call's answer waits for the next `FakeJev::release()` after it
  arrived, then answers as `Up` would. A `releases` counter sits in `Shared`, and the serving thread polls it every
  5 ms. That thread is a std thread, never a runtime worker. judge-tests may add a mode too, so a merge may have to
  keep both variants at the enum's end. The doc table gained a row. `cargo fmt` also wrapped `RateLimited`'s fields,
  because the enum now has a documented variant.
- The late tests use `held` (`max_wait_ms = 200`, kept short because a held verdict is late at any wait;
  `total_secs = 30`). They release the verdict after the turn that missed it, then wait for it to land with
  `until_late`, which uses a test-only `JudgeService::has_late` in `judge/inbound.rs`. This replaces the fixed
  800 ms sleeps. Each later held turn is released after it ends, so no fake thread is left waiting.
  `a_late_verdict_applies_to_the_next_message_alone` keeps the 5 s wait, because its second turn needs a verdict in
  time. Its two held turns each wait out the 5 s.
- `a_failing_jev_or_the_judge_off_leaves_the_request_unrouted` no longer has its 2 s wall bound. Under the 5 s
  wait, a `no_verdict` from Down, RateLimited or Malformed shows that the call's failure ended the wait: had it not,
  the turn would read `late`. Its `Slow(3 s)` case is now the held case. A bound would prove only that the failure
  came within the wait, which the reason already says.

**Proved.**
- After the change: the same 33 tests, 10 loaded runs, **0 failures** (`/tmp/w/after_*.log`). The 33 tests took 64
  to 68 s per run (nextest's summary), against 67 to 70 s before. Slowest: `a_routed_panes_p_profile_comes_back`
  (mean 46 s), `routing_off_shadow_…` (23 s), `a_late_verdict_applies_to_the_next_message_alone` (16.5 s, about 15 s
  on main), `a_failing_jev_…` (13 s).
- Unloaded: 33 of 33 passed.
- In the gate's suite, theseus-judge's own tests passed with the new mode.

## theseus-zvpl: a second switch's base, and a switch back (26be535)

**Found.** As the brief says: `route_to` keeps the first base with `r.from.get_or_insert(base)` and ends the move on
a switch back. Nothing held either behaviour.

**Changed.** Tests only, in tests_route_base.rs on its rig (helper `switched_twice`):
- `a_second_switch_keeps_the_first_base_and_a_changed_live_profile_clears_it`: a session that follows the live
  profile is moved to Opus by `sophisticated`, then to glm53 by `routine_coding`. `routed` is `{glm53, from
  sonnet}`. Its next turn has no verdict (`Malformed`: `no_verdict`) and runs on glm53. After a restart with
  `[model] live = "fable"` it runs on Fable, and `routed` is `None`.
- `after_two_switches_routing_off_runs_on_the_first_base`: the same session, restarted with routing off, runs on
  Sonnet, not on Opus.
- `a_switch_back_to_the_base_ends_the_move`: a pane on `-P glm53` is moved to Opus, then switched back by
  `routine_coding`. `routed` is `None`, and `last_target` is glm53. Restarted with routing off, the pane, carrying
  glm53, runs on glm53.

**Proved.**
- Planted `r.from.insert(base)` for `r.from.get_or_insert(base)`. Over all 36 route tests, exactly the three new ones
  failed:
  - `a_switch_back_to_the_base_ends_the_move`: `left: Some(Routed { profile: Some("glm53"), from: Some("opus"), hold: None })`, `right: None`
  - both two-switch tests, at `switched_twice`: `left: Some(Routed { profile: Some("glm53"), from: Some("opus"), … })`, `right: … from: Some("sonnet") …`
- Restored, with `touch` and a clean `git status`: all pass. Each new test passed 5 of 5 loaded runs.

## theseus-490i: a routed turn's model (676a0b4)

**Found.** As the brief says. `Metrics::turn` takes `gen_ai.request.model` from the trace root, which `Turn::start`
fills from the target before routing. The root's readers, found with a grep over the workspace and the cockpit:
`Metrics::turn` (`gen_ai.request.model`), and the exporter, which flattens the root's attrs onto the exported
turn span. The CLI, the TUI, the cockpit and the bench read no root `profile`, `provider` or `model`.

**Changed.**
- `Trace::set_root(attrs)` merges into the root under open spans, and has a unit test in `trace::tests`.
- `route_to` sets the root's `profile`, `provider` and `model` to the routed target's, for a switch and a detour
  alike, and the module doc says so. What changes for readers:
  - `theseus.turns`, `theseus.turn.duration_ms`, the token and cost points and the tool-call points now carry the
    routed model, and they already carried the routed profile.
  - The exported root span now agrees with its provider call span.
  - The model a fallback (theseus-7gir.18) answers with is unchanged: it is the provider span's `served_model`.
- `tests_route::rig_parts` builds the route rig with its `Parts` edited (`rig_on` and the new helper share
  `rig_built`), so a test can give it telemetry's pipeline.

**Test.** `tests_route_model.rs`, with its `mod` line after `tests_route_rows`'s in lib.rs:
`a_routed_turns_metrics_name_the_model_it_ran_on`. It sends two turns through `turn.submit`, with
`Telemetry::from_config` pointed at telemetry's `Receiver`. A hard question switches Sonnet to Opus, and a thanks
detours Sonnet to GLM 5.3 Flash. Each turn's root names the routed profile, provider and model, and the flushed
metrics hold exactly two `theseus.turns` points: `opus` with `anthropic`/`claude-opus-5-5`, and `glm` with
`zai`/`glm-5.3-flash`. I used none of telemetry/tests.rs's private helpers, and that file is unchanged.

**Proved.**
- Planted: the `set_root` call removed from `route_to`. The test fails at the root:
  `left: ("sonnet", "anthropic", "claude-sonnet-5-5")`, `right: ("opus", "anthropic", "claude-opus-5-5")`.
- Restored and touched: passes. 5 of 5 loaded runs green, with `trace::tests`.
- The gate's suite ran every telemetry test green.

## theseus-3urn: a switched turn's recall drops (4ab3132)

**Found.** As the brief says. On a switch, `recall_compiled` ran in the discarded first compile and
`std::mem::take` took `t.recall.drops`, so the stored compilation's budget named no recall drop.

**Changed.**
- While `t.route.defer_persist` holds, `recall_compiled` clones the drops. Otherwise it takes them, as before. A
  switch's compile then takes them.
- `keep_first` clears them once the first compile is the one used. Without the clear, a later loop's new
  compilation in the same turn would add them a second time.

**Test.** `tests_route::a_switched_turns_compilation_names_its_recall_drops`. Recall is live, `recall_budget_tokens`
is 30, and the notes session holds two heron notes of about 25 tokens each, so the pack keeps one and drops the
other for its budget. A switch to Opus and a kept first compile (chat, on Sonnet) each store a compilation whose
`budget.dropped` names exactly one recall node, one of the two notes. `a_routed_turn_still_writes_and_sends_its_recall`
is unchanged and green.

**Proved.**
- Planted: the first compile takes the drops again (`true => std::mem::take(...)`). The switch's case fails at the
  `dropped.len()` assertion (`left: 0`, `right: 1`). The 37 other route tests pass.
- Restored and touched: passes. 5 of 5 loaded runs green, with `a_routed_turn_still_writes_and_sends_its_recall`.

**The second compaction case: not added.** Reaching a ring's cut on the routed model needs a context near its
window. Every catalog model has a window of at least 200,000 tokens, and above `cold_switch_tokens` (30,000 by
default) a switch waits for a second agreeing turn. The rig's scripted short turns reach neither. A test would need
a catalog override that gives the routed profile a window of a few thousand tokens, then a session grown past it.
That is doable, but I left it as a follow-up rather than half a step.

## Every route test, final head (4ab3132)

The final proof covers 51 tests: `tests_route*` (now five files), `routing::`, and `turn::route_step::tests`. They
ran 10 times under load: **51 of 51 passed in every run**, 69 to 74 s each by nextest's summary
(`/tmp/w/final_*.log`).

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before each commit. fmt, shape, features, clippy,
the cockpit, the test build and the reader rule passed every time. The suite failed only on the known L1 set, so I
then ran the phases after it myself, each time: protocol types ok; the turn bench's frames, `frames_plain` 5 of 5
and `frames_tool` 9 of 9, ok; deny ok (advisories, bans, licenses, sources).

| gate | suite | failures |
|---|---|---|
| 1 (biy3) | 2,830 tests | 34: the 33 known L1 tests (theseus-sandbox's contract clauses and `spawn_100`, theseusd's `sandbox` tests), plus `theseusd::bench_profile theseusd_check_passes_on_the_bench_profile_with_no_vault`. Its `check` printed `L1: the self-test failed: checking the job: a workspace root, /, is the root`, which is L1's refusal on this root VM. It passed in the three gates after, and my change does not touch it. |
| 2 (zvpl) | 2,833 tests | the 33 known L1 tests |
| 3 (490i) | 2,835 tests | the 33 known L1 tests |
| 4 (3urn) | 2,836 tests | the 33 known L1 tests |

No timing test from the brief's list failed in any gate. No negative assertion failed. The lifecycle, jobs and
L1-row benches are skipped under `THESEUS_GATE_NO_BENCH`.

## The live check (the maintainer's)

On a scratch daemon of this build, with a fresh state dir. Discord and the web are off. The model is the stand-in
`theseus-sim fake-model`. The judge is on with route.v1 live. Telemetry's `otlp_endpoint` is a small
`python3 -m http.server`-style handler that saves each POST's body to a file.

1. **The routed model in telemetry (490i).**
   - Run `theseus --socket S ask -P haiku "Weigh two designs for a crash-safe write-ahead log."`.
   - If route.v1 answers `sophisticated`, the turn runs on Opus, and the saved `/v1/traces` body's root span carries
     `model = claude-opus-5-5`. The last `/v1/metrics` body's `theseus.turns` point has `theseus.profile = opus`
     and `gen_ai.request.model = claude-opus-5-5`. `-P` is a pin and is recorded in shadow, so to see a switch, send
     the question without `-P` on a haiku live profile, or in a pane that carries haiku.
   - Then a greeting (`"thanks!"`) on an Opus session that detours. Its root and its point name the detour
     profile's model.
2. **Two switches, then routing off (zvpl).**
   - A session moved twice (a hard question, then "Rename these twelve call sites the same way.").
     The session's record in the scratch store shows `routed` `{profile: glm53, from: <live profile>}`. I found no
     CLI command that prints `routed`, so read the record.
   - Restart with `[routing] enabled = false`. The next message runs on the live profile it started on.
   - A pane started with `-P glm53`, moved to Opus, then switched back: its record has no `routed`.
3. **The recall drop (3urn).**
   - Set `[memory] mode = "live"` and `recall_budget_tokens = 30`, and seed two notes of about 25 tokens each.
   - A hard question that switches to Opus: the session's stored compilation (its `compilation_id`, read from the
     scratch store) has `budget.dropped` with one `tier: "recall"` entry naming the note recall left out. Its
     `recall.ran` row's manifest names the same drop.

## Left or uncertain

- **A failed routed turn is still counted under its base model.** `count_failed_turn` gets the client's pre-route
  target. The fix would pass the routed target out of `TurnError`, which means adding fields to it, or reading the
  failed trace's root as `Metrics::turn` does. Both are outside route's files. I'm naming it for the owner rather
  than widening this change.
- **The `keep_first` clear is untested by a two-loop turn.** The rig's turns are one loop each. A routed turn with a
  tool call, whose second loop compiles a new compilation, would hold it.
- **The fake's `Held` mode** is one more mode in theseus-judge's fake. If judge-tests adds its own, the two variants
  need to sit together at the enum's end at the merge.
- **Docs** (the maintainer's to write):
  - theseus-core's AGENTS.md, Routing bullet: the trace root follows a route (490i); a switched compile's budget
    keeps recall's drops (3urn); the route rig's 5 s wait and the `Held` fake for lateness (biy3).
  - Its Tests section could name `tests_route_model.rs`.
  - Part III's item for this batch.
