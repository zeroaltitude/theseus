# CLOUD_REPORT: route.v1's verdict without a wait on every message (theseus-ddbi)

Branch `cloud/20261006-route-wait`, from `795e9345` (the task commit on `f589d9cb`). This session resumed the
task after an earlier session stopped at a usage limit; that session's step 1 (`10a2fd8f`) was pushed and is kept
as it was. Its step 2 was never committed, so this session redid it from scratch.

| Step | Commit | What |
|---|---|---|
| 1 | `10a2fd8f` (earlier session) | route.v1 asks in a request of its own, beside the inbound batch: way (b) |
| 2 | `1c68f7d9` | Jev's connections opened after serving and kept warm |
| 3 | `e4577bfd` | `route.decided` gains `late` and `answered_ms`; the metric `theseus.route.wait` |
| 4 | `3d1df6ca` | the default bound's table (a measurement test, run by name) |
| 2's test | `7c94e983` | the first-message test drops a wall-time bound that failed under load |

## The earlier session's "33 failures were the known L1 ones": not quite

I checked that claim on my own gate run. My first gate after step 2 had **34** distinct failures:
- 33 were the known L1 failures: theseus-sandbox's contract tests (19), its bench's `spawn_100`, and theseusd's
  sandbox tests (13);
- 1 was mine: `theseusd::judge a_start_with_the_judge_on_builds_nothing_of_it`. It expected the judge's breaker
  `idle` (client never built) after serving. The keeper now builds the client after serving, so health says `closed`.

I don't know what the earlier session's lost step 2 did, so I can't say whether its 33 hid the same failure. I fixed
it in step 2's commit (below). Every gate since has failed only on the 33 L1 tests.

## Step 1: the verdict sooner (way (b), `10a2fd8f`, earlier session)

**What it found.** Jev's client (`theseus-judge/src/client.rs`) reads the answer as one JSON body:
`post` reads the whole body, then `parse_response`. Jev's API answers one JSON object, not a stream, so way (a)
(take route.v1's answer as it streams) would need Jev to stream, not just our client. reqwest's `stream` feature is
on, but there is nothing to stream. So the cheaper way is (b): route.v1 asks alone, in its own request, beside the
batch of `classify.v1` and `role.v1`.

**What it changed.** `judge/inbound.rs`:
- the two requests go out at once;
- the verdict goes to the turn the moment route.v1's own request answers;
- each request is reserved as it goes out;
- each judgment carries its own request's call and cost (the batch's split between its two by question count);
- the batch waits for a permit only when one of its packs acts.

The fake Jev gained `set_latency(single, batch)` and `keep_alive(handshake)`/`opened()`.

**Cost of the second request** (measured with the fake's pricing, `JevPrice::jev_1_13_0`, $0.042 per million
tokens in and out). For the test message "Weigh two designs for the log.":
- route.v1's request: 73 tokens in (33 of state, 40 for its question), 30 out: 5 micro-dollars;
- the batch (6 questions): 438 in, 180 out: 26 micro-dollars.

The fake bills each question its state, so the split adds **0 tokens** under the fake's model. If real Jev bills
the state once per request, the split adds the state's tokens once per message. That is 33 tokens here; a long last
reply in the state could make it a few thousand. At $0.042/Mtok, 1,000 extra tokens is $0.000042 a message, about
$0.004 per 100 messages. The live check below reads the real figure from `judge.call` rows.

**Proof (rerun here).** `tests_route_wait::route_asks_alone_beside_the_batch_and_each_judgment_is_recorded_once`
(both requests' judgments recorded once, costs split correctly) and
`the_verdict_reaches_the_turn_before_the_batch_answers` (batch 3 s, the turn routes on the verdict with no wait
for it). **Planted revert (mine):** I sent the verdict only after `join` of both requests (the whole batch ends).
`the_verdict_reaches_the_turn_before_the_batch_answers` failed: `wait_ms` 2995, `answered_ms` 3006. Restored,
`touch`ed, and `git status` was clean.

## Step 2: no first-message handshake (`1c68f7d9`)

**What I found.** The client's only warm-up (`warm_on_message`, theseus-otny) runs as a message arrives. It runs
beside route.v1's request, so a fresh daemon's first request still paid DNS, TCP and TLS. And reqwest's pool
(`POOL_IDLE`, 180 s) closes idle connections, so the first message after three quiet minutes paid them again.

**What I changed.**
- `crates/theseus-core/src/judge/warm.rs` (new): `JudgeService::keep_warm(every)` and `Core::warm_judge()`. They
  act only when the judge is on and an inbound pack or the rerank is on. They build the client, open two
  connections (route.v1's request and the batch go out at once) with `HEAD`s of the judge's path (no key, nothing
  billed), and spawn a keeper. The keeper sleeps on tokio's timer and sends the `HEAD`s again after each `KEEP_WARM`
  (150 s, half a minute inside the pool's 180 s) of Jev's silence. Any answer of Jev's, a call's included, resets
  its clock, so a conversation sends no warm-up at all. It holds the service by `Weak` and ends with it.
- theseusd's `after_serving` calls `core.warm_judge()` beside `warm_ladder`. Nothing new is on the start path.
- `JevClient::refresh(n)` (a warm-up whether or not the client is warm; `warm_up` is now `if warm { None } else {
  refresh }`), `JevClient::heard_ago()`, and `client::KEEP_WARM`.
- `theseusd/tests/judge.rs`, `a_start_with_the_judge_on_builds_nothing_of_it`: after serving it now waits for and
  expects the breaker `closed` (client built). The rest is unchanged: no `judge.call`, the adoptions. A warm-up to
  an unreachable Jev moves no breaker. The test's name still says "builds nothing", which stays true of the start
  path. The maintainer may want to rename it.
- `judge/mod.rs`: one line, `pub mod warm;` (judge-sink's file, in review; the hunk is that line only).

**Tests** (`tests_route_wait.rs`):
- `jevs_connections_open_after_serving_and_a_first_message_pays_no_setup`: under a 1.5 s set-up, `Core::build` (all
  of the start path) leaves `opened`, `warmups` and `connections` at 0. `warm_judge` opens 2. The first message
  routes with `wait_ms` < 1000 and opens no new connection.
- `the_keeper_uses_its_connections_again_while_jev_is_silent`: with a 300 ms period, warm-ups keep coming on the
  same 2 connections, and nothing is billed.

**Planted reverts:**
- `core.warm_judge()` moved into `Core::build`, before serving: the first test failed, `nothing reaches Jev before
  serving`, left `(2, 2, 0)`.
- The keeper calling `warm_up` (which skips a warm client) instead of `refresh`: the keeper test failed (timed out
  waiting for three refreshes).

Both restored and `touch`ed; `git status` clean after each.

**The start path, unchanged.** `target/debug/theseus-sim bench lifecycle --runs 10 --check` after the change, two
runs, LIFECYCLE OK both:
- cold start to the first health answer: p50 17.0 ms, p95 20.2 ms (budget 50);
- SIGKILL then restart: p50 16.8 ms, p95 21.8 ms (budget 150);
- clean shutdown: p95 8.9 ms (budget 100);
- the daemon's own clock over 51 starts, p50/p95: core 0.98/1.32 ms, socket 0.06/0.35 ms, serving 7.68/10.16 ms
  (first run: serving 7.93/11.81).

I did not build the pre-change binaries for a before run. The bench's config has the judge off, and `keep_warm`
returns at its first line when `[judge] enabled` is false. With the judge on, the call is in `after_serving` only,
and the planted revert above proves the test catches a move onto the start path. The join's gate on the owner's
machine runs the lifecycle bench with its history.

## Step 3: visibility (`e4577bfd`)

**What I changed.**
- `route.decided` (`fact/route.rs`) keeps `wait_ms` and gains two fields:
  - `late` (bool): on a turn route.v1 acts on (live or canary), the verdict had not come by the decision. Either the
    wait ended at `max_wait_ms`, or there was no wait because Jev was known unreachable. `reason` still says which.
    In shadow or under a pin it is false.
  - `answered_ms` (number or null): when route.v1's request came back, in ms after the turn's start, if it had by
    the decision. A request that came back with no verdict (a failure) also counts. A late answer arrives after
    the row is written, so it is null.
- The oneshot carries `inbound::Answered { verdict, at }`. `route_step::beside` returns a `Waited { got, waited,
  answered }`. The `route_mode` hunk is untouched. I edited `beside`, its call site in `compile_first`, and two
  small hunks it feeds: `read_verdict`'s late-verdict spawn (it reads `Answered`) and `record_route`'s fields.
- The metric `theseus.route.wait`: a histogram in ms with the attribute `theseus.route.late` (bool). It is recorded
  for each turn route.v1 acts on (live or canary), not shadow, so shadow's zeros don't dilute it. It follows the
  judge's pattern: the `ROUTE_WAIT` Instrument, `Metrics::route_wait`, `Telemetry::record_route_wait`, called
  through `JudgeService::record_route_wait`.
- No store format bump: `route.decided` is a JSON ledger row, and no stored record's layout changed. No protocol
  type changed, so `protocol.gen` is untouched.

**Tests** (`tests_route_late.rs`):
- `a_verdict_in_time_routes_the_turn_and_costs_no_wait_past_its_answer`: `late` false, `answered_ms` set, `wait_ms`
  ≤ `answered_ms`.
- `a_late_verdict_never_holds_the_turn_past_its_bound_and_is_recorded_late`: fake `Held`, bound 300. `wait_ms` is in
  300..1300, `late` true, `answered_ms` null, the turn stays on sonnet. The released verdict routes the next message
  to opus.
- `the_route_wait_metric_counts_each_routed_turn_by_late`: one in-time turn and one late turn, read back from the
  OTLP receiver. Two histogram points, one with late=false and one with late=true; the late one's sum is ≥ 300.
- `route_step::tests` (paused clock) now also check the instant the verdict came.

**Planted revert:** `let late = false;`. Both `a_late_verdict_…` and `the_route_wait_metric_…` failed. Restored,
`touch`ed, `git status` clean.

## Step 4: the default bound (`3d1df6ca`)

`tests_route_measure.rs`, ignored, run by name:

`TZ=America/Phoenix cargo nextest run -p theseus-core --run-ignored only --no-capture --test-threads 1 -E 'test(/tests_route_measure/)'`

How the runs are set up:
- one fresh rig a cell, `max_wait_ms` 200, 8 messages a cell, each sent once the last one's requests are answered;
- the fake Jev with keep-alive connections and a 300 ms set-up per new connection;
- "single" is route.v1's request's latency, "batch" the other request's;
- rows other than "cold" are warm (`warm_judge` first);
- the rig's compile is a few ms (fake providers), so the wait past the compile is close to the single latency;
- "turn" is the turn's wall time with a fake model that answers at once.

| fake Jev (ms) | wait mean ms | wait p50 | wait p95 | in time | turn p50 ms | turn p95 |
|---|---|---|---|---|---|---|
| single 80, batch 400 | 80 | 75 | 116 | 8/8 | 100 | 153 |
| single 150, batch 400 | 149 | 145 | 184 | 8/8 | 170 | 221 |
| single 300, batch 400 | 200 | 200 | 201 | 0/8 | 228 | 240 |
| single 600, batch 400 | 201 | 201 | 201 | 0/8 | 227 | 236 |
| single 80, batch 1000 | 80 | 75 | 114 | 8/8 | 99 | 150 |
| single 150, batch 1000 | 150 | 145 | 186 | 8/8 | 169 | 224 |
| single 300, batch 1000 | 201 | 201 | 201 | 0/8 | 234 | 282 |
| single 600, batch 1000 | 201 | 201 | 201 | 0/8 | 227 | 233 |
| single 80, batch 2000 | 80 | 75 | 118 | 8/8 | 99 | 155 |
| single 150, batch 2000 | 150 | 145 | 184 | 8/8 | 171 | 229 |
| single 300, batch 2000 | 200 | 200 | 201 | 0/8 | 229 | 236 |
| single 600, batch 2000 | 201 | 201 | 201 | 0/8 | 228 | 236 |
| first message, cold, single 80 | 200 | 200 | 201 | 0/5 | 232 | 234 |
| first message, warm, single 80 | 117 | 117 | 119 | 5/5 | 155 | 158 |
| first message, cold, single 150 | 201 | 201 | 201 | 0/5 | 232 | 241 |
| first message, warm, single 150 | 187 | 188 | 189 | 5/5 | 227 | 232 |
| first message, cold, single 300 | 201 | 201 | 201 | 0/5 | 229 | 230 |
| first message, warm, single 300 | 201 | 201 | 201 | 0/5 | 227 | 229 |

**What it says.**
- The batch's latency no longer matters (step 1). Before step 1, the verdict rode the batch, so at batch 400, 1,000
  and 2,000 ms every message would wait the full 200 ms and still be late. That matches the reviewer's live check:
  `wait_ms` 133 to 201, about half late.
- Now the turn waits about route.v1's own latency less the compile. A verdict under the bound comes in time every
  time. One over the bound costs the whole 200 ms and buys nothing that message: the turn runs on the last verdict,
  and the late one carries to the next message.
- The warm-up turns a fresh daemon's first message from always late (cold: the 300 ms set-up pushes even an 80 ms
  answer past the bound) into in time at 80 and 150 ms.
- A first message still waits about 37 ms more than later ones at the same latency (117 vs 80), the cost of a
  session's first turn.

**Recommended default.** Keep 200 until a live measurement, then set it from route.v1's own `answered_ms` on the
owner's daemon, warm, by this rule:
- If the p80 of `answered_ms` less the turn's compile (about `answered_ms - wait_ms` on in-time rows) is under
  about 180 ms, keep 200. Nearly every verdict comes in time, and the turn waits only about Jev's own latency.
- If route.v1 alone answers in 300 ms or more at the p50 (as a lone question with about 50 output tokens may: the
  7-question batch took about 2 s), lower `max_wait_ms` to about 50. A wait the verdict mostly misses is pure cost
  on every message. The verdict then mostly applies from the next message, as the late path already does, and
  `late`/`theseus.route.wait` keep showing the rate.
- Between those, set the bound at the live p80 of `answered_ms` less the compile, rounded up to the next 50 ms.

## Proof, whole

- `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Each suite phase failed only on
  the 33 known L1 tests (after step 2's fix above). I then ran the phases after the suite myself:
  - `protocol types`: `cockpit/src/protocol.gen` unchanged;
  - `target/debug/theseus-sim bench turn --check --runs 5 --burst 0`: `frames_plain` 5 (budget 5), `frames_tool` 9
    (budget 9);
  - `deny`: advisories, bans, licenses, sources ok, offline from the fetched database.

  The suite ran about 3,000 tests a run (3,002 to 3,005). Before the gate I also ran the routing and judge tests alone: 182 passed
  (theseus-judge, and theseus-core's `tests_route*` and `warm`), and 110 passed for `tests_route*`, `route_step` and
  `telemetry`.
- **Under load.** Each run is `tests_route_wait`, `tests_route_late` and `route_step::tests`, 11 tests, under
  `nice -n 19` beside four busy loops at nice 0 (killed by their pids).
  - First batch: runs 1 and 3 passed 11/11. In run 2,
    `jevs_connections_open_after_serving_and_a_first_message_pays_no_setup` failed.
  - Rerun alone under load: it failed in five runs out of five, each at its bound on the turn's wall time
    (`took < 1.5 s`; the starved debug turn took longer). The assertions that prove the step held in every run: the
    wait past the compile under 1 s against a 1.5 s set-up, and no new connection.
  - I dropped the wall-time bound (`7c94e983`). Then five runs under load passed 11/11 each.
- **`theseus-sim bench turn --judge --runs 10`, before and after: not run.** The `--judge` arm is judge-turn-cost's
  bench (in flight, theseus-sim perf). It is not on this branch's base (`grep -rn judge crates/theseus-sim/src`
  finds only the quiet config that turns the judge off). The maintainer should run it after the join. None of this
  branch's changes writes a frame. The keeper's warm-ups write nothing, and `route.decided`'s new fields ride in the
  row the turn already writes. So the judge arms should still show 0 judge frames before the answer. The judge-off
  turn bench is unchanged: 5 and 9 frames.

## The live check (the maintainer's; real Jev, a few cents)

A scratch daemon of this branch's build, on a fresh state dir. Discord and web off, `[judge]` on with the Jev key,
routing live:

```sh
S=$(mktemp -d /tmp/route-wait.XXXX)
theseusd example-config > $S/theseus.toml   # then edit: [judge] enabled = true (key_secret's op:// reference),
                                            # [routing] enabled = true, mode = "live", max_wait_ms = 200,
                                            # [routing.modes.sophisticated] profiles = ["opus"],
                                            # [discord] enabled = false, [web] enabled = false
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &
D=$!
until theseus --socket $S/sock health >/dev/null 2>&1; do sleep 0.2; done
# 20 messages, 2 s apart, the first on the fresh daemon, in one session.
SID=$(theseus --socket $S/sock ask --json "Weigh two designs for a crash-safe log." | jq -r .session_id)
for i in $(seq 2 20); do
  sleep 2
  theseus --socket $S/sock ask -s "$SID" --json "Message $i: and what of the index's rebuild?" >/dev/null
done
theseus --socket $S/sock ledger -k route.decided -n 20 --json > $S/route.json
theseus --socket $S/sock ledger -k judge.call -n 60 --json > $S/judge.json
theseus --socket $S/sock shutdown; wait $D
```

The summary:

```sh
jq -r '.rows[] | [.data.wait_ms, .data.late, .data.answered_ms, .data.reason] | @tsv' $S/route.json
# in-time rate, p50 wait, p50 answered_ms
jq '[.rows[].data] | {n: length,
     in_time: (map(select(.late == false)) | length),
     p50_wait_ms: (map(.wait_ms) | sort | .[length/2|floor]),
     p50_answered_ms: (map(.answered_ms | select(. != null)) | sort | .[length/2|floor])}' $S/route.json
# the second request's cost: route.v1's calls, their tokens and cost
jq '[.rows[].data | select(.pack == "route.v1") | {usage, cost_micros, call: .call.questions}]' $S/judge.json
```

What it should show:
- No row with `late` true from a slow connection on the first message. The first row's `answered_ms` should be close
  to the later rows', not a set-up (100 to 300 ms) above them.
- `wait_ms` at most about 200 on every row, and about `answered_ms` less the compile on in-time rows.
- `late` true only where `answered_ms` is null.
- The in-time rate and p50 `answered_ms` are the inputs to step 4's rule for the default.
- route.v1's `judge.call` rows have one question (`call.questions` 1) and their own call id, distinct from the
  batch's.
- Health's `judge` block shows the breaker `closed` right after serving, before any message (the keeper built the
  client).
- With telemetry on, `theseus.route.wait` points by `theseus.route.late`.

## What is left, or uncertain

- **The keeper's cost when idle.** With the judge on, it sends 2 `HEAD`s every 150 s of silence, day and night:
  about 1,150 a day, unbilled. If the owner would rather not, it could keep warm only for an hour after the last
  message (then the first message after an hour pays the set-up again). I kept it simple, as the brief says "keep it
  warm".
- **`answered_ms` for a late verdict is null**, because the row is written before the answer comes. The late
  verdict's own `judge.call` row carries its latency.
- The fake bills each question its state, so the split's extra input cost under real Jev is a reasoned figure, not
  a measured one. The live check's `judge.call` rows measure it.
- `inbound.rs`'s `ROUTE_PACK` doc still said "batched with classify.v1" from before step 1. I corrected it in step 3.

## Docs (not edited; for the maintainer)

For the spec's route.v1 item (Part III), and `docs/status.md`'s routing line:

> route.v1 asks at the inbound point in a request of its own, beside the batch of classify.v1 and role.v1
> (theseus-ddbi). Jev answers one JSON body, not a stream, so a batched verdict could come only when the whole batch
> had, about 2 s after the turn started. One question answers sooner, and its verdict goes to the turn over a
> oneshot the moment its request answers. The state is sent twice, once a request. The turn waits for it beside
> its first compile, at most `[routing] max_wait_ms` (200) after it.
>
> Jev's connections are opened after serving, never on the start path (`Core::warm_judge` from `after_serving`;
> `judge/warm.rs`): two `HEAD`s of the judge's path, unbilled, kept warm by a keeper that sends them again after
> each 150 s of Jev's silence, inside the pool's 180 s idle time. So a fresh daemon's first message pays no
> connection setup.
>
> `route.decided` keeps `wait_ms` and gains `late` (the verdict had not come by the decision, so the turn ran on the
> session's last verdict or its base) and `answered_ms` (when route.v1's request came back, in ms after the turn's
> start, or null). The histogram `theseus.route.wait` (ms), by `theseus.route.late`, counts each turn route.v1 acts
> on.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the last code commit (`7c94e983`) failed in its
suite on 34 tests:
- 33 are the known L1 ones (theseus-pv6i): theseus-sandbox's contract tests (19), its bench's `spawn_100`, and
  theseusd's sandbox tests (13);
- 1 is `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy` (theseus-1g8j, on the
  brief's list of load flakes): "the pool thread took the idle thread's policy", left 0, right 5. It passed alone
  three times out of three, and this branch touches nothing of `learning/`.

The gates on `1c68f7d9`, `e4577bfd` and `3d1df6ca` each failed only on the 33 L1 tests. In every gate, fmt, shape,
features, clippy, cockpit, the test build and the reader rule passed, and so did the phases I ran after the suite
(protocol types, the turn bench: 5 and 9 frames, deny).
