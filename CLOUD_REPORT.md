# CLOUD_REPORT: thread-keepwarm, T9 (theseus-ytt8)

Branch `cloud/20261013-thread-keepwarm`. Store format untouched (no stored record or field changed).

## What I found
- A thread's session is a private conversation, so the existing keep-warm already read it, on the *profile's* 24 h window,
  with no notion of thread state: a paused thread kept being read. No per-day line exists in the worker; the only lines are
  the session's limit and the day ceiling (`day_hold`), and a refusal stops that session until its next message.
- Provider-prefix rule (documented in `keep_warm_threads.rs`): a thread's request already carries the shared layers
  (header, persona, project; same bytes, first three breakpoints) before its card and messages, so its one read restarts the
  shared entries and its own tail's. A tail-only request would be a different prefix, which the provider does not hold, so
  "cover only what would be lost" cannot mean a smaller request: one request per thread, no extra request for shared layers.

## What I changed (a9d39d28)
- `[threads] keep_warm_hours` (default 4, 0 = none; validated; template lines + template test). The window is
  min(thread, profile).
- `keep_warm_threads.rs`: `TurnRunner::thread_warm` (active only), the pure `stats_of`, `Core::threads_health/threads_cache`.
  Prepare forgets a paused/done/zero-hours thread session; a restart skips them and applies the shorter window;
  `thread.pause`/`thread.done` call `KeepWarm::forget` at once; a resume holds nothing until the next owner message.
- Day's line: `KeepWarm::due` is most-recently-written first; when a read is refused by the ceiling/runaway line the rest of
  the round is skipped, postponed one interval (no busy loop), counted (`skipped_today`) and warned.
- Metrics (`theseus_protocol::thread_cache`): per thread and total, from the day's `provider.call` (loop 0 = turn's first)
  and `keep_warm` rows: cold resumes, rewrite tokens/dollars, warm hits, rewrites avoided (hit only a keep-warm read kept)
  with modeled saving (write price minus read price over tokens read), keep-warm reads/dollars. Surfaces: health `threads`
  (`keep_warm_hours`, `cache`), CLI health `threads:` lines (`render/threads_cache.rs`), `thread show` ("cache today"),
  `ThreadRow.cache` (also `thread list`). Health serves a minute-old copy and reads nothing for a daemon with no thread.
- TS regenerated (4 new files). Doc: theseus-core/AGENTS.md "Threads kept warm".
- Defect noted/changed while there: none beyond making `ThreadsConfig` `PartialEq` only (f64).

## Proof
- New: `rpc/tests_thread_keep_warm.rs` (7) and `keep_warm_threads::tests` (3), `config::threads` template test, render test.
  `cargo nextest run -p theseus-core -E 'test(thread_keep_warm)|test(keep_warm_threads)|test(tests_keep_warm)|test(config::threads)'`: 21/21.
- Planted reverts (restored and touched each time, git status clean after):
  - thread window ignored -> `a_thread_is_kept_warm_for_its_window_and_a_plain_session_for_the_profiles` failed.
  - pause does not forget -> `pausing_stops_the_reads_at_once_and_a_resume_waits_for_a_message` failed.
  - thread state not checked (paused warmed) -> 3 failed: pausing, `a_read_finds_a_paused_or_done_thread...`, restart test.
  - oldest-first order -> `at_the_days_line_the_most_recent_is_first...` failed.
  - TTL edge `>` to `>=` -> `a_gap_past_the_ttl_is_a_cold_resume_and_the_edge_is_warm` failed.
- Whole suite `cargo nextest run --workspace`: 4588 passed, 34 failed. 33 are the known L1 failures (theseus-sandbox contract,
  `spawn_100`, theseusd sandbox). One is outside the lists: `tests_work::the_substrate_scenario_reads_as_one_tree_and_pings_for_the_right_reasons`
  (left "proc.run", right "sleep 2", tests_work.rs:381, a work-board job label). Unrelated to keep-warm by inspection; I could
  not confirm it on a clean base (the stash rebuild failed, likely disk), so it is **unverified as pre-existing**: please run it on main.
- fmt, shape.sh, clippy `-D warnings` (workspace, all targets): clean. Cockpit builds.
- Gate: `scripts/gate.sh` fails in **door/keel** before any build, on the same finding on an untouched tree
  (see below). I ran the later phases by hand (fmt, shape, clippy, cockpit, suite) as above. Not run: deny, benches.
- `theseus-sim bench lifecycle --runs 10 --check`: **not run successfully** (debug build: "Connection reset by peer"); measure on the owner's machine.
  No timer or read on a turn's path was added; a plain turn's frames are unchanged (no frame code touched).

## Keel
`python3 scripts/keel-guard.py` output (not from my diff; range is merge-base with a stale origin/main):
```
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs ...` on 1c2beb2747: its signature does not verify (%G? is E)
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
keel: the same test: ...a_checkpoint_for_close_is_made_durable_by_the_close -> ...is_durable_without_the_close (renamed and rewritten)
```
The ack's signature fails here only because the signer key is not in this VM's keyring. **Keel findings expected** from my own
commits: none. (I made helpers in `tests_keep_warm.rs` `pub(crate)`; no test deleted or weakened.)

## Live check (owner's daemon, after a day of threads)
- `theseus health` : a `threads:` line (window, sessions held, reads skipped at the day's line) and `today (...)` totals + per-thread lines.
- `theseus thread show <slug>`: `cache today:` line.
- `theseus thread pause <slug>` then `theseus health`: held count drops at once; no `keep_warm` row for its session afterwards:
  `theseus ledger window` / the ledger query for kind `keep_warm` and that session id.
- Scratch daemon on the stand-in with the clock advanced past `keep_warm_hours`: the thread's reads stop while a plain session's go on (as the paused-clock test shows).

## Left / uncertain
- Numbers cover the thread's *current* session only, and the local day; a first call with nothing in the prior hour counts as a
  resume only if the session predates the window. Rewrite dollars are the cold call's creation tokens at the catalog's price.
- First health call after a minute pays one ledger scan (blocking pool); the cockpit's 2 s polling hits the cached copy.
- Day-line refusal still stops the refused session until its next message (existing behaviour, tested); the skipped ones only wait.
