# Cloud report: sink-fast (theseus-ehkp, theseus-xkbs, theseus-ju99)

Branch `cloud/20261006-sink-fast`, from main at 57f265f2 (store format 23). Started 01:11 UTC, report written
04:25 UTC, 2026-10-07. A 4-core VM, root, fdatasync about 0.15 to 0.2 ms: **on this disk the gain shows in counts
(frames, syncs) far more than in walls**, as the brief expected. No store-format bump: no record changes. The
categorize mark and the budget record are the same META records under the same keys; only the frame they ride in
changes.

## Commits

| commit | step |
|---|---|
| 877fecee | 1. ehkp: a frame's staged blobs written before the between-turns guard |
| 6871a7f0 | 2. ehkp: `Blobs::put_many`, a batch's syncs together and its directory once |
| 47814b8f | 3. ehkp: a stop writes its blobs as one batch and its rows in frames of 512; the stop line counts turns running |
| 93a73788 | 4. xkbs: categorize's mark in the sink's frame; a shadow judgment's block written between turns; blocks written ahead |
| 42a7b615 | 5. ju99: a test holds the busy-bound guard |
| 2c2ff8fc | found while measuring: `Blobs::put`'s shared temporary name raced (ENOENT); the stop line's own frames |
| 6d45409c | found while measuring: a sink frame carries each session's newest mark alone |
| 52320342 | the 180 s burst measure, an ignored test in theseus-sim (`perf/burst.rs`) |

## 1. ehkp: the blobs before the guard (877fecee)

**Found.** `sink::run` took `Turns::between`, then `write_one`, which wrote each staged blob (two syncs each) and
then appended. A turn beginning mid-frame waited for every blob's syncs.

**Changed.** Before it asks `between`, the writer peeks the front frame's judgments (`Queue::front_blobs`, which
takes nothing from the queue, so a stop meanwhile still finds every judgment) and writes their staged blobs on a
blocking thread. The guard then covers the append alone. The second hazard: `write_staged_blobs` used to remove a
blob from `staged_blobs` before its put, so a stop's flush could find it gone and append a row naming a blob still
being written. Now a blob stays staged until its put returns, and the puts go one caller at a time
(`blob_puts`), so finding it gone means it is on disk. `write()` still writes any blob that landed after the
peek (those few are written inside the guard: a judgment that joined the frame after the pre-write).

**Proof.** `tests_sink_blobs::a_turn_beginning_while_a_frames_blobs_are_written_runs_at_once`: a staged blob,
`hold_puts`, the writer stuck in the put; a turn's `begin` completes at once (timeout 2 s); no frame while the turn
runs; the row lands after the blob, a quiet stretch after the turn. **Plant:** no pre-write (the blobs back inside
the guard): the `begin` times out, FAIL.

## 2. ehkp: the syncs batched (6871a7f0)

**Changed.** `Blobs::put_many(&[&[u8]])`: every new blob (deduplicated, those already stored skipped) written to its
temporary file, then each synced, then each renamed, then the directory synced once: n + 1 syncs for n blobs where
`put` makes 2n. `put` keeps its contract for attach.rs, each shadow point's prepare, and learning/.
`Blobs::syncs()` counts syncs (files' and the directory's) for the tests and the stop line.

**What a crash leaves at each step:** before the renames, temporary files no name reads (as `put`'s); after some
renames and before the directory's sync, each blob there whole or not at all; after it, every one. The sink appends
no row naming any of them until the batch returns, so no row outlives its blob.

**Proof.** `blobs::tests::many_blobs_are_written_with_one_directory_sync` (5 new + 1 stored + 1 repeated: 6 syncs, no
temporary left, nothing synced on a second call). `tests_sink_blobs::a_frames_blobs_are_synced_together_with_one_directory_sync`:
20 staged blobs in one frame, 21 syncs. **Plant:** a `put` a blob: 40 syncs, FAIL.

## 3. ehkp: the stop (47814b8f, and 2c2ff8fc's line fix)

**Checked: no turn runs when `finish_stop` flushes?** Not guaranteed. The stop (theseusd main.rs) does not wait for
running turns: a turn held at its model is a task that runs on while the daemon stops, and the flush does not wait
for it (it never takes `between`). Its frames land beside such a turn; the turn's own later frames are the stop's
existing concern (`close_late_rows`). I did not change that; the stop line now says `turns_running` so the
maintainer sees it. In the burst runs below it was 0.

**WAL limits.** A frame's body length is a u32 (4 GiB); the log refuses only past `max_total_bytes`; a frame never
splits across a 64 MiB segment (a bigger one rolls to a fresh segment). A judge row with its facts is about 1 to 2
KB.

**Changed.** `Queue::flush` writes every queued judgment's staged blob first as one `put_many`, then the rows in
frames of `STOP_ROWS` = 512 (about 1 MB a frame). A batch's file syncs go out from up to 4 threads when there are
more than 4 (`Blobs::sync_files`, `std::thread::scope`). **Not `syncfs`**, measured beside a writer
(`dd` of 256 MiB in a loop), 700 files of 2 KB, three runs each:

| | sequential fsyncs | 4 threads | syncfs |
|---|---|---|---|
| alone | 128, 134, 127 ms | 82, 79, 77 ms | 25, 19, 19 ms |
| beside the writer | 815, 421, 552 ms | 377, 458, 72 ms | 273, 51, 26 ms |

syncfs is fastest alone, but its first run beside the writer paid for the writer's dirty pages (273 ms) and the
writer pays for it too; on the owner's machine a neighbour's build is that writer. Threads keep the cost to our
files. The owner should hear this choice; it is one function to change.

The stop line (`stopping: the judge's settled judgments are written`) now has `frames` (the flush's own, from
`JudgeService::flushed_frames`), `blob_syncs` (the blobs' syncs during the flush), and `turns_running`. A first
version counted every frame the store gained during the flush, route.v1's in-flight budget frames among them (41
frames for 3,738 judgments); 2c2ff8fc fixed it.

**Proof.** `tests_sink_flush`: now 1,100 judgments, 300 of them naming a staged blob, a turn held so the sink writes
nothing: 3 frames (with 32-row frames planted back: 35, FAIL), one sync a still-staged blob and one for the
directory (269 = 300 − 32 + 1: the writer had pre-written the front frame's 32 before its wait, which the test
counts), and a restart reads every row and every blob. Ignored measure
`tests_sink_flush::measure_a_stop_with_a_backlog_and_its_blobs` (3,000 queued, 700 staged): **383 ms, 6 frames, 693
blob syncs** (main's flush would be 94 frames and 1,400 syncs by its code; I did not build main's test binary).

## 4. xkbs: the mark and the block off the turns (93a73788, 6d45409c)

**The mark.** Carried in the sink's frame: `prepare_categorize` no longer writes it; once the reservation is made the
moved mark goes into the point's `unwritten` map (by judgment id) **before `deciding` ends**, so the next exchange end
reads it at once (`JudgeService::mark`: the newer by `through` of memory and store). The sink's `write` adds a META
record per categorize judgment in the batch beside its row, and drops the map entries after the append. A crash loses
mark and row together. A skipped or failed judgment's row is written as any other, so its mark moves too. A mark the
store already holds a newer one than is left out (a later judgment's row written first), and **6d45409c**: one frame
carries each session's newest mark alone (`newest_each`); `bench turn --judge` showed a frame with two marks of one
session, and in settle order the older could have been written last.

**The block.** Callers of `reserve`, and where they run:
- loop.v1 (`judge_loop`, spawned after the turn's end), categorize.v1 (spawned), continue.v1 (`judge_continue`,
  spawned mid-turn), the memory pass's (`judge_memory`, spawned): none is a turn's own task. Their prepares no
  longer reserve; the async half calls `judge::reserve_between` after the prepare: if `ShadowBudget::needs_frame`
  (the first reservation of a process, a new day, a new block, the day's first pause), it waits for
  `Turns::between` (the sink's timing), then writes the frame there, then calls Jev. So the block is durable before
  the calls it books. The service is held only for the write, never across the wait.
- the gate (security.v1/v3, `prepare_gate`): **left at once**, so a notice never waits for its turn's end.
- consolidation's citation check: **left at once**: an operator's `memory.consolidate` awaits it, and a job inside a
  turn can make that call, which would then wait for its own turn.
- route.v1 and a live rerank: `reserve_beside`, unchanged (theseus-otny).

**Written ahead.** Every sink frame raises the next block in memory once less than half the current is left
(`ShadowBudget::ahead`) and writes it with the budget's record, so most reservations need no frame at all. A crash
then books up to **two** blocks' rest (spend.rs's doc said one; updated). A new `blocks` lock serializes the raise
and its write, and `reserve`'s raise and write, so a reservation that fits a raised block finds it written (it used
to be a few ms' race in `reserve`; with a wait between turns it would have been long). `reserve_beside` takes no lock
(a turn waits on it), so its accepted gap is as before.

**Proof.** `tests_sink_off_turn`:
- `categorizes_block_and_mark_wait_for_the_running_turn`: tenth human message, then a turn held: the decision reads
  the session, no frame lands and Jev is not called for FLUSH_EVERY + 1 s; after the turn, the block's frame alone,
  then the call (the budget META present when Jev first sees it). A second exchange end (the same result again)
  while another turn holds the row: judged nothing twice, and the mark is not in the store; after it, the row and
  its mark land in one frame.
- `a_loop_judgments_first_block_waits_for_the_running_turn`: likewise for loop.v1's first block.
- **Plants:** the mark's `put_meta` back at prepare: FAIL (a frame inside the turn, 59 vs 58). The block written at
  once (`reserve_between` never waits): both FAIL (12 vs 11; 59 vs 58). The mark read from the store alone: FAIL
  ("judged twice", 2 vs 1). `newest_each` passing every mark: FAIL.
- Three existing tests assumed the block inside the turn and were changed: `tests_sink_between` (a first judgment
  warms the budget, then the original scenario), `tests_judge::a_tiny_day_limit_pauses_shadow_with_one_row` (waits a
  quiet stretch, not 300 ms), `tests_judge_surfaces::judge_list_filters_by_pack_session_and_time` (the three
  judgments now wait for the first block together and settle in any order, so a session's rows are found by session,
  not by position).

## 5. ju99: the guard held (42a7b615)

`tests_sink_busy::a_turn_that_never_ends_gets_a_frame_beside_it_once_a_busy_stretch`: quiet 500 ms, quiet bound 1 s,
busy bound 3 s; one turn held 14 s while 8 judgments settle every 100 ms (1,128). At most ⌈14 / 2⌉ = 7 frames inside
it: **6** here (5 under load). **Plant:** `since` returns the pass's start: **36**, FAIL.

## Found while measuring: `Blobs::put`'s temporary name (2c2ff8fc)

A 20 s burst logged `judge: the state's blob was not written; not judged ... No such file or directory`. Two puts of
the same bytes at once shared `.<digest>.tmp-<pid>`, so one renamed the other's file away. It predates this branch
(two shadow points' puts of one state). Each write now gets its own name (a counter). Test
`blobs::tests::the_same_blob_put_at_once_from_many_threads_is_stored_once`; with the shared name planted back,
ENOENT, FAIL. `put_derived` has the same shared name and the same race; I did not change it.

## Measures, A B B A in one session

A = main 57f265f2, B = this branch at 2c2ff8fc, both `--profile release-thin` (theseusd, theseus-sim,
theseus-index), frozen in /tmp/bins/{A,B}. (B's theseusd embeds the cockpit build; A's worktree had none.)

**`bench turn --judge --runs 30`** (`/tmp/bins/X/theseus-sim bench turn --judge --runs 30 --theseusd /tmp/bins/X/theseusd`):

| run | off plain p50 | loop plain p50 | packs plain p50 | packs tool p50 | judge frames before · after · between (loop/packs, each kind) | standalone mark frames |
|---|---|---|---|---|---|---|
| A | 12.5 | 12.9 | 13.8 | 34.6 | 0 · 3 · 0 | 6 a judged arm |
| B | 12.1 | 13.0 | 13.0 | 35.2 | 0 · 0 · 0 | 0 (in sink frames) |
| B | 13.2 | 13.1 | 13.6 | 34.2 | 0 · 0 · 0 | 0 |
| A | 12.6 | 12.8 | 13.1 | 33.4 | 0 · 3 · 0 | 6 |

Judge frames before an answer: 0 in all four. A's 3 "after" frames per kind were categorize's mark frames landing
after a measured turn's answer, inside the bench's window; B has none, its marks ride in sink frames. Walls are
equal within noise on this disk.

**The 180 s burst** (`perf::burst`, three sessions back to back, every pack as wired), turn wall p50 by 15 s
window, ms:

| window (s) | A | B | B | A |
|---|---|---|---|---|
| 0-15 | 38.9 | 39.7 | 39.1 | 37.9 |
| 15-30 | 73.3 | 76.0 | 74.0 | 73.0 |
| 45-60 | 113.5 | 111.4 | 115.0 | 109.0 |
| 90-105 | 153.1 | 143.4 | 150.4 | 152.9 |
| 165-180 | 216.2 | 206.8 | 189.8 | 208.5 |
| all (p50 / p95) | 111.4 / 214.7 | 112.3 / 207.6 | 113.1 / 197.6 | 110.1 / 208.8 |
| judge frames over the burst | 606 | 106 | 440 | 508 |
| shutdown: judgments flushed, ms | 2,586 in 825 | 3,539 in 475 (7 frames, 981 blob syncs) | none left | 6,886 in 1,994 |

The wall climbs in every run as each session's history grows (a compile cost, not the judge's); A and B track each
other on this disk. The stop's cost per judgment is what moved: A 0.32 and 0.29 ms a judgment (frames of 32, two
syncs a blob: about 81 and 216 frames by its code), B 0.13 ms in 7 frames. **What a beginning turn waited on** I did
not measure at the daemon (it would need a trace field); the order test of step 1 proves the guard no longer covers a
blob's syncs. On the owner's disk (fdatasync about 7 ms) each avoided sync is worth about 7 ms, so the counts above
are the prediction to check.

## The live check (the maintainer's, on the owner's machine, A B B A against main)

```bash
# Builds, frozen (main as A, this branch as B).
git worktree add /tmp/sf-main main && (cd /tmp/sf-main && scripts/build.sh --profile release-thin)
mkdir -p /tmp/sf/A /tmp/sf/B
cp /tmp/sf-main/target/release-thin/{theseusd,theseus-sim,theseus-index} /tmp/sf/A/
scripts/build.sh --profile release-thin && cp target/release-thin/{theseusd,theseus-sim,theseus-index} /tmp/sf/B/

# 1. The 180 s burst, A B B A (the measure is this branch's test; it drives either theseusd).
for x in A B B A; do
  THESEUS_BURST_THESEUSD=/tmp/sf/$x/theseusd THESEUS_BURST_SECS=180 THESEUS_BURST_DIR=/tmp/sf/burst-$x-$RANDOM \
    cargo test -p theseus-sim --bin theseus-sim -- --ignored --nocapture burst
done
```
Expect: B's per-window p50 near main's earlier sink (the review's 142 to 156 ms), A's near 269 to 280 ms. The
`shutdown:` line is the stop with a backlog: B's `ms` well under A's for the same `judgments` (the review: 2,827 in
11,452.8 ms), with `frames` about judgments / 512 and `blob_syncs` about the staged blobs + 1, `turns_running=0`.

```bash
# 2. After the stop, every judgment present: restart a kept scratch daemon (its fake `op` on PATH, as the
#    burst's rig starts it), then stop it and count the judge.call rows in its WAL (`judge log` pages at 500).
d=/tmp/sf/burst-B-<n>   # the dir of a B run above
PATH=$d/bin:$PATH OP_SERVICE_ACCOUNT_TOKEN=bench-not-a-token \
  /tmp/sf/B/theseusd --config $d/config.toml --socket $d/sock --state-dir $d/state >>$d/restart.log 2>&1 &
theseus --socket $d/sock judge log -n 5      # it serves, and reads the newest rows
theseus --socket $d/sock shutdown
grep -aoh '"kind":"judge.call"' $d/state/store/wal/* | wc -l
grep -c 'not written' $d/theseusd.log $d/restart.log
```
Expect: the count is the rows the sink wrote during the burst plus the stop line's `judgments`, and no `not
written` line. **Run here on B** (a 30 s burst, kept in /tmp/bk): the stop wrote 10,353 judgments in 1,157.5 ms,
21 frames, 2,635 blob syncs, `turns_running=0`; the restarted daemon served `judge log`; the WAL holds 12,017
`judge.call` rows (68 sink frames during the burst, about 24 rows each, plus the 10,353); no `not written` line.

```bash
# 3. The turn bench with the judge.
for x in A B B A; do /tmp/sf/$x/theseus-sim bench turn --judge --runs 30 --theseusd /tmp/sf/$x/theseusd; done
```
Expect: 0 judge frames before an answer in every arm; B: 0 "after" (A: categorize's marks), no
`[meta:judge.categorize.*]` frame of its own; p50s within noise.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before every commit; each suite run had exactly the 33
known L1 failures (theseus-sandbox's 12 contract tests, `spawn_100`, theseusd's sandbox tests: root with no job
cgroup, theseus-pv6i), and the phases after the suite (protocol types, `bench turn --check --runs 5 --burst 0`:
5 and 9 frames, `cargo deny --offline check`) passed each time. Exceptions:
- Step 2's first gate: 10 more theseusd job tests failed (job_approval, mcp_server, reaping, push, outbox,
  job_latency) because the session's disk allowance was spent (1.1 GB left; incremental caches were 17 GB). I
  removed `target/debug/incremental` and reran: only the 33. Not code.
- The last gate (before 52320342): one more,
  `tests_m3::a_jobs_raw_output_stays_while_its_wrapper_lingers_and_goes_when_it_ends`, at tests_m3.rs:1819
  (`assert!(!out.exists())`): after `wrapper_lives` turned false the sweep still classed the job `running`
  (`SpoolSweep { removed: 0, kept: 1, kept_by: {"running": 1} }`). It passed 5 of 5 alone. It is in the spool and
  sweep, which this branch does not touch: a race between a wrapper's exit and the sweep's own liveness read, for
  someone to look at.
- Under load (nice 19 beside four busy loops at nice 0, three runs of `tests_sink` and `tests_categorize`): 20 of 20
  each time.
- No failure of a known flaky test in any run.

Suites run besides: `tests_sink*`, `tests_categorize`, `tests_judge*`, `tests_reserve`, `judge::`, `memory_pass::`
(81 tests, green); theseus-core's whole suite inside every gate with `TZ=America/Phoenix`.

## Left, uncertain, and for the owner

- **The first reservation after a start always waits for a gap** (`needs_frame` is true until the budget is loaded),
  so a busy daemon's shadow judgments wait up to the quiet bound (120 s; the busy bound, 600 s, with a turn always
  running) before their first call; continue.v1's, dispatched mid-turn, waits for its turn's end when it needs a
  block. Shadow only; but it is a behaviour change.
- **The gate's and the citation check's blocks are still written at once** when the block ahead did not cover them
  (rare after the first): a frame inside a turn in that case. An option: give the gate `reserve_beside`'s shape (its
  calls are spawned, and its notice must not wait); I left it, since it widens otny's accepted gap.
- **Two blocks' rest after a crash**, from writing ahead; spend.rs's doc says so.
- The pre-write covers the judgments queued when the writer peeks; one that joins the frame after it has its blob
  written inside the guard.
- `syncfs` vs threads: threads chosen (measured above); the owner may prefer otherwise on their disk.
- `put_derived` shares the temporary-name race fixed in `put`.
- The stop still does not wait for a running turn (unchanged; now visible as `turns_running`).
- Docs for the maintainer: Part III's item; status.md; the spec's or `docs/design/m5-judgment.md`'s budget section
  (§2.6: a block is no longer always "a frame of its own"; written ahead in sink frames; two blocks' rest after a
  crash) and the categorize mark (§2.4/28b: written beside its row, not at dispatch). theseus-core's AGENTS.md got
  one sentence (42a7b615).
