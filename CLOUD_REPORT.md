# CLOUD REPORT: core-waits (theseus-23wh, theseus-ig6n, theseus-t2yb, theseus-vbju)

Branch `cloud/20261005-core-waits`, from `main` at 4a449460 (store format 22; batch 8's queue-frames was not in it:
`git log --grep theseus-6qwr` finds nothing, so its move of a late result's wake did not bear on this wait; nor were
timing-flakes' fixes: theseus-cs71 is named only in an earlier merge's message). Four commits,
one per issue, all in theseus-core's tests. No kernel, wake.rs, judge or fake.rs change; no store format,
protocol, config or dependency change. No file nears its ceiling: tests_m3.rs is 7,832 of 8,050.

The VM: 4 cores, 15 GB, root. "The recipe" below is AGENTS.md's: the one test, from the test binary built once,
with `--exact` and `--test-threads 1`, at `nice -n 19`, beside four nice-0 `sh -c 'while :; do :; done'` loops (one
per core), in a loop, killed by pid. "8 loops" is the same with eight (two per core): t2yb and vbju would not fail
under the recipe here, and did under 8 loops. Every failure's output is kept in the session; the counts are below.
Durations are wall time per run, process start included.

## 1. theseus-23wh: the golden's wait for its wake (commit 4b9611d)

**Found.** I first made the wait's timeout say what it saw (state, wake, each pending wake's due time and its
distance from `now_ms`, `now_ms`). Under the recipe, main failed 4 of 5 runs (80.8, 79.4, 82.5, 82.5 s; one pass in
49.1 s), and all four said the same:

```
no wake due in 30 s: state Queued, wake None, pending [wak_… due 1791256755190 (-30923 ms)], now_ms 1791256786113
```

Neither part the brief named was missing. The wake was due about 0.9 s before the wait even began: the turn that
set it ("after": "1s") took about 1.9 s after its `wake.at` call (its second loop, its frames), so the kernel's
`end_turn` found the free execution's own wake already due and queued it itself (`why: "wake"`, kernel.rs ~1357)
instead of parking it `Waiting`. `due_now` needs `Waiting`, so it never held, and no poll length helps. That path is
also a transcript of another shape (an `execution.queued` in the turn's end frame, no due-scan frame from `drive`),
so it must not happen at all, not just be waited out.

**Difference from the brief's reading of the code:** the brief's "the due time by the kernel's clock" is right, but
the failure is the turn's end outrunning the wake, which `end_turn` turns into a different state; the clock and the
poll were fine.

**Changed.** The scenario's wake is set `WAKE_SECS` = 20 s out (ten times the tail measured). Its input still masks
to `"after":"#s"`, so the golden does not move; the prompt says "Wake me in twenty seconds." (the prompt is not in
the golden). `wake_due` waits by the scenario's progress: first the turn has parked the execution (`free`, `Waiting`,
the wake pending), else it fails at once, naming what it saw; then it sleeps to the wake's due time by the kernel's
clock (at most 1 s per sleep) until `due_now`. A 60 s guard, for a wake that never comes due, prints the same
`wake_seen`. It does not read the bus tap. It runs on whatever tokio runtime runs `conversation` (only
`tokio::time::sleep`), so turn-stack's own runtime drives it too.

**What the test then proves:** that a turn which sets a wake parks its execution with the wake pending, that the
wake comes due by the kernel's clock, and that `drive` fires it through the due scan and runs its turn, the
transcript as the golden has it. It no longer runs the first second-scale wake: unloaded it takes about 22 s, was 4.

**Proved.**
- After, the recipe, 30 runs of the final binary: 29 passed, 0 real failures. Run 14 "failed" on line 1204
  because my own commit sequence (`git stash`) put HEAD's golden, with `-#:#`, on disk for a moment while that run
  read it; one extra run passed in its place: 30 clean passes. Durations: runs 15 to 30 and the extra 68.2 to
  71.9 s (median 71.1); runs 2 to 9 ran beside the gate's compile and suite, 82.8 to 109.1 s; 10 to 13, 47.7 to
  68.9 s.
- An earlier binary (before ig6n) passed its first 7 recipe runs (69.2 to 75.9 s); its later runs failed only
  because I rewrote the golden file for ig6n under it, so they are not counted.
- **Plant:** in the kernel's `set_wake` (wakes.rs), `let due_at_ms = due_at_ms + 3_600_000;`. The golden failed at
  the guard after 63 s: `no wake due in 60 s: state Waiting, wake Some(Input), pending [wak_… due … (+3559869 ms)],
  now_ms …`. Restored and touched; `git status` clean.

## 2. theseus-ig6n: the golden's time zone (commit a866d9f)

**Changed.** tests_output's normaliser (`Mask::text`) writes an offset's sign as `±` (`offsets`: a `+` or `-` after
`HH:MM:SS ` and before `HH:MM`, `wake::Local::full`'s shape), with a unit test
(`a_local_times_offset_is_masked_with_its_sign`: west, on and east of UTC read alike; `2026-10-06`, `a - b`,
`12:30 -07:00` and a one-digit hour are kept). `THESEUS_GOLDEN=write` moved exactly the two `wake.at` lines (the
tool's preview, the narrator's "Wake #N set for …"), `-#:#` to `±#:#`. I chose the mask over pinning TZ: setting an
environment variable while other tests read the local time on other threads is a data race.

**Proved.** The golden passes under `TZ=UTC`, `TZ=America/Phoenix` and `TZ=Asia/Tokyo`; main's binary fails under
`TZ=UTC` (line 1204, `+#:#`). **Plant:** `Mask::text` without `offsets` (the sign kept): the golden fails under all
three zones (`+#:#`, `+#:#`, `-#:#` against `±#:#`). Restored and touched.

**The gate line's `TZ=America/Phoenix` can go once this joins** (it is in the cloud brief's gate line; scripts/ and AGENTS.md do not set it).

## 3. theseus-t2yb: a cancel's wait for four dispatched reads (commit 5d81b23)

**Found.** Under the recipe, main passed 30 of 30 (3.4 to 4.1 s). Under 8 loops it failed 13 of 20, each "never
dispatched" at the 5 s bound (the runs took 29.5 to 32.2 s, the held CPU-pool reads finishing after the panic). The
600 ms reads can end before the fourth is dispatched, or the 10 ms poll can miss the window where all four run.

**Changed.** `Timing` gains a hold (`hold(bool)`, `wait_held`; a run that has started waits while it is on, capped
at `HOLD_WAIT` = 90 s, past the test's own guard, so a test that never releases cannot hang the pool). The test runs
on `rig_parts` with `cpu_cores = Some(4)`, holds the reads, waits until all four are `Dispatched` (a 60 s hang
guard that says `{n} of 4 dispatched`), cancels, and only then releases: each late completion comes after the cancel
by order. The 600 ms delays go. Every assertion is as it was. Only `Timing` and this test changed.

**Proved.** After: the recipe 30 of 30 (3.7 to 4.5 s, median 4.0); 8 loops 30 of 30 (9.4 to 11.8 s, median 10.4).
Unloaded 0.07 s; in the gate 0.2 s. **Plant:** the kernel's `cancel` (kernel.rs:2603, `for c in &e.outstanding`)
as `e.outstanding.iter().skip(1)`, so one running call is left dispatched: the test fails at once, `the four
running calls`, left 3, right 4. Restored and touched.

## 4. theseus-vbju: a failing Jev's turn bound (commit c7df97d)

**Found.** Under the recipe, main passed 30 of 30 (22.9 to 24.2 s). Under 8 loops it failed 10 of 10, every time at
`Down`, the first mode: `Down: the turn took 3.18 s` to `3.58 s`. Down fails at once, so that turn waited on nothing:
the bound measured only a fresh rig's first turn's speed.

**Changed.** Only this test. `Slow` sleeps for 300 s against `total_secs` = 30. A judgment books its call as it
ends (`judge_loop`'s `budget.settle`), so right after the turn returns the test asserts health's `(calls_today,
failed_today)` is `(0, 0)`: a turn that waited returns after the 30 s timeout with `(1, 1)`. The judgment's row is
no such signal: it rides a later frame of the sink's own (my first version checked "no row yet", and the plant
below passed it; the health counters were the fix). The other modes fail at once, so a waiting turn and a
non-waiting one take the same time; a wall bound never told them apart, and they keep their class and
unchanged-turn checks. The wait for the row has its own guard, 60 s (the timeout plus 30). The test takes about 39 s
unloaded (38.8 s in the gate), mostly the slow mode's 30 s.

**Proved.** After: the recipe 30 of 30 (48.6 to 50.3 s, median 49.3); 8 loops 10 of 10 (63.8 to 66.1 s); unloaded
39.2 s. **Plant:** `at_loop_end` judging in place,
`tokio::task::block_in_place(|| rt.block_on(judge_loop(..)))` for its `rt.spawn`: the test fails, `Slow(300s): the
turn returned after 30.06s with its judgment ended`, left (1, 1), right (0, 0). Restored and touched.

## Under load, and the suite

Five runs of each module, the final binary, `TZ=UTC`, `nice -n 19 --test-threads 4` beside four nice-0 loops:

- `tests_output::` (3 tests): 5 of 5 passed, 71 to 73 s.
- `tests_m3::parallel::` (8 tests): 4 of 5 passed, 12 to 17 s. Run 1 failed one test that is not mine and not on
  the brief's list: `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn` panicked at
  tests_m3.rs:6401 with `[0, 84, 0, 0, 0, 0, 0]`: one of seven instant reads recorded `duration_ms` 84 against its
  `< 20` bound. It uses neither `Timing` nor `Slowed` (a plain `rig` and the built-in `fs.read`), so the hold does
  not reach it; it is a wall bound on a read's own run, which a starved CPU-pool thread stretches. A finding for an
  issue of its own; the other four runs passed it.
- `tests_judge::` (10 tests): 5 of 5 passed, 48 to 49 s.

theseus-core's whole suite once under load: `TZ=UTC cargo nextest run -p theseus-core --retries 0 --no-fail-fast`
at nice 0 beside four nice-0 loops: 1,280 of 1,280 passed (4 skipped), 103.7 s of tests.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the final tree (all four changes; run while my
loaded golden runs were going): fmt, shape, features, clippy, cockpit, test build and the reader rule passed; the
suite ran 2,831 tests, 2,797 passed, 34 failed: the 33 known L1 tests (theseus-sandbox's contract tests and
`spawn_100`, theseusd's `sandbox` tests; root with no job cgroup, theseus-pv6i) and
`theseusd::bench_profile::theseusd_check_passes_on_the_bench_profile_with_no_vault`, which fails on the same cause:
its `theseusd check` output says `L1: the self-test failed: checking the job: a workspace root, /, is the root`
(theseusd, untouched). All of theseus-core passed in it (the golden 23.1 s, t2yb 0.2 s, vbju 38.8 s). The phases
after the suite I ran myself: the protocol types (protocol.gen unchanged), `theseus-sim bench turn --check --runs 5
--burst 0` (plain 5 of 5 frames, tool 9 of 9), and `cargo deny --offline check` (advisories, bans, licences, sources
ok; the database fetched at setup). Each commit was checked with fmt and theseus-core's clippy at its own tree; the
whole gate ran once, on the head, not once per commit: the four steps touch three separate test files.

## The live check (the maintainer's, on a 16-core machine)

Build both trees' theseus-core test binary (`cargo test -p theseus-core --lib --no-run`, then copy the
`target/debug/deps/theseus_core-<hash>` it names), then for each binary `B` and each test `T` of
`tests_output::the_cores_output_matches_its_golden`, `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched`,
`tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`:

```bash
pids=(); for i in $(seq "$(nproc)"); do sh -c 'while :; do :; done' & pids+=($!); done
fails=0; for i in $(seq 200); do
  (cd crates/theseus-core && TZ=America/Phoenix nice -n 19 "$B" --exact "$T" --test-threads 1) >/tmp/run.log 2>&1 || { fails=$((fails+1)); cp /tmp/run.log /tmp/fail-$i.log; }
done; echo "$T: $fails of 200"; kill "${pids[@]}"
```

Main should fail the golden at "no wake due" (theseus-23wh) and, perhaps, the other two as their issues say; this
branch should fail none. Expect the golden at about 20 s more than main's passing runs. (Use `TZ=America/Phoenix` for
main's binary; this branch's passes in any zone.)

Then:

```bash
cd crates/theseus-core && for z in UTC "$(cat /etc/timezone 2>/dev/null || echo America/Phoenix)"; do
  TZ=$z cargo test -p theseus-core --lib tests_output:: ; done        # 3 passed in each zone
cd ../.. && TZ=UTC cargo nextest run --workspace --retries 0           # at the gate's load: none of the four fails
```

## Left, uncertain, and for the owner

- **The golden is now about 20 s longer**, every run: the real clock has no seam (`Core::build` passes `RealClock`;
  `Parts` has none), so the only way to keep the scenario on its parked path under any load is a wake its turn
  cannot outrun, then a real wait. Under the recipe here it took ~71 s; beside the gate's own compile and suite
  up to 109 s, which is near nextest's 120 s kill. That margin was already thin on main (its scenario alone took
  ~50 s here under the recipe); 20 s is my choice of margin, and 10 s would still be five times the tail I measured
  if the owner prefers the shorter test. A clock seam in `Parts` (a `Clock` for the kernel, as kernel-sim has)
  would make this wait instant and exact; that is a kernel and rpc change outside this task.
- tests_judge's `a_failing_jev…` now takes ~39 s (was ~23 s); the gate's suite ran 212 s, and it is not the tail.
- tests_inbound has a test of the same name (`tests_inbound::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`,
  13.6 s in the gate). I did not read or touch it (judge-tests' area); if it has the same 3 s bound, it has the
  same flaw.
- Main's t2yb and vbju would not fail under the recipe on this VM (30 of 30 each); they did under two loops per
  core. The maintainer's 16-core run is the one to compare with the issues.
- Docs: the cloud brief's gate line can drop `TZ=America/Phoenix` once ig6n joins, and its known-failures list
  theseus-23wh, ig6n, t2yb and vbju. No spec or status edit is owed by the code.
