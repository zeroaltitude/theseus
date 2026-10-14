# CLOUD REPORT: wait-cost (theseus-7m7w.3)

Branch `cloud/20261013-wait-cost`, base f8aec2d2 (store format unchanged: no stored field added).

## Step 1-3: the asked wait, the warm cache, the contract
**Found.** `job_wait` clamped `timeout_secs` to `proc_sync_secs` (jobs_tools.rs). Also: `wait()` used `std::time::Instant`, so a paused clock could not drive it; it is now `tokio::time::Instant` (same in production).
**Changed** (ced74e6b):
- `[tools] job_wait_max_secs` (default 1800; validated `proc_sync_secs..=3600`, 3600 = `WAIT_SECS_MAX`, the tool's deadline), in config.rs, config/limits.rs, the template, `ToolRuntime`. Clamp is now `ask.clamp(1, job_wait_max_secs)`. Job end, a person's message and a stop still end the wait at once.
- Result: "Still running after N s" with the real N; if the cap cut an ask: "...(the longest one wait lasts; X s was asked)". Meta gains `warm_reads`.
- Contract: the tool description says it waits up to timeout_secs and returns early when the job ends or a message arrives; the registry appends "A wait lasts at most N s." (`say_wait_cap`, called from `definitions_for`).
- Keep-warm: new `keep_warm_wait.rs`. A response that asks for `job_wait` has its request kept (`stash_for_wait`, one line in turn.rs; one clone, only then, only if the request caches and the prefix >= `keep_warm_min_tokens`). The wait re-sends it at max_tokens 1 through the existing `keep_warm_send` (reserve, day ceiling, `keep_warm` row with `during_wait: true`, health counts, telemetry), at 240 s of a 5 min entry, every `[cache] keep_warm_minutes` of a 1 h entry; it runs beside the wait's other events, so a settle/message/stop drops it. Two parallel waits share one due time. Stash dies at turn end.
- No change to toolrun.rs beyond a field and one call; turn.rs stays at its 3525 ceiling.
**Proof.** New `tests_wait_cost.rs` (paused clock, a `Gated` launcher holding the job): ends at 300 s of a 900 s ask (took 300-303 s, delivers); running job goes 900 s ("Still running after 900 s"); message at 120 s ends it at 120 s (interrupted_by message, joins the next request); cap 600 (text + description); no timeout = 60 s; 900 s wait on a 5 m cache reads at 240/480/720 s, each equal to the request that asked, 3 keep_warm rows during_wait, counted; job ending at 300 s -> 1 read; 200 s wait -> 0; 1 h cache and small prefix -> 0. Config test `job_wait_max_secs_has_a_default_and_a_range`. All 10 pass.
Planted reverts: old clamp back -> 7 of 9 wait tests fail; stash call removed -> 3 fail (the read, the once-or-not, the measure); message no longer ends the wait -> `a_message_at_120_s...` fails. Files restored and touched.
Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1`, with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` because the 40 GB disk allowance filled): door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build, reader rule pass. Suite: 4495 pass, 33 fail, all theseus-sandbox contract/bench and theseusd sandbox tests (the known root-L1 failures, theseus-pv6i); nothing else. Not run: protocol_types, the benches (NO_BENCH), `cargo deny` advisories phase FAILED here (Cargo.lock untouched; licenses/bans/sources ok; not investigated). `tests_work::the_substrate_scenario...` failed once under a loaded first gate run and passed alone (twice).
Keel: after refreshing the stale local `origin/main` ref the guard is clean; no keel findings expected.

## Measure (stand-in, modeled cache, paused clock; `cargo nextest run -p theseus-core -E 'test(five_minute_job)' --no-capture`)
Model asks `job_wait 900` repeatedly; 5 min cache; 30k-token prefix modeled. Before = `job_wait_max_secs = 60` (the old clamp).
| case | model calls | keep-warm reads | cache read tok | cache written tok | dollars |
|---|---|---|---|---|---|
| before, 5 min job | 7 | 0 | 241,677 | 40,614 | 0.1541 |
| after, 5 min job | 3 | 1 | 120,327 | 40,256 | 0.1265 |
| before, 20 min job | 22 | 0 | 860,152 | 41,969 | 0.2902 |
| after, 20 min job | 4 | 4 | 281,083 | 40,336 | 0.1595 |
I did not write a docs/benchmarks file (the brief forbids editing that tree): please add this table there. The async bench's steer_stand_in (interrupt/cancel) was NOT run (needs a release-thin build; disk). Those families call `job_wait` with the steer ending the wait, which this change leaves alone: run `python3 bench/async/steer_stand_in.py --bin-dir target/release-thin --family interrupt --arm after --inject-at 20 --inject-at 90` (and `cancel`) and compare with steer-queued's times.

## Live checks for the maintainer
1. Real model, thinking on: a keep-warm read sets max_tokens 1 on the request (same as the between-turns read); confirm the provider accepts it mid-turn: run a Sonnet 5.5 task with `proc_run background sleep 600` then `job_wait 900`; expect 2 `keep_warm` ledger rows with `during_wait` (`theseus ledger` / `theseus health` cache block) and the next call's usage `cache_read` >> `cache_creation`.
2. Terminal-Bench, before/after, the tasks whose trials had a `job_wait` answering "Still running after 60 s" (list from the 2026-10-13 records, not by score): compare model calls/trial, $/trial, pass rate. Not run here.

## Uncertain / design notes
- A read in flight when the wait ends is dropped unbooked (a few cents at most).
- A failed wait read calls `KeepWarm::stop` for that session's between-turn reads until its next message (shared code path).
- Reads on a 5 m cache cost 0.1x of the prefix every 4 min; break-even against one rewrite is about 12 reads (~48 min), still inside the 30 min default cap.
- Docs to update: spec Part III item, status.md; core AGENTS.md already updated here.
