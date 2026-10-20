# CLOUD_REPORT: timing-flakes (theseus-1b2zm, theseus-nyczg)

Branch `cloud/20261019-timing-flakes`, base `08336e8`. VM: 4 cores, root. The session's container was reset near the end
(build tree and the long report draft lost); what follows is rebuilt from my notes. Commits are all pushed.
No retry, longer timeout, larger bound or flaky-list entry was used.

## 1. Key table (base vs fix under load; N is below 50 because a starved run takes 5 to 30 s here)

L-A = brief's recipe (test at nice 19 beside 4 busy loops at nice 0; a starvation load on 4 cores).
L-B = the whole theseus-core test binary looping at 8 threads, test at nice 0.

| # | test | load | base | fix | base's failing assertion |
|---|---|---|---|---|---|
| 1 | checkpoint `a_slow_verdict_never_holds_a_loop…` | L-A | 20/20 | 20/20 | both: harness `no the task's turn in 20 s` (starved) |
| 1 | same | L-B | 0/20 | 0/20 | never red at base here |
| 2 | watch `words_match_as_text_or_a_pattern…` | L-A | 15/30 | 0/30 | `watches.get(id).is_none()` |
| 3 | horizon `a_met_watch_ends_done…` | L-A | 5/20 | 0/20 | `next_due_ms: None` |
| 4 | digest `the_slot_waits_behind…` | L-A, L-B | 0/20, 0/30 | 0/20, 0/30 | never red; shown by plant |
| 5 | store `the_slowest_frame_since_a_mark…` | L-A | 1/50 | 0/50 | `left: 5, right: 3` |
| 6 | recall `a_failed_vector_search…` | L-A | 25/50 | 0/50 | `2.12 s` against 2 s |
| 7 | discord `task_autocomplete_ranks…` | L-B | 0/20 | 0/20 | never red here |
| 8 | theseusd `whole_replay` (uid 65534) | none / L-A | 0/40 / 5/25 | 0/25 (L-A) | `wait: No child processes (os error 10)` |
| 9 | theseusd `defer` restart | L-A; L-A + dsync loop | 0/12; 0/15 | unchanged | not reproduced |
| 10 | reminders `a_snooze_from_a_button…` | L-A | 19/20 | 0/20 | `index out of bounds: the len is 0` |
| - | peek `last_line_goes_out…` | L-A 0/10, L-B 0/15 | | 0/15 (L-B) | never red here |
| - | war room `ending_a_task_thread…` | L-A; L-B | 5/30; 1/30 | 0/30; 0/30 | guests `[9000...0202]` still listed |
| - | tests_paced | | not touched | | |

Test 8 was run as uid 65534 via `setpriv` (L1 refuses a root daemon's job).

## 2. Commits and causes

- `52be7e8` core, **product**: an ending watch (reply met; check met or expired) leaves the board before its frame is
  written, put back if the write fails. New test with a debug hook at the frame; planted old order fails it.
- `10ab23d` kernel, **product**: `job_l1` started the in-process (`Reap::Command`) init without registering it; the
  daemon's SIGCHLD-woken sweep reaped it as an orphan, so the wait got ECHILD. Now `children::spawn(Kind::Owned)`
  (spawn generic over the error type). Debug plant `THESEUS_TEST_L1_WAIT_DELAY_MS` plus new test
  `a_late_wait_for_the_replays_check_still_reads_its_exit`: plant-revert fails 3/3 as uid 65534, fix 0/3.
- `3ca4271` store: `commit_delay_ms` slept in the timed stretch; now `commit_extra_ms` is counted, not slept (10 s
  planted, `>= 40_000` kept). Plant (earliest frame) fails it.
- `677ee05` core: the work board is a projection applied after the commit (theseus-hanu); the test waits for it.
  By design; read-your-writes for `status.short` would need a `Push::caught_up_to`, not built.
- `ff1df55` core: the scene's driver ticks on the real clock; the pacer window now closes before the slot and DM are in
  place. Plant: old order plus a 1.2 s step fails (`claimed.is_none()`), new order passes.
- `7c64d23` core: paused clock, same 2 s bound.
- `d18f542` discord + sim: `theseus_sim::process_cpu()` bounds the same 50 ms median in CPU time. Plant (120 ms spin)
  fails it. The 400 ms "settled" sleep in that test is still a quiet wait.
- `fa14369` core: checkpoint test ordered by events: `FakeJev::hold_when`, `FakeProvider.gate` (`RequestGate`), a
  test-only `JudgeService.route_answers`; asserts the move is exactly at request 8 and loops 3 to 5 ran on the base.
  Plant (the boundary blocks on the verdict) fails it.
- `660e630` core: post is durable at commit, `wake.noticed` follows; tests await it (by design). Plant fails.
- `44037d6` core: peek test counts distinct lines, not re-sent views. **Unverified live** (never red here); the plant
  `TICK = 300 ms` still fails it.
- `53c5e56` discord: `end_guests` removes members then clears the record (the crash-safe order); the test waits for
  the record. Plant fails it.
- `935a115` core (nyczg): the tool-use warm read logs applied rows, pages, window and last position. **No fix**: not
  reproduced (0/27); next red should name its cause (`applied=0 pages>=1`: suspect `first_at`'s bytime).
- `9d8b0b2` keep every assertion where the keel guard counts it. `keel-guard.py --base 08336e8 --head HEAD --quiet`:
  empty (rc 0).

Not touched: `fake_model::tests_paced` (upper bounds on the stand-in's wall-clock pacing; cannot be made
deterministic without virtualizing its clock or weakening bounds).

## 3. Live check for the maintainer

`mkdir -m 777 /tmp/nb; cargo nextest run -p theseusd --no-run`, then
`setpriv --reuid=65534 --regid=65534 --clear-groups env HOME=/tmp/nb TMPDIR=/tmp/nb <whole_replay test binary> --exact a_late_wait_for_the_replays_check_still_reads_its_exit`
must pass (fails on the old code).

## 4. Checks

- `cargo fmt --all --check`, `scripts/shape.sh`, clippy `-D warnings` on theseus-core, -kernel, -store, -judge, -sim,
  -discord, theseusd: clean at `9d8b0b2`'s sources.
- `cargo nextest run -p theseus-core` whole, TZ=America/Phoenix, before the reset: 3464 passed, 0 failed, 24 skipped.
- Whole-crate runs of store, kernel, judge, sim, discord, theseusd on the final sources were started but lost to the
  reset. Earlier partial runs: discord `tests_war_room::a_shared_place_refuses_a_share…` failed alone at 101 s
  ("a thread for the task", in a test I did not touch; unchecked at base). A whole theseusd run at an earlier commit:
  known L1/root failures plus `acceptance_journey` (a 5 s wall bound of its own).
- Full gate, benches, cargo-deny: not run, as the brief says.
