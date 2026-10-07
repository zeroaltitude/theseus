# CLOUD_REPORT: bench-rows (theseus-w7dk, theseus-ma8r)

Branch `cloud/20261006-bench-rows`, from main at 57f265f2 (store format 23, not bumped).
Commits: `fd1f578f` (w7dk), `5cb3735f` (ma8r).

## 1. theseus-w7dk: each run's line (`fd1f578f`)

**Found.** `bench turn` printed summaries only. The daemon had no per-frame time, and the WAL has none.

**Changed.**
- theseus-store `frame_times.rs` (new) and `store.rs`:
  - the writer reads its clock twice a batch (before the write, after the index commit) and notes each answered frame in a 64-entry ring: first position, batch time in µs, and when it was answered;
  - `WalStore::slowest_frame_since(Instant)` reads it, next to `frames_written_here`;
  - a frame's time is the batch's: write + fdatasync + redb index commit. One sync commits a batch, so its frames share the time.
- theseus-core `store.rs` and `turn.rs`:
  - the turn's trace gains `attrs.slowest_frame = {first, us}`, the slowest frame answered since the input arrived. That covers admission's frames, which precede the daemon's `elapsed_ms`. A neighbour session's frame counts too, which is what a stall is;
  - `turn.rs` +8 lines, to 3,509 of 3,523. The trace's closing attrs moved into a `TurnRunner::end_attrs` helper because `finish` sat at clippy's 100-line limit;
  - the core golden gains `"slowest_frame":{"first":#,"us":#}` in each turn's trace line (`THESEUS_GOLDEN=write`, 17 lines, nothing else changed).
- theseus-sim `perf/runs.rs` (new) and `perf.rs`: `Kind.runs: Vec<Run>`, one line per run:
  - the line shows wall, daemon ms, frames, and the slowest frame's ms and its records;
  - a run over 2× the kind's p50 is flagged `<- over 2x the p50`;
  - `--json` carries the same per run;
  - the frame-count check is unchanged, and the history's columns are unchanged.

**Format rule.** Trace attrs are a free-form map inside `TurnEnded`'s trace, not a typed field. No `MANIFEST_FORMAT` bump, no layout sample. (An older build's trace simply lacks the key; the bench says "slowest frame not named".)

**Gap.** The turn's last frame, which carries the trace, is written after the attr is read, so it is never a candidate. In the outliers below the slow frame was an earlier one.

**Proof.**
- `cargo test -p theseus-store frame_times`: 2 pass. They are `the_slowest_frame_since_a_mark_is_the_one_made_slow` and `the_ring_forgets_the_oldest_frames`.
  - The test-only delay seam is `Inner::commit_delay_ms`: it sleeps once inside the writer's timed stretch. It is a new seam; the store had none, only `fail_next_sync`.
- `cargo test -p theseus-sim runs::`: 4 pass. They check the shape of a run's line, the lookup of the named frame, an unfound frame, and an old daemon.
- **Plant:** `slowest_since` reduces to "the first" (`.reduce(|a, _b| a)`). `the_slowest_frame_since_a_mark_is_the_one_made_slow` fails ("the slow frame's first record"). Restored, `touch`ed, and the test passes again.
- Gate (below).

## 2. Measured here (4 cores, no other load; debug builds as the gate builds them)

**30 runs each, quiet** (`theseus-sim bench turn --runs 30 --burst 0`):
- plain: p50 39.4 ms, max 55.7, no run over 2× p50. Slowest frames were 1–2 ms.
- tool-call: p50 95.3 ms. Two outliers, both with their time in one frame:

| run | wall ms | daemon ms | slowest frame | which frame |
|---|---|---|---|---|
| 16 | 407.0 | 202 | 122.8 ms | `[completion, action, execution, ledger:action.succeeded, node, ledger:provider.call]` |
| 17 | 283.0 | 88 | 182.2 ms | `[ledger:execution.queued, execution, ledger:execution.running]` |

- Run 17's frame is the admission frame, before the daemon's own clock starts. That is why wall minus daemon was 195 ms there.
- On this disk the frames' typical time is about 1.3 ms, so these are fdatasync/commit stalls of 100× the median.

**Under load.**
- The recipe taken literally (bench at `nice 19`, four busy loops at nice 0) starves the daemon too: p50 2,598 ms plain. It tells nothing about outliers.
- A gentler version (two busy loops, bench `nice 5`, 3×30 runs): plain p50 about 48–51 ms, tool about 117 ms.
  - The one run over 2× p50 was plain run 14: wall 199 ms, daemon 43 ms, slowest frame 144.6 ms, the `execution.queued` admission frame.
  - So 3 of 3 outliers found here were one slow frame, none outside the frames, none in the daemon's own compute.
- For outliers and frames under load I used a **measurement-only, uncommitted** patch of the bench. It drops frames made only of `ledger:memory.*` rows from the count (`/tmp` binary, not in the branch). Unpatched:
  - the tool-call kind failed the frame-count check in 4 of 4 loaded runs;
  - the plain kind also failed in 3 of 3 runs on a nearly full disk;
  - in these the WAL held a frame of `memory.labeled`/`memory.gated` rows, a memory-pass frame that landed in the measured window (before the turn or in its quiet stretch). The count check trips; the bench's own doc says pass frames are written only between turns;
  - the trace's own count was right.
  - **Finding for the owner.** This is the bench's load fragility, not this change's, and I left it (the brief says keep the check as is). A bench-side exclusion of `ledger:memory.*`-only frames would fix it.
- On the VM's near-full disk the daemon also writes a `ledger:disk.low` frame in the window: free space under about 5 GB trips it. That is the VM, not the code. I deleted `target/debug/incremental` and a baseline target several times to keep clear of it.

**What to read in the gate's log (owner's machine).**
- The turn step prints `each plain run…` and `each tool-call run…` blocks. A line ending `<- over 2x the p50` is an outlier: look at its `daemon` vs `wall`, and its `slowest frame … ms [records]`.
  - A big slowest-frame time (tens to hundreds of ms): a stalled frame (fdatasync, or the index commit). The records name which frame.
  - A small one with a long wall: the time is outside the frames (scheduler, a first-of-kind cost). Compare `daemon` and `wall` to place it.
  - `wall − daemon` large with a frame of about that size: an admission frame, which precedes `elapsed_ms`.
- The gate's turn step runs 5 runs, so only a stall that lands in those 5 shows.

**FAST: before and after, A B B A A B B A** (30 runs each, `--burst 0`, binaries from the same checkout, built the same way, with `theseus-index` beside; A = main, B = this branch):

| | plain wall p50 | plain daemon p50 | tool wall p50 | tool daemon p50 | frames (p50) |
|---|---|---|---|---|---|
| A (4 runs) | 41.5 ms | 32.8 | 104.7 | 92.8 | 5 and 9 |
| B (4 runs) | 42.4 ms | 34.0 | 109.5 | 96.8 | 5 and 9 |
| control, A vs a copy of A (ABBAABBA) | 42.8 vs 41.4 | 34.5 vs 32.8 | 107.5 vs 105.0 | 95.0 vs 92.8 | |

- Frames are identical (5 and 9) in every run.
- B reads +0.9 ms (2%) plain and +4.8 ms (4.6%) tool.
- The A-vs-A control differs by 1.4 and 2.5 ms, so the gap is within the noise of this VM at the resolution of 4 runs of p50.
- I can't claim it is zero. The code adds two `Instant::now()` and one uncontended lock per batch, and a small lock plus a 2-field object per turn.
- An earlier A/B (not matched builds: the main target had `theseus-index` and workspace features, the baseline didn't) read +2.8 ms plain, which this matched pair did not reproduce.
- Please re-measure on the owner's machine (the same `A B B A` with `theseus-sim bench turn --runs 30`).

## 3. theseus-ma8r: restore before cancel (`5cb3735f`)

**Found.** The cause is as the brief says. `run` called `cancel_phase` before `restore_phase` on the same rig; cancel's runs start jobs through turns.

**Changed.** `lifecycle.rs`: `run` runs restore before cancel (seed already runs in the cold phase). The module doc gains an **Order** paragraph: a phase that writes into the rig runs after the rows that read it.

**Restore row, before and after** (`--phases cold,shutdown,kill,swap,restore,seed,cancel --runs 10`, run twice each):

| | restored store | restore p50 | copy phase |
|---|---|---|---|
| old order (main) | 836,952 B and 15 sessions; 838,640 B and 15 | 55.7 and 53.0 ms | 11.3 and 10.6 ms |
| new order | 204,388 B and 5 sessions; 203,582 B and 5 | 27.6 and 23.6 ms | 6.0 and 3.4 ms |

This reproduces the brief's 0.85 MB/15 and 0.22 MB/5 on this VM.

**Test.** `tests/sim.rs::the_restore_rows_store_is_the_same_with_the_cancel_row_selected` runs the real bench (the `theseusd` beside `theseus-sim`; it asserts the binary exists) with `--phases restore` and `--phases restore,cancel --runs 1`. Segments and sessions must be equal and the WAL bytes within 25%.
- Measured: 6,243 vs 6,246 B alone; in the gate run 6,648 vs 6,239 B (a 6% spread from id and clock lengths), hence the 25% bound.
- Why this test: the order is a straight-line sequence in `run`, so only a real run can see it, and the numbers are the ones the row prints.

**Plant.** The old order (`cancel_phase` before `restore_phase`): the restore row's WAL is 135,855 B and 3 sessions, against 6,243 B and 0 sessions. The test fails (`the restore row's WAL is … bytes alone and … with the cancel row selected`). Restored and `touch`ed; the test passes again.

**For the maintainer's history note.** The bench history's restore row moved at d279767f (about 139 → 233 ms) and moves back at `5cb3735f`.

## Gate

I ran `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree both commits make, with `CARGO_INCREMENTAL=0` (the disk was full otherwise). I committed in two steps; the second commit's tree is a subset, and each step is independent of the other.
- fmt, shape, features, clippy, cockpit, test build and reader rule: ok.
- Suite: 3033 tests, all pass except the known L1 failures (the VM runs as root, theseus-pv6i). They are 20 `theseus-sandbox` tests (`bench spawn_100`, 19 `contract` cases) and 13 `theseusd::sandbox` tests (two of those 30 s timeouts), 33 in all. This run's other failures were mine and are fixed in the commits: the core golden (`slowest_frame`) and my restore test's 5% bound (now 25%).
- The phases after the suite: protocol types (`cockpit/src/protocol.gen` unchanged), the turn bench (5 runs `--check`: 5 and 9 frames, ok), and `cargo deny --offline check` (advisories, bans, licenses, sources ok).
- The lifecycle and jobs benches were skipped, as `THESEUS_GATE_NO_BENCH` does.

## Live check (the maintainer's, on the owner's machine)

```
theseus-sim bench turn --runs 30
theseus-sim bench lifecycle --phases restore,cancel --runs 10
```
- The first should print a per-run block under each kind, with `slowest frame … ms [records]` on each line. Any line over 2× the p50 is flagged.
- The second should show the restore row's input at the pre-cancel store: a few KB of WAL when only these two phases run, and about 0.2 MB and 5 sessions with the full `cold,shutdown,kill,swap,restore,seed,cancel` set. The old order gave 0.84 MB and 15.

## Left, or uncertain
- The overhead in section 2 (+0.9 and +4.8 ms against a 1.4 and 2.5 ms control) needs the owner's machine to settle.
- The bench's frame check breaks under load (memory-pass frames), as described in section 2. I did not fix it.
- Docs to update at review: `docs/spec` Part III (the item for this step) should record the `slowest_frame` trace attr and the per-run lines. `crates/theseus-sim/AGENTS.md`'s `bench turn` paragraph should mention them, and the lifecycle row's order note. I didn't touch the spec or `docs/status.md`.
