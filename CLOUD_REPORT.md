# CLOUD REPORT: cloud/20261006-judge-sink

judge-turn-cost's sink, fixed (theseus-s1am, theseus-ych4, theseus-3bl9). Started 17:39 UTC, report at about
20:30 UTC (deadline 20:39). Base: judge-turn-cost's head `54de79d` plus this branch's task commit `adae427`, store
format 22. No format bump (nothing stored changed), no new package, no protocol or config change.

| Step | Commit | Subject |
|---|---|---|
| 1 | `e04d694` (+ `0afe76f`) | judge: the sink keeps one clock for a backlog pass (theseus-s1am) |
| 2 | `a760b09` | judge: a clean stop writes every settled judgment before the store closes (theseus-ych4) |
| 3 | `44f0a42` | judge: the adoptions test tells the warm read's wait from its sleep (theseus-3bl9) |
| 1' | `0afe76f` | judge: the backlog test's bound is measured against the sink's own rate (theseus-s1am) |

## Step 1: one clock per backlog pass (theseus-s1am)

### What I found

As the review said: `sink::run` called `Turns::between(Instant::now(), …)` once per batch, so every 32-row frame
measured its quiet bound (120 s) and busy bound (600 s) from a fresh clock. On a daemon whose turns are never a quiet
stretch (500 ms) apart, every frame waited out a whole quiet bound: 32 rows per 120 s. The judgments also lived in an
mpsc channel plus a batch held in the task's locals, so nothing outside the task could reach them (step 2's need).

### What I changed (`crates/theseus-core/src/judge/sink.rs`, `judge/mod.rs`)

- `sink::Queue`: the settled, unwritten judgments (a `VecDeque` under a std Mutex, a `Notify`, a sender count). The
  recording's `Channel` pushes onto it (never blocks); the service holds it (`JudgeService.queue`); the writer task
  holds it and the service by `Weak`, as before. The writer ends when the service is gone or no sink is left (the
  count, so a `Channel` built and dropped in `built()`'s race never ends it).
- **Passes.** A pass begins when a judgment lands on an empty queue (then the old window: up to `FLUSH_EVERY` for
  32) and ends when a frame leaves the queue empty. Every frame of the pass waits through `Turns::between` with
  `since` = the pass's start, so once a pass is past the quiet bound the backlog goes in the next gaps between
  turns, frame after frame, each frame still waiting for no turn to run (the handshake is unchanged). With no turn
  running it drains as fast as frames append, as main's sink did.
- **One guard on the busy bound:** `since(start, t)` is the pass's start, but never older than `quiet_bound` before
  now. Without it, a backlog that never empties (judgments arriving faster than gaps allow) would pass the busy
  bound at 600 s and from then on write every frame beside running turns, gaps or not. With it, a frame is written
  beside turns only after `busy_bound - quiet_bound` (480 s) of turns with no gap. Design choice for the owner: the
  pass's busy bound is effectively 480 s after the quiet bound, not 600 s from the backlog's start.
- **Frame size kept at 32:** a turn that begins while a frame is appended waits for that append (`Turns::begin`), so
  the frame's size bounds that wait; a backlog is many 32-row frames back to back in a gap, each re-checking for a
  turn.
- `JudgeService::unwritten()` (the queue's length), and test-only `settle` and `sink_timing` (a test's shorter
  bounds; the build uses `memory_pass::Timing::default()`, as before).

### How I proved it

- `tests_sink_backlog::a_backlog_drains_in_the_gaps_once_its_pass_passes_the_quiet_bound`: the sink's quiet bound
  set to 3 s. First 1,500 judgments with no turn running, timed (the rate of a sink that writes as they land, main's
  rate, on this machine and load). Then 1,500 more settled 300 at a time while turns run 300 ms each, 200 ms apart
  (never a quiet stretch). Each turn's window checks `store.last_position()` at its start and its end: equal, so no
  frame lands inside a turn. The backlog must be written within `quiet bound + 3 × the no-turn time + 3 s`.
  - Unloaded: no-turn 224–238 ms; beside turns 3.9 s; bound 6.4–6.7 s.
  - **Under load** (nice 19, four busy loops at nice 0), 5 runs of the sink and ladder tests: all pass. No-turn
    6.8–7.9 s, beside turns 15.4–17.0 s, bound (at 2×, before I widened it) 19.7–21.7 s. The margin at 2× was thin
    (about 15%), so I widened the bound to 3× (`0afe76f`); the bug's sink is about 140 s here either way.
  - My first version had a fixed 8 s bound; it failed every loaded run (CPU starvation, not the clock: the sink's
    own writes took 7 s with no turn at all). That is why the bound is now measured.
- **Planted revert** (the clock taken per frame again: `between(Instant::now(), …)`): fails, "128 of 1500 written
  after 6.9 s, past 6.7 s" (4 frames, one a quiet bound). Restored and touched; `git status` clean.
- The branch's `tests_sink_between` still passes.

## Step 2: a clean stop flushes the sink (theseus-ych4)

### What I changed

- `Queue::flush` / `JudgeService::flush_sink()`: under the writer's lock (the writer's own frames take it too, so
  the two never interleave), mark the sink closed and write every queued judgment in frames of 32. After it, a frame
  the writer takes is dropped with a debug line (a row after the stop's last checkpoint would be replayed by the
  next start; the core's `ledger_unless_closed` does the same).
- `Core::finish_stop` (`outbox.rs`), the one clean-stop path (shutdown method, SIGINT, SIGTERM, restart onto a
  changed note; theseusd calls it on both the socket and the stdio paths): `flush_judgments()` after the posts settle
  and before `close_late_rows` and the last checkpoint, on `theseus_store::blocking`, with an info line (count, ms)
  when it wrote any and a stop phase `judgments written`. A stop with nothing queued writes and waits for nothing.
- **What a SIGKILL loses** (accepted, said in the sink's module doc, not fixed): the queue. On a quiet daemon a
  window of rows (2 s); on a busy one the whole backlog waiting for a moment between turns (up to the quiet bound's
  worth and more). Their spend is not lost: the budget's blocks are written before the calls.
- theseus-core's AGENTS.md, the judge paragraph: two sentences after "a press finds it in `pending` meanwhile" (the
  backlog's clock, the stop's flush, the SIGKILL loss, the two tests). That is all I changed there; the maintainer's
  resolve.py for judge-reads' text needs to keep them.

### How I proved it

- `tests_sink_flush::a_clean_stop_writes_every_settled_judgment_before_the_store_closes`: a turn held running, 200
  judgments settled; none written after the window; `finish_stop()`: unwritten 0, 200 rows, **7 frames** (200/32
  rounded up; the store's `frames_appended` across the stop), a judgment settled after the stop is never written,
  and a new core on the same store reads all 200.
- **FAST, the stop's cost:** the whole `finish_stop` with 200 pending took 29 ms (debug build, this VM; 7 frames at
  this disk's fdatasync p50 of 0.2 ms, plus 200 rows' encoding and their sentences). With nothing pending the flush
  is a lock and an empty take. Nothing was added to the start path.
- **Planted revert** (the flush's call removed from `finish_stop`): fails, "the stop took every judgment". Restored
  and touched; `git status` clean.

## Step 3: the adoptions test tells a wait from a sleep (theseus-3bl9)

- `tests_ladder_unread::the_warm_reads_adoptions_wait_for_a_moment_between_turns` holds its turn 2 s (4 × the warm
  read's 500 ms sleep) with no adoption written, then, after the turn ends, all three, no sooner than a quiet
  stretch after its end.
- **Planted revert** (in `rpc/packs.rs`, the warm read's `turns.between(…)` replaced by a 500 ms sleep): the **old**
  test passes (the defect confirmed), the new one fails, "no adoption while a turn runs, 2s past the warm read's
  sleep". Restored and touched.

## Step 4: "between turns" kept

- `tests_sink_between::a_judgments_frame_waits_for_the_running_turn_to_end` passes at every commit.
- **Planted revert** (the sink's between-turns wait replaced by `None`, writing at once): `tests_sink_between`,
  `tests_sink_backlog` and `tests_sink_flush` all fail. Restored and touched.
- `bench turn --judge --runs 10` on the **debug** build at the head (`target/debug/theseus-sim bench turn --judge
  --runs 10 --theseusd target/debug/theseusd`), wall p50 / p95, judge frames before the answer · after · between:

  | arm | turn | frames | p50 | p95 | judge frames |
  |---|---|---|---|---|---|
  | off | plain | 5 | 38.0 ms | 41.5 ms | 0 · 0 · 0 |
  | off | tool-call | 9 | 83.3 ms | 95.4 ms | 0 · 0 · 0 |
  | loop | plain | 5 | 40.2 ms | 46.9 ms | 0 · 1 · 0 |
  | loop | tool-call | 9 | 91.4 ms | 106.5 ms | 0 · 1 · 0 |
  | packs | plain | 5 | 41.2 ms | 54.2 ms | 0 · 1 · 0 |
  | packs | tool-call | 9 | 88.3 ms | 100.8 ms | 0 · 1 · 0 |

  **0 judge frames before any answer.** The one "after" per kind is categorize's META mark, as in judge-turn-cost's
  report. loop: 6 judge frames over 20 turns (4 after the last; the sink's frames 32, 32, 32, 16 rows), packs: 8 (6
  after the last). The first turn submitted at serving: 0 `pack.mode` frames before its submit, its answer, or after.
  Debug p50s carry the debug build's cost; read the release ones in the live check. The gate's `bench turn --check`
  (debug, judge off) after each step: frames 5 and 9.
- `bench turn --judge --runs 30` on **release-thin** at the head (`scripts/build.sh --profile release-thin`, then
  `target/release-thin/theseus-sim bench turn --judge --runs 30 --theseusd target/release-thin/theseusd`), two runs,
  wall p50 / p95, judge frames before the answer · after · between:

  | arm | turn | frames | run 1 | run 2 | judge frames (runs 1, 2) |
  |---|---|---|---|---|---|
  | off | plain | 5 | 12.3 / 13.7 | 12.3 / 14.9 | 0·0·0, 0·0·0 |
  | off | tool-call | 9 | 33.0 / 39.1 | 31.4 / 36.8 | 0·0·0, 0·0·0 |
  | loop | plain | 5 | 13.3 / 15.5 | 13.4 / 15.1 | 0·3·0, 0·3·0 |
  | loop | tool-call | 9 | 32.7 / 36.4 | 31.8 / 36.2 | 0·3·0, 0·3·0 |
  | packs | plain | 5 | 13.6 / 15.5 | 14.1 / 16.3 | **1**·2·0, 0·3·0 |
  | packs | tool-call | 9 | 34.4 / 38.2 | 33.1 / 39.1 | **1**·2·0, **1**·2·0 |

  - **loop: 0 judge frames before any answer** in 120 measured turns. Its 16 frames per run: 10 after the last turn
    and 6 within the turns, which are exactly its 6 categorize marks (`[meta:judge.categorize.<session>]`); so every
    sink frame (`[judge.call ×32, meta:judge.budget]` ×9, ×22 ×1) landed after the last measured turn.
  - **packs: 1 frame before an answer** in 3 of 4 kind-runs. Its 23 frames: 16 sink frames, 6 categorize marks and
    **one lone `[meta:judge.budget]`**: the shadow budget's block frame, which `reserve` writes at a judgment's
    prepare when a reservation crosses a $0.01 block (at 30 runs the fake's prices cross one; at 10 runs they did
    not, in judge-turn-cost's report and in my debug run). 17 frames landed after the last turn, so 6 were within
    the turns: by count, at most the 6 marks and the block frame can be the in-turn ones, and none of the 16 sink
    frames, though the bench does not name which frame sat before which answer. The block frame and the marks are
    not the sink: judge-turn-cost's report left `reserve`'s block frame to judge-tests (its area) and named it as
    able to land inside a turn. I did not change either. If "0 before the answer" is to hold for every judge frame,
    `reserve`'s block frame (and categorize's mark) need the same between-turns handling: an issue for the
    maintainer.
  - FAST: judged p50s against off: plain +1.0 to +1.8 ms, tool-call -1.2 to +1.7 ms (within this VM's noise for
    tool-call). The bench's fdatasync estimate: loop 1.6–1.8 ms a turn, packs 1.9–2.1 ms (mostly the judged states'
    blobs, as judge-turn-cost found).

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit: fmt, shape, features, clippy,
cockpit, test build, deny (with the fetched advisory database), reader rule all pass; the suite fails only on the 33
known L1 tests (theseus-sandbox's contract tests and `spawn_100`, theseusd's sandbox tests: a root daemon's L1,
theseus-pv6i). The phases after it, run by hand: protocol types unchanged (`cockpit/src/protocol.gen` clean), `bench
turn --check --runs 5 --burst 0`: frames 5 and 9, ok. Lifecycle and jobs benches skipped (NO_BENCH).

At the head (`0afe76f`): the same, the suite 2,837 run, 2,804 passed, 33 failed (the 33 known L1), 21 skipped;
protocol types clean; `bench turn --check`: frames 5 and 9, ok.

One gate run (before step 2's commit) failed 106 tests at once: the session's disk allowance was spent (18 GB of
cargo's incremental cache). I deleted `target/debug/incremental` and reran the gate: only the 33 known. No flake on
the list of known timing tests showed up in any of my runs.

## The live check (the maintainer's)

1. Build the head (`scripts/build.sh --profile release-thin`), then the bench:
   `target/release-thin/theseus-sim bench turn --judge --runs 30 --theseusd target/release-thin/theseusd`.
   Expect 0 judge frames before every answer (each arm, each turn kind) and judged p50s near the off arm's (the
   judge-turn-cost report's release p50s: plain 9–11 ms, tool-call 23–26 ms).
2. A scratch daemon with the judges on and the stand-in model (`theseusd --config <c> --socket <s> --state-dir <d>`,
   `[judge] enabled = true` at a fake Jev, as the bench's `packs` arm configures it), driven with back-to-back turns
   for 3 minutes (e.g. `for i in $(seq 1 2000); do theseus --socket <s> send "turn $i" >/dev/null; done`, or the
   bench's burst). Then `theseus --socket <s> judge log` and the label counts: judgments written should keep near
   settled. Before the quiet bound (the first 120 s) rows may lag, since no gap is a quiet stretch; after it the
   backlog is written in the next gaps. Against judge-turn-cost's sink (32 written of 1,551 at 180 s), expect nearly
   all settled written by 180 s. Settled is what Jev answered (its `seen()`, or the daemon's
   `theseus.judge.*` metrics); written is the `judge.call` rows.
3. With judgments pending (stop within 2 s of a burst, or while turns still run): `theseus --socket <s> shutdown`.
   The daemon's log says `stopping: the judge's settled judgments are written` with the count and ms, and its stop
   phases include `judgments written`. Start it again on the same state dir: `judge log` shows every one of them (the
   count settled before the stop equals the `judge.call` rows after it).
4. Optional, the accepted loss: the same with `kill -9` instead of the shutdown: the queued judgments are gone,
   nothing else is.

## Left, uncertain, and for the owner

- The release bench's packs arm showed one judge frame before an answer in 3 of 4 kind-runs; by the frame counts it
  is `reserve`'s budget block frame or a categorize mark, not the sink (see step 4). Proposed follow-up issue:
  write the shadow budget's block frame (and categorize's mark) between turns as the sink now does, or beside the
  turn's own frames; neither is in this task's scope ("Leave alone: everything outside the judge sink").
- The pass's busy bound guard (480 s of gapless turns before a frame goes beside them) is my choice; 600 s from the
  backlog's start would let a never-empty backlog write beside turns forever after.
- A stop now writes judgments a turn's run settled within the stop; judgments still in flight to Jev at the stop are
  not settled and are not written (as before).
- Docs: Part III's item for judge-turn-cost should say the sink keeps one clock a backlog pass and a clean stop
  flushes it (theseus-s1am, theseus-ych4); theseusd's AGENTS.md "Every clean stop is one path" could name the judge's
  flush beside the checkpoint.
