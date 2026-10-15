# CLOUD_REPORT: store-close (theseus-7okz9)

Branch `cloud/20261013-store-close`, based on main as cloned (store format 39 in the task's words; no format bump, none needed).

## Step 1: a clean stop closes the store (SIGTERM leaked the whole core)

**Found.** The 9 store handles were not nine subsystems. They were one leaked `Arc<Core>`: `core=1` after the runtime dropped, with
`held=9` handles on the store, which is the count the owner's install #29 logged. The cycle was in
`crates/theseusd/src/mcp.rs`: `Core.mcp_server` keeps a health closure; the closure held `Arc<Server>`; the server held `McpCore`;
`McpCore` held `Arc<Core>`. Only the `shutdown` method's wake (`core.shutdown.notify_waiters()`) broke it (the task stopped the
server and replaced the closure). SIGTERM and SIGINT (the service manager's stop) never notify `shutdown`, so the core and its
store outlived the runtime, redb never closed, and the next start repaired the index. The durable stop checkpoint of
theseus-8ub95 had hidden the replay but made every unclosed file need repair.

How it was found: no thread outlived the runtime (the new warning lists the threads: `store-writer` and main only), so
the holder was a cycle, not a thread. The owner's daemon was not needed: a test daemon with the MCP server, index tender, web UI, judge,
telemetry and a connected `work.watch` leaked nothing on `shutdown` but leaked `held=9 core=1` on SIGTERM; with
the MCP server off, nothing leaked.

**Handle list (what held the store).** One `Arc<Core>`, through the MCP server's health closure; the 9 handles are that
core's own store clones (core store, kernel, turn handles, caches). Candidates the task listed (the push, the work board's
observer, watches, routines, thread indexes, census/inspect readers) were exercised in the test and hold nothing past the runtime.

**Changed** (a309ad5e): `mcp.rs` holds the server by `Weak` in the closure (reads `stopped` once the server is gone);
`main.rs`'s stop warning now names `core=` (live cores) and the threads left (`live_threads`); `tests/store_close.rs` (4 tests);
`crates/theseusd/AGENTS.md` trap.

**Proved.**
- `cargo test -p theseusd --test store_close`: 4 passed. Each starts a daemon with every service on, a connected `work.watch`, a turn, and a batch of ~22 reads/writes
  (routines, watches, wakes, threads, census --exact, memory, ledger.verify, ...), stops it by `shutdown` and by SIGTERM, at the end of the work and 0 ms to 4 s after a start,
  and requires no `outlived the runtime` in the log and, for the first two, `index_repaired == false` and `replayed_into_index == 0` at the next start.
- `cargo test -p theseusd --test stop_checkpoint`: 2 passed (unchanged).
- **Planted revert** (mcp.rs back to the parent's): `a_sigterm_with_work_on_every_surface_closes_the_store` and `a_sigterm_soon_after_a_start_closes_the_store_too` FAIL with
  `held=9 core=1 threads=store-writerx1,theseusdx1`; the two `shutdown` variants pass (as in production). File restored and touched; `git status` clean.

**Not changed, and worth the owner's attention.** The signal arms in `serve_socket` and the stdio arm call `stopping_on` but never
`core.shutdown.notify_waiters()`, so the web UI's graceful shutdown, the MCP server's stop and the harness's notify socket loop
do not get their wake on a signal (the runtime's drop ends them anyway). Notifying there is the other half of "an explicit shutdown order"; I left it out
because the `Weak` alone closes the store and it is a behaviour change beyond the measured fault. With it, the planted-revert test would no longer fail (the notify breaks the cycle too), which is why I did not add it.

## Step 2: the lifecycle bench guards the restart

**Changed** (a8f0d71e): `Start.index_repaired` (from the store phase's `index_repaired`); the shutdown phase bails if the restart after
`shutdown` repaired the index, and ends with one SIGTERM stop (once health says the MCP server is `listening`, as the owner's is) and the same check on the start after it.
Unit test: `lifecycle::tests::a_start_is_read_from_health` reads the flag.

**Proved.** `theseus-sim bench lifecycle --phases cold,shutdown --runs 3`: LIFECYCLE OK with the fix.
Planted revert of the mcp.rs fix: exit 1, `Error: the restart after a SIGTERM repaired the store's index (13 ms to serving): a handle outlived the runtime`.

**Before/after serving start.** On this VM, debug build, a synthetic 50,000-session store (194.6 MB of WAL): the restart after SIGTERM with the bug repaired the index
and served in 372 ms (the bench bails there and prints that); with the fix there is no repair (the guard passes). The owner's 584 MB index repaired in 8.44 s of store open (install #29's number); that store is not here. The bench's SIGTERM start is
not a timed sample (it is not added to the restart percentiles, so the gate's budgets are unchanged); the check is the repair flag, which is exact, not a timing.
(`--sessions 50000` also reports `LIFECYCLE BUDGET MISSED` on a debug build on this 4-core VM, cold store phase p50 184 ms; that is the synthetic size, not this change, and the gate runs the empty store.)

## Step 3: `census --exact` on a fresh session

**Found.** The token counter answers HTTP 400 to a request with no message; a fresh session's first request has none.
**Changed** (f6312a72): `census.rs::exact_counts` skips the full count when the request has no messages: total = system + tools (both measured over the smallest form), messages 0.
The `FakeProvider`'s `count_tokens` now refuses an empty request as the API does. `exact_asks_the_provider_to_count`'s `total == estimate` is rewritten to
`total == system + tools` and within 2 of the estimate (the smallest form's word rounds by a token); two tests added: every kind of place, and a session with a message.
**Proved.** `cargo test -p theseus-core --lib tests_census`: 11 passed. Planted revert of census.rs: `exact_on_a_fresh_session_counts_with_no_message` and `exact_asks_the_provider_to_count` FAIL (the 400); restored and touched.

**Live check for the maintainer** (needs a real key; counts, bills nothing):
`theseus census --exact` on a fresh daemon should print exact counts with messages 0 and total = system + tools, no 400; `theseus census --exact --session <id>` of a session with messages should still print messages > 0.

## Live check for steps 1 and 2 (the owner's install)

1. `theseusd` as a service on the owner's store: `systemctl --user restart theseusd` (SIGTERM), then `theseus health`: the store phase should say `index_repaired: false`, and the journal should have no `handles on the store outlived the runtime`. Serving start should be back near 1 s, not 8.5 s.
2. If a warning appears, it now prints `core=N threads=...`: `core=1` means another cycle through `Arc<Core>`; threads named there are real holders.

## Keel findings expected

None of mine. `python3 scripts/keel-guard.py` here reports pre-existing findings that come from the base history, not this branch's diff: seven or so `the same test ... (moved)` lines for `compiler.rs` tests and two renamed/rewritten tests,
two acks refused because their signatures do not verify in this VM (`%G? is E`: no keys), and `crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0)`, which is in the range from the guard's merge-base and covered by the ack on 1c2beb2747 that this VM cannot verify. My edits to lifecycle.rs do not touch that line.
The gate stopped at its door and keel phases for that reason, so I ran the later phases by hand (below).

## Gate (by hand, since the keel phase cannot verify signatures here)

- fmt `cargo fmt --all -- --check`: ok. shape (`scripts/shape.sh`): ok. clippy `cargo clippy --workspace --all-targets -q -- -D warnings`: clean.
- suite `cargo nextest run --workspace --no-fail-fast`: 5356 run, 5322 passed, 34 failed, 47 skipped. 33 of the failures are the known L1 set (theseus-sandbox's 19 contract tests and `spawn_100`... theseus-pv6i, root VM: 18 contract + bench in theseus-sandbox, 14 in theseusd's `sandbox`). The 34th, `theseus::herdr a_change_reaches_herdr_within_ten_ms_at_the_99th_percentile` (a 10 ms p99 under load, in the CLI crate this branch does not touch), passed alone (4.4 s). Every new and changed test passed in the suite (`store_close` x4, `stop_checkpoint` x2, `tests_census` x11, lifecycle's unit test).
- Mid-run the VM's disk allowance filled once; I deleted regenerable test binaries and `target/debug/incremental`, and reran the suite from scratch.
- Not run: deny (`cargo deny` offline fetch not done), cockpit lint/test/build, benches (`THESEUS_GATE_NO_BENCH=1`), size.

## Left over / uncertain

- Notify `core.shutdown` in the signal arms (above).
- A cycle of this kind will not be caught by a bench that stops with the method only; the bench now also stops with SIGTERM once. The class is held by `tests/store_close.rs`.
- Docs to change at review: `crates/theseusd/AGENTS.md` trap (done) and the spec's Part III item for 8ub95/7okz9: "a handle outliving the runtime was a core cycle through the MCP server's health closure".
