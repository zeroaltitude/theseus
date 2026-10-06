# Cloud report: daemon-proofs (theseus-xbtr, theseus-hohs, theseus-nh1k, theseus-grxh)

Branch `cloud/20261005-daemon-proofs`, from main at 4a449460 (store format 22, unchanged here). Started 03:07 UTC,
report 04:58 UTC, 2026-10-06. Machine: the 4-core VM, as root, no sccache. "Under load" below means the test at
`nice -n 19` beside four busy loops at nice 0 (AGENTS.md's recipe), unless it says otherwise.

Commits, oldest first:

| commit | step |
|---|---|
| 04d051d | daemon: a `--stdio` daemon's stop waits for its tasks, so its store closes before the process ends (theseus-xbtr) |
| c9bfa31 | store: the page tests write the same rows unsynced and in fewer frames (theseus-hohs) |
| 378c1fc | nextest: the clean-stop test leaves the flaky list (theseus-xbtr; 81kk's override) |
| a0cd63e | sim: the lifecycle bench times a cancel's round trip, its verdict checked (theseus-nh1k) |
| feb02b1 | mcp: a test that only the L1 role's watch of the daemon can pass (theseus-grxh) |

No new dependency, no store format change, no protocol type change, no config key.

## 1. theseus-xbtr: a stdio daemon's clean stop

**Reproduce first.** `versions::a_stdio_daemon_stops_cleanly_on_a_sigterm_or_a_sigint` alone, no retries
(the test binary run directly), `THESEUS_LOG=debug`:
- 30 runs under the recipe (4 loops): **30 passed, 0 failed** (about 6 s a run).
- 30 runs beside 16 busy loops: **30 passed, 0 failed** (about 28 s a run).

So no natural failure on this VM. I went to the cause instead, from the reported numbers.

**What I found.** The reported failure, `replayed_into_index` 10 at `last_position` 20, is the second run's
*every* record, so it is not one late row past the last checkpoint: the checkpoint itself was lost. The stdio arm
ended differently from the socket daemon: `main` dropped the socket daemon's runtime (`drop(rt)`, which waits for
every blocking task), but shut the stdio daemon's down with `rt.shutdown_timeout(500 ms)`, because tokio's stdin
reads on the blocking pool and only the client's end of the pipe ends that read (theseus-p7q). The stop's last
checkpoint (`checkpoint_for_close`) is a non-durable commit that redb's close makes durable as the store drops.
So any task on the blocking pool still holding the core 500 ms after the stop (on a loaded machine: the outbox's
warm read, the retention or adjacency warm builds, consolidation, the learning pass) kept the store open as the
process ended: redb never closed, its last durable commit was the previous run's close, and the next start
replayed the whole run and repaired the index. A debug log of a passing run shows the order: `last checkpoint`,
`store: index closed` (2 ms later), then `runtime dropped` at 501 ms (the abandoned stdin read).

No append lands past the checkpoint here: `close_late_rows` (81kk) already holds that. It is the close, not a row.

**What I changed (04d051d).** `crates/theseusd/src/stdio.rs`: the pipes are copied by two plain threads to and
from one end of a `UnixStream::pair()`, and the core serves the other end as an ordinary tokio `UnixStream`
(`into_split`). The stdio arm's end is now `drop(rt)`, as the socket daemon's is: it waits for the blocking pool,
and the store closes before the process ends. The stdin thread stays blocked in its read, holding nothing of the
core; the stdout thread writes each read at once (no lock held between writes) and, on its end or a write error,
shuts the pair's read side, so a client gone from stdout fails the core's next write as the closed pipe did.
`main` waits up to 500 ms for the stdout thread after the runtime drops (`stdio::flush`). A debug build's plant,
`THESEUS_TEST_HOLD_CORE_MS` (beside `crash::planted`'s kind), holds the core on the blocking pool from the stop's
start for that many ms; it is called in the stdio arm, not in `after_serving` (telemetry3's hunk). The
`Exit::Exec` path (a restart onto a changed note) still uses `shutdown_timeout`: I left it, see below.

The stdio stop's speed also changed: a signal stop no longer waits out the 500 ms bound, so
`a_stdio_daemon_stops_cleanly_on_a_sigterm_or_a_sigint` takes 0.12 s alone instead of 1.1 s.

**Proof.**
- New test `versions::a_stdio_daemons_stop_waits_for_a_task_that_holds_the_core`: a stdio daemon with the plant
  at 1,500 ms, settled, then SIGINT; it must exit 0, the next start must replay nothing and repair nothing, and the
  stop must have taken at least 1.5 s. Passes (1.6 s).
- **Planted revert** (the old end: `shutdown_timeout(500 ms)` for stdio and tokio's stdin/stdout, the plant kept):
  the test fails with
  `{"index_repaired":true,"last_position":10,"replayed_into_index":10,...}` and the log's
  `store: index rebuilt from WAL replayed=10 checkpoint=0 last=10`: the incident's shape, the whole run. Restored,
  `touch`ed, `git status` clean of it, passes again.
- The stdio stop test, the new test, and 81kk's `a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing`,
  30 iterations under the recipe, `--test-threads 1`, no retries: **30 of 30 passed** (90 test runs).
- The whole `versions` file 5 times under load: 5 of 5, 9 of 9 each. `closed_pipe` and `reaping` (its
  `a_stdio_daemon_is_a_subreaper_too`) pass.
- The stdio test's override is off `.config/nextest.toml` in 04d051d.

**81kk's override (378c1fc).** `a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing`, 30 runs under load
(in the 30 iterations above), none failed, so it is off the list in its own commit. The list now has no override.

**Left / for the owner.**
- I could not make the race happen naturally here (cheap syncs, 4 cores); the forced interleaving and the incident's
  exact numbers are the evidence. The 200-run live check below is the real test of it.
- The stdio arm never listens for the `shutdown` method: only the socket loop awaits `core.shutdown`. A stdio
  client's `shutdown` is answered, and the daemon keeps serving (I found it writing the test; it was so before this
  change). AGENTS.md's "Every clean stop is one path: the shutdown method, ..." is not true of `--stdio`. Not fixed
  here: it needs the answer written before the conn future is dropped. Worth an issue.
- `Exit::Exec` (a stdio or socket restart onto a changed note) keeps `shutdown_timeout(500 ms)`, with the same
  hazard in principle; for the socket daemon it could be `drop(rt)` too. Not in this task.
- theseusd's AGENTS.md has a trap line for the relay and the plant (in 04d051d).

## 2. theseus-hohs: a 21-second page test

**Measured first** (timings in the test, then removed; strace for syncs). This VM's `fdatasync` costs about 27 µs,
so the 21 s on the owner's machine is about 14 ms × 1,511 syncs. Before:

| test | syncs | alone | starved (nice 19, 4 loops) |
|---|---|---|---|
| `a_filtered_page_equals_the_scans_answer` | 1,511 fdatasync + 4 fsync | 0.77–0.84 s (appends 0.72, walk 0.02, queries 0.06) | 41–49 s |
| `cursor_pages_under_concurrent_writes_have_no_duplicate_or_gap` | 423 | 0.15–0.17 s | 9.6–10.3 s |
| `each_terms_count_follows_its_keys_and_equals_a_walk` | 431 | 0.18–0.21 s | 10.8–11.9 s |

With only the sync off, the page test starved was still 34–35 s, of which the appends were 30 s and the queries 5.6:
each frame is a handoff to the store's writer thread (theseus-vni9), and a starved process waits for each handoff.

**Choice (c9bfa31).** Two of the listed options, for the two costs:
- `fsync: false` for the module's stores (`unsynced()`, used by `open` and the terms test's `opened`), as
  store/tests.rs does where the sync isn't the subject. Nothing these tests assert depends on the WAL's sync: the
  index is written after each frame either way, and redb's commits keep their own durability (the module's
  durable-checkpoint tests still pass).
- Fewer frames for the same rows in the page test: the same seeded loop builds the same rows, appended every 25
  batches and at batch 700, so the checkpoint follows batch 700 as before. Checked, not assumed: a digest of every
  walked row's (position, kind, session), the last position (3200), the row count (2985), and the checkpoint's
  position (1480) are identical before and after. Every page shape (after, before, both, neither, the limit range,
  the two-tag read) is unchanged; I kept all 400 queries.
- The cursor test's 400 setup rows go in frames of 25 too; its writer, the subject, still appends a frame per row.
  The terms test keeps a frame per row (a key's terms changing frame by frame is its subject): unsynced only.
- Not chosen: an environment knob (it would test less by default), a cheaper oracle (the queries are 60 ms alone),
  a slow-timeout override (it hides the cost rather than removing it).

After:

| test | syncs | alone | starved |
|---|---|---|---|
| page | 11 | 0.19–0.22 s | 10.7–11.1 s |
| cursor | 10 | 0.03 s | 1.0–1.2 s |
| terms | 27 | 0.09–0.10 s | 7.3–9.0 s |

All of `tests_pages` (7 tests) 5 times beside 8 busy loops at nice 19: 5 of 5 (22–25 s a run).

## 3. theseus-nh1k: a cancel's round trip, bounded by a bench

**Choice (a0cd63e): the lifecycle bench**, not `bench jobs`: `bench jobs` starts the wrapper itself with no daemon,
so there is no `execution.cancel` to time; the lifecycle bench already runs a real theseusd and a real `proc.run`
job through a real turn against the stand-in model, and the gate already settles it and reruns it once on a miss.

A `cancel` phase (`PHASES`, `TITLES`, the `--phases` default, `budget_ms`, the history's column): on one daemon,
each run starts a job (`Rig::start_job`, split out of `park_and_start_job`) and times `execution.cancel` from the
request to its answer. Each answer must carry one cancelled action and one verdict with `state`
`termination_verified`, `killed` > 0 and `survivors` 0, and `health` must show no action dispatched; anything else
fails the bench. In the default phases, so the gate's `lifecycle_bench` runs it (about 12 s more at 10 runs).

**Numbers** (debug build, 20 runs a bench):

| condition | p50 | p95 | max |
|---|---|---|---|
| quiet, three benches | 40.5 / 41.1 / 41.2 ms | 42.0 / 42.8 / 43.2 ms | 43.4 ms |
| 4 busy loops, bench at nice 0 | 39.9 | 57.7 | 58.0 |
| 16 busy loops, bench at nice 0 | 74.0 | 94.7 | 105.8 |
| 4 busy loops, bench at nice 19 | 411.7 | 467.4 | 468.5 |

The quiet p50 sits at about 41 ms in every bench: something in the cancel's path waits about 40 ms (a poll, likely in
the wrapper's verification). Not chased here.

**Proposed budget: p95 250 ms**, no margin of its own (`margin_ms` 0), in the code as a proposal for the owner's
machine to set; it leaves the gate's busy allowance room above the 16-loop p95. **Plant:** the review's
`tokio::time::sleep(Duration::from_millis(2500))` right after `terminate_all` in `cancel_execution_judged`:
`bench lifecycle --phases cancel --runs 20 --check` gave p95 2,545.6 ms, "LIFECYCLE BUDGET MISSED", exit 1. Restored
and `touch`ed; `git status` clean of it.

The full default bench (`--runs 5 --check`, every phase) passes with the new row. theseus-sim's tests: 51 of 51
(the history's fixture row gained the cancel column).

Note: halfway through, the disk filled (the debug target's incremental cache, 14 GB): jobs refuse to start below
1 GB free, and the bench then failed with "the turn left no job running". I deleted `target/debug/incremental` and
built with `CARGO_INCREMENTAL=0` after that; every number above is from runs with the disk fine.

## 4. theseus-grxh: the L1 role's daemon watch

**What I changed (feb02b1).** `theseus-sim fake-mcp --outlive-stdin` (stdio only; it conflicts with `--http`): after
`serve_pipes` returns on its stdin's end, the fake waits forever, so only a signal ends it. New test
`mcp_l1::a_server_that_outlives_its_input_ends_with_the_daemons_kill_9`: the server in L1 with that flag (checked on
its command line), the daemon's `kill -9`, and the role, the init and the server all gone within `wait_gone`'s 10 s.
`l1_fake` became `l1_fake_with(args)`. theseusd's AGENTS.md says how to run these as an ordinary user and why the
flag exists.

**Proof, as uid 65534** (this VM lets an ordinary user make user, pid and network namespaces):
`setpriv --reuid=65534 --regid=65534 --clear-groups env HOME=<dir it owns> TMPDIR=<dir it owns> <the mcp_l1 test binary>`
from `crates/theseusd`. The repository and target are world-readable here, so nothing needed loosening.
- All three tests pass (0.2 s).
- **Plant:** the `if fds[1].revents != 0 { return Woke::DaemonGone; }` lines removed from theseus-kernel's
  `mcp_l1.rs`: the new test **fails**, "pid 5509 outlived the daemon's kill -9", at 10.2 s; the old kill test still
  passes (0.06 s). Restored, `touch`ed, `git status` clean of it.
- 5 runs of the file under load (nice 19, 4 loops) as uid 65534: 5 of 5, 3 of 3 each.
- As root the three return at once (the refusal path), as before.

## The live checks (the maintainer's, on the 16-core machine)

1. The two stop tests, 200 runs each, no retries, at nice 19 beside one busy loop per core:
   ```
   cargo test -p theseusd --test versions --no-run   # note the binary it prints
   for i in $(seq 16); do sh -c 'while :; do :; done' & echo $! >> /tmp/busy.pids; done
   cd crates/theseusd
   for t in a_stdio_daemon_stops_cleanly_on_a_sigterm_or_a_sigint a_clean_stop_closes_the_index_and_the_next_start_repairs_nothing; do
     f=0; for i in $(seq 200); do nice -n 19 ../../target/debug/deps/versions-<hash> --exact $t >/dev/null 2>&1 || f=$((f+1)); done
     echo "$t: $f of 200 failed"
   done
   kill $(cat /tmp/busy.pids); rm /tmp/busy.pids
   ```
   Should show 0 of 200 for each. The stdio test now takes about 0.1 s alone, not 1.1 s.
2. Settled: `target/debug/theseus-sim bench lifecycle --phases cancel --runs 20 --check`: p95 well under 250 ms
   (here 43 ms); set the budget from it. Then the gate whole, whose lifecycle phase now runs `cancel`. With the plant
   (`tokio::time::sleep(std::time::Duration::from_millis(2500)).await;` after `terminate_all` in
   `crates/theseus-core/src/rpc/driver.rs`'s `cancel_execution_judged`), the row misses at about 2,545 ms and
   `--check` exits 1.
3. The mcp_l1 tests as an ordinary user (the owner's own uid, not root): `cargo nextest run -p theseusd --test mcp_l1`:
   3 pass; with the two `DaemonGone` lines removed in `crates/theseus-kernel/src/mcp_l1.rs`'s `wait`, the new test
   fails at 10 s with "outlived the daemon's kill -9" and the old one passes.

## Docs the maintainer may want to touch

- AGENTS.md's invariant "Every clean stop is one path: the `shutdown` method, SIGINT, SIGTERM, ..." in
  crates/theseusd/AGENTS.md: not yet true of `--stdio` for the `shutdown` method (see 1).
- scripts/gate.sh's comment above `lifecycle()` lists the phases; it doesn't name `cancel` (a shared file: left).
- lifecycle.rs's module doc says "Six phases"; there were eight before this and nine now. I added the `cancel`
  bullet and left the count alone.
- The spec's §9 has no cancel row; the budget here is Theseus's own, not §9's.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on feb02b1 (with `CARGO_INCREMENTAL=0`, for the disk):
fmt, shape, features, clippy, cockpit, test build, and the reader rule pass; the suite ran 2,832 tests:
**2,799 passed, 33 failed, 21 skipped**, and failed the gate only on the known L1-as-root set (theseus-pv6i): 20 of
theseus-sandbox's (its contract clauses and egress tests, and its bench's `spawn_100`) and 13 of theseusd's
`sandbox` tests. No other failure, no flaky retry (the list has none now), one slow test
(`theseusd::wakes::a_repeating_wake_at_the_floor_runs_twice_a_minute_apart`, over 60 s and passed: it waits a minute
by design). The output golden passed with the TZ. After the suite I ran its later phases myself: protocol types
unchanged; the turn bench `--runs 5 --burst 0 --check` (frames_plain 5 of 5, frames_tool 9 of 9); `cargo deny
--offline check`: advisories, bans, licences and sources ok. The lifecycle and jobs benches are skipped in a lane's
gate; I ran the full lifecycle bench myself (`--runs 5 --check`, every phase ok); `bench jobs --class l1` can't
run as root here.

One false alarm, mine, so it isn't mistaken for a finding: a run of the versions file failed
`each_start_names_its_build_...` because I had run a test binary built at c9bfa31 against a theseusd built at
feb02b1 (that test compares the two builds' commits). The current binary passed 5 of 5 under load.
