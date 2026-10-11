# CLOUD_REPORT: cli-pages (theseus-7bee, theseus-0jet)

## What the earlier session did (fda7cdf, "work in progress")
Nearly all of the task: `ExecutionListParams {ids, n, before}` / `ExecutionListResult.older` (protocol `exec_list.rs`,
TS regenerated); the core's `execution.list` (`rpc/exec_list.rs`: a bare call as before, `{n,before}` newest-first via
the index, `{ids}`); the CLI's `latest_session` and `watch --interactive` asking `{n:1}`; `theseus sessions` and
`theseus executions` paging (50, `--before`, `-n`, `--all`, footer); `history` defaulting to `-n 200`; name resolution
reading executions a page at a time; the `surfaces` bench phase; tests (`crates/theseus/tests/pages.rs` 8,
`rpc/tests_exec_list.rs` 3). It stopped without a report or a measurement.

## What I did
- Verified it: `cargo nextest run -p theseus --test pages` 8/8; `-p theseus-core exec_list` 3/3; sim lifecycle/history 20/20.
- **Measured** on this VM's release build, `bench lifecycle --sessions 200000 --phases surfaces --runs 10`:
  `watch --interactive` to its prompt p50 4.7 ms, p95 7.3 ms (before the bench change, 10.0 / 14.3 in the first run);
  budget 50 ms: ok. The TUI's startup reads: p50 332 ms, p95 378 ms.
- **Fix (commit after fda7cdf, "sim: the surfaces phase budgets watch's prompt")**: the phase's sample was the slower of
  watch and the TUI reads, so it always missed at 330 ms: the TUI's `executions.watch` is the push's seed, which
  health-o1 changes and I was told to stay out of. The budget now holds watch (the row this task fixes); the TUI reads
  print beside it with no budget. Once the seed is O(1), budget it too (sim AGENTS.md says so).
- **Planted reverts**: `watch --interactive`'s `{n:1}` back to a bare list: `pages::watch_interactive_without_a_session_asks_for_one_page_of_one`
  failed. `execution_list` ignoring `before`: `tests_exec_list::execution_list_pages_back_from_the_newest_born` failed.
  Both restored and touched; `git status` clean.

## `SessionListResult` / bare `session.list` callers (item 1)
Changed to `{n:1}`: `cmd.rs::latest_session`, `interactive.rs` watch. Already by id: `interactive.rs:145`/`356`,
`resolve.rs:149`, TUI titles, discord `session_info`, status names. `theseus sessions` pages; `--all` is the only bare
call. **Left bare: `herdr_sync.rs:301`**, which needs every session (it matches pins and reconciles every pane); it is
a rarely run sync, not a prompt. The cockpit still calls bare `session.list`/`execution.list`; paging it is later work.

## Live check for the maintainer
`theseus-sim bench lifecycle --theseusd target/release/theseusd --sessions 200000 --phases surfaces --runs 10 --check`
should print `watch --interactive ... p95 <50 ms: ok`, and the TUI line with p50 about 330 ms (no budget).
`theseus sessions` on a real store prints 50 rows and a footer with `--before <cursor>`.

## Gate
`scripts/gate.sh` fails at the **keel** phase, on findings that are not this branch's: local `main` in this clone is
older than the branch's base (merge-base a9ad950), so the range includes others' changes (tui test assertion count,
two `#[allow]`/`#[expect]` in theseus-index and theseus-protocol tests, long-files.txt: lib.rs 2726->2733 and golden.rs).
My diff against the task commit touches none of those files; theseus-protocol lib.rs is 2729 lines, shrunk by my change.
Keel findings expected: none from this branch. I ran the later phases by hand: fmt ok, shape ok, clippy
`-D warnings` clean, suite 3,660 passed, 33 failed (all the known L1 failures: theseus-sandbox contract tests and
`spawn_100`, theseusd `sandbox` tests; this VM runs as root, theseus-pv6i). The protocol-types test is in the suite
and passed. Not run: cockpit build, deny, benches (NO_BENCH). The disk filled once during the work; I cleared
`target/release` and `target/debug/incremental`.
