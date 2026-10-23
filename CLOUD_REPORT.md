# CLOUD_REPORT: tail7c-card (theseus-15kn3)

Branch `cloud/20261021-tail7c-card`. Fix commit `056e5665`. Files touched: `card/run.rs`, `card/tender.rs`,
`card/tests_job.rs`, core `AGENTS.md` (owner-card paragraph). No file outside the row's list.

## Cause and fix
- Cause: `card_run` returned at "full" before writing `last_run_ms`; `next_at` then found the run due at once, and
  `tend` reran it, scanning and clustering the archive each time.
- Fix: a non-dry run that finds `open >= max_open` now sets `last_run_ms = Some(now)`, writes the state, and returns
  before `state::seen`, `evidence::scan` and `cluster::cluster` (counts 0). The card tender keeps an `AtomicU64` `ran`
  mark stored when `run` starts; `next` uses `stored.max(ran)`, as `people_sweep_after_serving` does. `done`'s one-day
  look is unchanged. Module docs in `run.rs` and `tender.rs` and the AGENTS.md paragraph say a full run is an attempt.

## Scan counts: readers
Only `crates/theseus/src/card_cmd.rs` `propose_lines` prints `evidence/withheld/clusters/fresh` (and the dry run's
`fresh.div_ceil(8)`). `rpc/card.rs` and the cockpit do not read them. A dry run keeps its scan (and the "full" stop
after it), so the owner's `card propose --dry-run` still shows the counts. Only a real run's counts are 0 when full.

## Twins and other loops (read)
All `tend` users keep a `ran` mark: `learning/tender.rs`, `consolidate/tender.rs`, `judge/onto/sweep.rs`,
`judge/people/sweep.rs`, `own_bench/sampler.rs` (load seen, store at :327), `rpc/earned.rs`, `rsi/backlog/tender.rs`;
the card was the only one without. Outside `tend`: `keep_warm.rs` (in-memory `call_at`), `aws/tend.rs` and
`aws/durable.rs` (fixed `sleep` periods/backoff), `initiative/digest.rs` (no loop). None has the bug. No fix needed.
Follow-up (not done): a shared ran-mark helper for the tenders.

## Tests (all in `card/tests_job.rs`)
- `a_full_card_records_its_attempt_and_scans_nothing`; `a_full_card_does_not_rerun_within_its_interval` (paused clock,
  counts the journal line, exactly 1); `a_failed_card_run_is_not_due_again_at_once` (bad `[card] profile`, 1 warn line);
  `a_card_below_max_open_runs_again_on_its_normal_schedule` (after "decline", `next_at` is "weekly", a run is no
  longer "full" and reads the archive. The fixture has nothing fresh, so it asserts no stop and evidence > 0, not calls > 0).
- Not run red-first on the base; proved by planted reverts instead:
  1. Drop the state write in `run.rs`: `a_full_card_records_its_attempt_and_scans_nothing` fails (tests_job.rs:799).
  2. Drop the state write and the `ran` mark: that test fails, and the two paused-clock tests spin the hot loop and hit
     nextest's 120 s kill (2 timed out).
  3. Drop only the `ran` mark: `a_failed_card_run_is_not_due_again_at_once` hits the 120 s kill (hot loop).
  Each file was restored and touched; `git status` showed only my intended changes.
- Passing: `cargo nextest run -p theseus-core card tender` 147/147; `-p theseus-core` whole 4127 passed, 25 skipped.
  `cargo nextest run -p theseus card` was not run (no CLI reader changed).

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, fmt, shape, features, clippy, cockpit,
test build and reader rule passed. The suite phase failed, on: the known L1 cases (`theseus-sandbox::contract`,
`spawn_100`, `theseusd::sandbox`), and also `theseus-core` `tests_self_graduate`, `tests_self_backlog`,
`tests_job_handles`, and `theseusd` `job_handles`, `mcp_server`, `outbox`, `push`, `planted_secrets`, `reaping`,
`job_latency`. The disk filled twice during the gate (28 GB of target; I deleted `target/debug/incremental`), so the
non-L1 failures are probably disk and load. Evidence: the three theseus-core modules pass alone on the base and on my
tree, and the whole `theseus-core` suite passes (4127). I did NOT re-run the `theseusd` failures alone, nor
`machine_checks` after the suite; the maintainer's gate should. `cargo deny fetch` was not done.
Keel guard (`--base origin/main --head HEAD --quiet`): empty output, exit 0. Store format unchanged (49; no bump).
Signing: commits are unsigned, as the brief says for this VM.

## Live check
On a scratch daemon with `[card] max_open = 1` and one open proposal, `journalctl`/log shows one
`card: the run finished ... stopped="full: 1 proposals are waiting"` at the first attempt, then nothing until the
next learning hour at least `every_days` later; `theseus card propose` shows `0` counts, `--dry-run` shows the scan.

## status.md sentence
A full owner card now records its attempt and scans nothing, and the card tender keeps a ran mark like its twins, so a
full queue no longer loops the nightly run (theseus-15kn3).

## Owner's daemon after install
One journal line at the next attempt, then quiet until the normal schedule or until proposals are answered.

## Uncertain
`origin/main` was at `8260dbab` when the keel guard ran; the guard reported 278 files changed by the working tree
against that base, none of them mine.
