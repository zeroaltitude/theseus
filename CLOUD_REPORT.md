# CLOUD_REPORT: task-forecast (theseus-zz752)

Branch `cloud/20261013-task-forecast`. Commits: f259d107 (the feature), 6cee45bc (table read after serving, live scenario),
then the helper split (below). No store format change, no new dependency, no config key.

## What I found
- `core/arrangement.rs` has no "class" of a task. I defined one in `forecast::class_of` from the task record's acceptance
  lines (`open` 0, `checked` 1-2, `specified` 3+): an arrangement's pieces fill the record's objective and acceptance, so
  the class reads the same for a task started now and one started before the board existed (no session read in the warm-up).
- The in-process index has no BM25 terms (the tender is a socket, async). "Nearest brief" is the Jaccard overlap of the hashed
  words of title + objective. No embedding, no model call.
- `loop.ended` rows are counted by the board, so loops are known for tasks that end while the board is up; history rows have
  no loops (the kernel never stored them and adding them is a format change). Loops are in the in-memory forecast, not on the wire.
- "Route down" is NOT offered: `profile.use` is the daemon's live profile, not a per-session verb.
- A real gap, fixed in 6cee45bc: the board seeds on its first watcher, so a daemon nobody watched never read the history.
  `Core::warm_forecasts` now runs from after_serving. Also fixed: a task whose record the board had not read when its frame
  arrived got no forecast (about half of new tasks in the live scenario).

## What changed
1. `theseus-protocol`: `forecast.rs` (`WorkForecast`, calibration types, wording), `WorkView.forecast` (additive),
   `Why::PastForecast` + rule 11 + `forecast_answers()` (Keep going = no call, Stop = `task.cancel`), method `forecast.calibration`.
2. `theseus-core`: `forecast.rs` (+ `forecast/history.rs`, `tests.rs`), `push/work/forecast.rs`, `rpc/forecast.rs`;
   `task.create`'s first line (`⏳ task c91e "…" · forecast 8 min (p90 14) · $0.60 (p90 $1.50) · from 23 like it`, or
   `no forecast yet: 2 like it`); board wiring (the view, the sticky `past` bit, Jev's latest `loop.v1` read from `judge.call`
   rows of the loop pack, a task's end adds its row).
3. CLI: `theseus tasks forecast` (own file `tasks_forecast.rs`; `Tasks` gained an optional subcommand), `theseus work` tree
   lines and the long `theseus status` rows show `$0.42 of ~$0.60` (status asks `work.watch` once, only when a task row shows).
4. Docs: AGENTS.md of core, protocol, CLI. TypeScript regenerated (`cockpit/src/protocol.gen/`). The core output golden moved
   by the one new first line.

## How it was proved
- `forecast/tests.rs` (12 tests incl. a 40-task two-class history: p50/p90 by interpolation, <5 like it = none, class before brief,
  nearest briefs fill the 25, 2,000 kept once each, calibration backtest counts, the book waits for the store and says a forecast once).
- Protocol `row_11_*`; `push/work/tests_forecast.rs` (whole cores: first line + view, "no forecast yet: 2 like it", one notice and
  none after a seeded restart, numbers-only / place rule); theseusd `tasks.rs::a_new_task_has_a_forecast_and_a_slow_one_is_told_once`
  (real daemon: 30 tasks, restart, forecast "from 25 like it", one `past_forecast` notice on a `work.watch` socket; 8/8 passes).
- Planted reverts: (a) class ignored in `forecast_among` failed `the_class_comes_before_the_brief`, `fewer_than_five_*`,
  `two_classes_*`, `the_calibration_*`; (b) the `prev.past` check removed from the policy failed `row_11_*` and
  `a_task_past_its_p90_is_told_once`. Both restored and touched.
- FAST: `a_forecast_is_a_memory_read`: median 0.99 ms over 2,000 rows in a DEBUG build (bound 25 ms debug, 1 ms release);
  a release run was not completed on this VM. Warm-up: logged as `forecast: finished tasks read rows= ms=`; I measured it only at
  0-30 rows (0 ms); NOT measured at 2,000 rows. Run the lifecycle bench / a copy of the store to confirm 50 ms.
- No new frame: nothing is stored; a plain turn is unchanged (golden and frame-budget tests pass).

## Live check for the maintainer
`cargo nextest run -p theseusd --no-capture -E 'test(a_new_task_has_a_forecast_and_a_slow_one_is_told_once)'` prints the first line and the
notice. On a real daemon: `theseus work` (task lines carry `[$x of ~$y]`), `theseus tasks forecast`, `theseus status`.
I did not do a hand-run scratch daemon beyond that test.

## Left / uncertain / for the owner
- The notice is told at the next board frame after the crossing (no timer), so a task idle between frames hears late; a crossing
  while the daemon is down is not told (a seed reads it already past): this is what makes "never twice across a restart" hold.
- Tasks that end while the board is not seeded join the table only at the next start's read.
- Deferred tasks: minutes count from when they began to run live; in history a task over 24 h is left out.
- `judge.call` rows now pass the work board's row filter for the loop pack only (a payload substring check).
- Calibration is a backtest (forecast each task would have had), not stored forecasts.

## Gate
`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in `door`/`keel` on a finding that is not mine: the inherited Keel ack on
1c2beb27 (lifecycle.rs "seed" budget) does not verify here (`%G? E`, no key). I ran the later phases by hand: fmt ok, shape ok,
clippy `-D warnings` ok, cockpit lint/test/build ok, full `cargo nextest run --workspace`: 4,503 passed, 33 failed, all 33
the known L1 ones (theseus-sandbox contract + `spawn_100`, theseusd sandbox). `cargo deny check`: advisories FAILED (no
dependency changed; Cargo.lock untouched), bans/licenses/sources ok. Benches not run. One early `tests_output` timeout ("no job's
completion in 30 s") passed on rerun, and the golden diff was only the intended line.

**Keel findings expected:** none from my commits (my own `#[expect]` was removed by splitting the test).
Keel guard output now: `lifecycle.rs:155 budget-raised "seed"` (inherited).
