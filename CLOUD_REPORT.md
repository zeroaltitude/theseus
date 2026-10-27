# CLOUD_REPORT: bound the RSI door's git fetch (theseus-6vlr4)

Branch `cloud/20261027-p3t-rsi-fetch-bound`, base `9470df70`.

## Checked before starting
- `git log origin/main`: `theseus-g5wxc` has landed (6d65073 the fetch, 81518ed its join fix: the owner is told of a failed fetch; merged at 2557601). This row is finding 2 only. Nothing else of this row had landed. Note: the clone's `origin/main` ref was stale (7d39d57) until I ran `git fetch origin main`; the keel guard showed 100+ unrelated findings until then.

## Citations, and what each actually was at the base
- `rsi/join/git.rs:50-51` `pub fn fetch`, body `p.git_ok(&p.repo, &["fetch","-q",remote])?` — correct.
- `rsi/tree.rs:338` the door's `match fetch(...)` — correct (inside `refresh_main`; `:356` docs correct).
- Other callers: `revert.rs:226`, `revert/doing.rs:311`, `join/reviewing.rs:126` — correct. `join/git.rs:219` and `:229` — WRONG: those are not callers (that file is 238 lines of other functions; no fetch there). The real extra callers are `join/joining.rs:82` and `:239`. All of them now get the bound through `fetch()`, which errors at the bound; they already propagate with `?`, so a hung remote fails that step (revert, review, join) and it is retried at its next step.
- `rsi/mine/fixture.rs:125` / `:133` — WRONG: they are a recorded model ledger (`argv(run, ["git","fetch","origin"], …)`), not a scripted git; they cannot express a hang. The usable fixture is `rsi/tests_tree.rs` (`fixture()`, a bare upstream plus a clone, hermetic env). I added a stand-in `git` on the place's `PATH` (see tests).

## What the two locks protect, and whether the fetch can leave them
- `tree.rs` `REPO` (std Mutex, taken by `open` and `close`): serializes `git worktree add/remove/prune` and the branch-name choice, which race for `.git/worktrees` locks. It guards nothing a fetch writes (`refs/remotes/origin/*`, `FETCH_HEAD`, objects; git locks those itself). **The fetch can move outside it, and did**: `open` now canonicalizes, runs `refresh_main`, then takes `REPO`. `open` reads `origin/main` once afterwards, so a concurrent fetch only changes which commit that is. Doc comments on `REPO`, `FETCHED`, and `refresh_main`.
- `tree.rs` `FETCHED` (std Mutex, a list of repo→last fetch time): guards only that list. It is held across the fetch on purpose: it makes the fetch single flight, so cuts of a pass that start together wait for the one fetch (60 s freshness) instead of each fetching. **It cannot leave without a second "fetch in flight" marker doing the same job**; the wait is now bounded by the limit instead. Not moved.
- The tick: `pipeline_tick` holds its tokio tick mutex and runs `start` (→ `tree::open`) and `clear` (→ `tree::close`) in `theseus_store::blocking`. Before, a hung fetch inside `REPO` stalled both. Now `close` never waits on a fetch, and `open` waits at most the bound.

## What changed (commits)
- da377e7 `rsi: the door's git fetch is bounded, killed and reaped at its bound, and taken outside the tree lock`; follow-up commit: clippy `redundant_clone` fix in the new test.
- `join/git.rs`: `FETCH_LIMIT = 60 s` (a named constant, not a config key: it is a fault bound with one right answer, "few knobs"; no config/template/example change), `fetch()` = `fetch_within(…, FETCH_LIMIT)`. **Mechanism: a wrapper, not a git-level bound**, because git has none for connect/total: `http.lowSpeedTime` counts only after a transfer starts, ssh `ConnectTimeout` is the user's config, and the transport varies. The child leads its own process group (`process_group(0)`); a watcher thread does `recv_timeout(limit)` (a timed wait, no polling) and on timeout `killpg(SIGKILL)`; the caller's `wait_with_output` then reaps git and drains its pipes. On the bound the fetch returns `Err("git fetch origin did not finish in 60s and was killed")`; it never succeeds with a stale base.
- `tree.rs`: `Place::git_command` extracted from `git`; `fetch_limit(p)` (a `cfg!(test)` seam reading `THESEUS_SELF_FETCH_LIMIT_MS` from the place's own env; in a daemon it is always `FETCH_LIMIT`); `open` reorder; docs.
- The door's handling (unchanged, verified): `refresh_main`'s `Err` arm logs, notifies the owner through `Place::notify`, does not record a fetch (the next cut tries again), and `open` goes on from the base it has.
- New tests: `rsi/tests_fetch_bound.rs` (stand-in `git` that, on `fetch`, records its pid and its `sleep 600` child's pid and waits) and `rsi/pipeline/tests_fetch.rs`.

## Proof
`cargo nextest list -p theseus-core -E 'test(/tests_fetch|a_hung_fetch|a_fetch_that_answers/)'` selects exactly 4 tests:
`rsi::pipeline::tests_fetch::a_hung_fetch_does_not_hold_the_ticks_start`, `rsi::tests_fetch_bound::{a_fetch_that_answers_inside_its_bound_succeeds, a_hung_fetch_fails_at_its_bound_and_leaves_no_git_behind, a_hung_fetch_holds_no_close}`. All 4 pass (4.1 s); `-E 'test(/^rsi::/)'` 338/338 pass.
- Tick test (positive): with a hung fetch (bound 800 ms) one `pipeline_tick` ends, `started.len() == 1`, the item's tree exists, the stand-in fetched, nothing left running.
- Close test: while an `open` waits on its hung fetch, `tree::close` of another tree finishes (<2 s) with the opener still in flight; then the open succeeds from the base it had.
- Leak test: after the bound, both recorded pids (stand-in shell and its `sleep`) are not running (a zombie awaiting its reaper does not count; /proc/<pid>/stat state).
- Red at the base: the base has no `fetch_within`, so the new tests do not compile there; the red is shown by the plants.
- Planted reverts (each restored with `touch`, `git status` clean afterward except my own changes):
  (a) bound removed (watcher waits forever): tick, leak and close tests all `TIMEOUT [120 s]` (nextest kill). The tick test's own 30 s `tokio::time::timeout` cannot fire because the tick blocks its runtime thread; nextest's kill is what fails it.
  (b) group kill replaced by `kill(pid)` only: the leak test `TIMEOUT 120 s` (the orphaned `sleep` keeps the pipes open, so the leak shows as a hang, not as the pid assertion).
  (c) `refresh_main` moved back inside `REPO`: `a_hung_fetch_holds_no_close` FAIL "the open was still waiting on its fetch when the close ended (4.03 s)".
- Keel guard (`python3 scripts/keel-guard.py`): `keel: ok … 0 findings` after the base ref was refreshed. No keel changes; nothing edited in guarded files.
- Load runs were not requested for this task and were not done.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, keel tests, fmt, shape, recall tests, features, clippy (after fixing one redundant clone), clippy-probe, cockpit, test build all ok. (A first run died with `ld` Bus error because the disk allowance was spent; I deleted `target/debug/incremental` and re-ran with `CARGO_INCREMENTAL=0`.) Suite: 8746 run, 8712 passed, 34 failed, all the known L1 failures: 20 in theseus-sandbox (contract, `spawn_100`) and 14 in `theseusd::sandbox` (root daemon, no job cgroup, theseus-pv6i). No other failure, no timing flake. Remaining phases run by hand: protocol_types (no diff in `cockpit/src/protocol.gen`) and `cargo deny check` (ok). Benches skipped (`THESEUS_GATE_NO_BENCH=1`).

## Not done / uncertain
- A fetch that fails is not remembered: a night that opens N items against a black-holed remote waits one bound (60 s) per item. `OWNER CALL:` should a failed fetch suppress retries for a short window (e.g. the 60 s freshness), trading a 60 s wait per item for retrying later? I left it, since the owner's line favours not stopping work and a retry may hit a recovered remote.
- The kill races a child that exits in the same instant the bound fires (pid reuse, theoretical, documented at the kill).
- Docs: nothing edited; `rsi` docs could mention `FETCH_LIMIT` in the self pipeline note.

CLOUD REPORT COMPLETE
