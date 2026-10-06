# Cloud report: budget-upper (theseus-ps9i, theseus-xd6l, theseus-9o2o)

Branch `cloud/20261006-budget-upper`, commits 988d524e (ps9i), 8012730c (xd6l), b6f68a35 (9o2o), on e7f26cd5.

## 1. theseus-ps9i: the turn's call on `upper` (988d524e)

**Found.** `turn.rs` reserved `price.reserve_micros(max_tokens, compiled.est_tokens)`, the estimate without its margin.
**Changed.** The two expressions (the `reserve` and the row's `input_micros`) take `compiled.estimate.upper`. `est_tokens`
stays the estimate (counted + estimated, margin left out). `fact/turn.rs`: the field's doc says so, and the narrated line now reads
"... for about N input tokens and the estimate's margin" (the dollars are on `upper`, the count is not); the core golden's
`Calling ...` lines move by those words alone (it masks digits). `tests_m3::one_calls_reservation_and_settlement_match_the_catalog_by_hand`
pinned `est * 2`; it now reads `estimate.upper` (one value, its doc line updated; tests_m3.rs 7,835 of 8,050). No other budget test moved.
**Cost, checked against the code** (`MARGIN_PERCENT` = 40, on the estimated part only; Sonnet 5.5 $2 in, $10 out): append turn
60k counted + 10k estimated: +4k tokens = $0.008, 0.56 % of $1.42 (128k cap); cold 100k estimated: +40k = $0.08, 5.4 % of $1.48;
at a 16k cap, 2.7 % and 22 %. The report's numbers hold.
**Proof.** New `tests_turn_reserve.rs` (2 tests): a new session's first turn reserves `reserve_micros(max_tokens, upper)`, with `upper > tokens`;
a stand-in billing input 11 % over the estimate with its whole `max_tokens` (`Scripted::BilledBy`) settles within the reservation.
Plant (`est_tokens` back at the `reserve` line): both fail; money case: reserved 1,307,258 micros, settled 1,310,258 (past it). With the fix: reserved 1,318,162, settled 1,310,258.
Restored and touched. `cargo nextest run -p theseus-core tests_budget|tests_compaction|tests_output|tests_turn_reserve|...` passed after the tests_m3 value moved; core suite whole: 1379 passed.
**audit.rs.** `audit_reserve` reserves on `est.tokens` of a request with nothing counted (`counted` None), so its `upper` is 1.4x its tokens: the same reasoning holds
(a provider counting more than the byte estimate settles past it). Not changed here; the one-token change would be `est.upper`.

## 2. theseus-xd6l: memory's telemetry in `Core::build` (8012730c)

**Changed.** `runner.memory.export_to(t.clone());` beside the judge's and the stops' in `rpc/mod.rs`'s pipeline branch (one line).
**Proof.** `telemetry::tests_daemon_path::a_core_built_with_a_pipeline_records_the_retention_gauge`: pipeline to the receiver, `+retention`, projection built with 2 nodes, gauge read "2", grown to 3, read "3".
Plant (line removed): fails after 10 s with `no the retention gauge: []`. Restored and touched.

## 3. theseus-9o2o: the stop test (b6f68a35)

**Reproduced** on main's test under the load recipe (4 busy loops at nice 0, test at nice 19): 5 of 5 failed, `took` 6.26, 6.65, 5.47, 5.77, 5.72 s (bound 4.5 s).
**Found.** The unpaced fold's wall time swings from 80 ms to 6 s under starvation, so timing an unpaced twin (my first try, 29 of 30 pass, 1 fail) is no bound either. And
under load the build often had not reached its first pace when the 1.5 s sleep ended, so a stop test proved nothing about waits.
**Changed.** The test waits until `Adjacent::paces()` is 1 (counted before it waits), holds 1.5 s, begins the stop, and asserts the sum of the build's pace waits
(`recall::adjacency::WAITED`, a `cfg(test)` thread-local beside `PACES`, three lines in `pace()`; read on the build's thread) is at least 0.9 of the hold and under hold + 3 looks.
It proves the stop ends the waits whatever the speed of the fold. Under load: 30 of 30 passed, each `waited` 2.005 to 2.008 s, `took` about 7.2 to 7.6 s.
Plant (`quiet_blocking(BOUND)` for `quiet_blocking_unless(..)`): fails, 20.0 s of waits against a 1.5 s hold. It ran here (namespaces work on this VM), unskipped.
(A `recall/` file was touched, for the test-only counter; no behaviour.)

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test build, reader rule passed. Suite: 2992 run, 2959 passed, 33 failed,
all the known L1 ones (theseus-sandbox contract (19) and bench `spawn_100`, theseusd `sandbox` (13): root without a job cgroup, theseus-pv6i). Then by hand: protocol types
(no diff in cockpit/src/protocol.gen), `cargo deny --offline check` ok, `bench turn --check --runs 5 --burst 0` ok (5 and 9 frames). Benches skipped (NO_BENCH).

## Live check (maintainer), on a scratch daemon on the stand-in model, fresh `--state-dir`
1. Cold turn and append turn on main's build and on this one: `theseus --socket <s> ledger` (or `session history`) and compare each `model.calling` row's `reserve`, `input_micros`
   with its `est_tokens` and the `context.compiled` row's `estimate.upper`. Expect on this branch `input_micros` = `upper` x input price, higher than `est_tokens` x price by the 40 % margin of the estimated part (cold turn: 1.4x).
2. A stand-in billing 11 % over the estimate with its whole `max_tokens`: the action's `reserved_micros` >= the turn's settled cost here, below it on main.
3. With an OTLP receiver configured and a `+retention` daemon: `theseus.memory.retention.nodes` after the warm build, as before (the daemon's path is unchanged).

## Left / uncertain
- The narrated line's wording changed (the golden moved by it). If the owner prefers the old words, restore them; the dollars on that line are the only change then.
- audit.rs's reservation is the same shape (above); left alone.
