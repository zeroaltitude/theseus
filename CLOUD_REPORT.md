# CLOUD_REPORT: status-short-board (theseus-8kn12)

Branch `cloud/20261013-status-short-board`. One code commit, `756096c`, then this report.

## What I found (profile of `status --short`, 1,000 sessions, the bench's own store)

`strace -f -ttt` of the old CLI inside `bench status` (6 runs; strace inflates every syscall about 3x, so read the
shares, not the milliseconds). Timeline of one run, 17 ms under strace:

| phase | at (ms, strace) | notes |
|---|---|---|
| exec, dynamic loading (38 MB debug binary, libc/libm/libgcc) | 0 to 4.2 | |
| Rust runtime start, `/proc/self/maps`, cgroup quota reads | 4.2 to 7.9 | std/tokio start |
| multi-thread tokio: 4 worker threads (`clone3` x4) | 8.0 to 10.8 | not needed for one request |
| clap parse of the whole command tree | 10.8 to 12.9 | `--version` alone costs 3.5 ms debug vs 1.1 ms for `/bin/true` |
| config read | none | the CLI reads no config on this path |
| connect (unix socket) | 12.9 to 13.0 | local, tiny |
| request to answer | 13.5 to 16.0 | the daemon's `executions.watch {limit: 0}` |
| seen file | 16.3 to 16.6 | read twice (`displayed()` and `done_count()`), 4 `openat`s |
| parse, render, exit | 16.7 to 17.7 | |

The daemon's side of the old read, from `rpc/methods.rs` and `push.rs`: it subscribed the connection to the bus
(`watch_all`), cloned every view on the board (the seed holds the 1,000 most recent executions, so about 1,000 clones),
partitioned and sorted them, counted executions in the store, and built `confirm_list`. The answer was every
needs-you/working view plus questions. The CLI then recounted them. The reads probe's 3.7 ms is the same kind of cost;
the bench's extra comes from that O(board) clone per draw plus the CLI's fixed start (thread pool, clap).
I did not time the daemon's handler in isolation: it has no debug env timestamp, and `strace` of the daemon would
only repeat the request-to-answer row above.

## What I changed (`756096c`)

- `crates/theseus-core/src/push/short.rs` (new): `Short`, kept on the board. A needs-you view that did not fail sits in
  a `BTreeSet` ordered by (since, id); a failed one in a map with its view's position (plus an ordered set); working is
  a counter. `Board::apply` and `Board::load` move an execution between the groups through one call each; the seed builds
  it with `Short::of`. push.rs gained a field, three call lines and `Push::status_short` (work-board's split will port a
  few lines).
- Protocol: method `status.short` (`STATUS_SHORT`), `StatusShortResult { position, needs_you, working, failed:
  [ShortFailed {execution_id, position}], first }`, TS regenerated (`cockpit/src/protocol.gen/`). I did not use
  `executions.watch {limit: 0}`: its contract returns every active view and subscribes the connection, so a counts-only
  read needed its own method. Both reads share one dispatch arm (`executions_reads`), because `dispatch` was at clippy's
  100-line and complexity limits.
- The CLI (`status.rs`): `--short` (not `--watch`, not `--json`) sends `status.short` only. The `✗` keeps its meaning
  (theseus-xnff's (a)): the daemon sends every failed execution with its view's position and the CLI counts those the seen
  file has not reached, exactly `Board::summary`'s rule. The seen file is read once (`seen::counts`) instead of twice.
  An old running daemon answers "method not found" and the CLI falls back to the full read (a binary swap survives a
  daemon that has not restarted).
- `main.rs`: `#[tokio::main]` became a plain `main` that builds a current-thread runtime for `status --short` and the
  multi-thread one for everything else (one request needs no pool).

## How I proved it

- Property test `push::tests::short::the_groups_equal_a_full_recount_after_any_run_of_changes` (proptest, 1 to 200 random
  state changes over 12 executions, kept groups equal `Short::of(views)` after every step, and the counts equal a recount
  by level) plus `the_first_is_the_longest_waiting_and_a_failure_comes_last`.
- `tests_push::short`: a real core with an approve rig: before any turn all zero; after a turn that asks, `needs_you 1`,
  `first` is that execution; the counts equal a recount of `executions.watch`; and `status.short` made no subscription
  (`all_watchers() == 1`, only the full read's).
- CLI: all existing `tests/status.rs`, `status_edges.rs`, `seen*.rs`, goldens and unit tests pass unchanged (424 tests in
  `-p theseus -p theseus-protocol`). The scripted daemon in `tests/status.rs` now answers `status.short` from the same
  snapshot, so the `--short` outputs are checked byte for byte against the same expected strings as before.
  New: `the_short_form_asks_for_the_counts_alone` (records the requests: only `status.short`) and
  `a_daemon_without_status_short_gets_the_full_read` (error -32601, same line printed).
- Planted revert: `if a.short && !json` made `if false && …` in status.rs: `the_short_form_asks_for_the_counts_alone`
  FAILED (requests were `executions.watch, session.list`-style full read), the others passed; restored, touched, all 14
  `binary(status)|binary(status_edges)` tests pass. The bench's own 5 ms check is a timing check and, in debug on this
  VM, fails with or without the change, so this test is the deterministic guard.
- Bench, `target/debug/theseus-sim bench status --runs 300 --sessions 1000` (debug, 4-core VM, load about 2.5 to 4).
  Indicative only. Same VM, interleaved with the old binaries:

  | | p50 | p90 | min | max |
  |---|---|---|---|---|
  | before (main) | 6.59 / 6.58 ms | 7.54 / 7.99 ms | 5.52 | 10.4 |
  | after | 4.86, 4.82, 4.62 ms | 5.84, 6.08, 5.49 ms | 3.6 to 3.97 | 7.0 to 11.7 |

  The debug floor on this VM is `theseus --version` at 3.5 ms (exec of a 38 MB debug binary plus clap), so a debug run
  cannot reach 5 ms at p90 here. Whether the release build does is the maintainer's check.

## Live check for the maintainer

```
scripts/build.sh --profile release-thin
target/release-thin/theseus-sim bench status --runs 300 --sessions 1000 --check   # p90 <= 5 ms, exit 0
THESEUS_SOCKET=<scratch sock> target/release-thin/theseus status --short           # same line as before
```
Compare against the previous install's `theseus` with the same scratch daemon: the two lines must be identical. Also
`theseus status --short` from the new CLI against the operator's running (old) daemon must still print its line
(fallback), without touching it beyond what the CLI already does.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (built with `CARGO_PROFILE_DEV_DEBUG=line-tables-only`,
because this session's disk allowance filled twice at full debug info; the first full-suite run failed 310 tests from
ENOSPC, so it was discarded). Phases door, keel, fmt, shape, features, clippy, cockpit, test build, reader rule all pass.
Suite: 4,386 tests, 34 failed:
- 33 are the known root-VM L1 failures: `theseus-sandbox::contract` (19), `theseus-sandbox::bench`
  (spawn_100), `theseusd::sandbox` (13).
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` times out at 21 s.
  It is not on the known list. It fails identically alone on the base commit (I stashed my change and ran it), so it is not
  this change. A finding for the maintainer.
The phases after the suite I ran by hand: protocol types (the TS is committed and clean), `cargo deny --offline check`
(advisories, bans, licenses, sources ok), `scripts/keel-guard.py` (0 findings). The benches were skipped by
`THESEUS_GATE_NO_BENCH=1`.

## Left, uncertain, for the owner

- **Should `bench status --check` join the gate?** Not in the debug gate: it needs a release binary (debug cannot reach
  5 ms on a 4-core VM, and a debug floor is 3.5 ms). I would add it to the release bench phase, which
  size-rss-budgets (theseus-2m7jg) already adds; I did not edit the gate.
- **theseus-e5dw** (the seen count includes executions this daemon does not know, so ◆1 never clears): not fixed. `◆` comes
  from `seen::done_count` over the whole seen file; the fix needs the daemon's execution set or a per-daemon seen file,
  which is wider than `--short`. I only merged its read with `displayed()` (`seen::counts`).
- Remaining fixed cost is the process: exec of the binary and clap's parse of the full command tree. A pre-clap fast path
  for `[--socket S] status --short` would save about 1 ms in debug, but it duplicates argument parsing, so I left it.
- `status.short` sends every failed execution (they stay on the board for good). With very many failures the answer
  grows with them, not with sessions. If that ever matters, cap by newest and send a count.
- The seen file still holds a mark per execution ever known and is parsed in full each draw (once now).
- Docs to change: `crates/theseus/AGENTS.md` (the status.rs entry: `--short` asks `status.short`), the spec's protocol
  table (new method), and theseus-core's AGENTS.md "push" paragraph (the board keeps `Short`).
- Store format: untouched (no stored record changed).

## Keel findings expected

None. keel-guard: 0 findings.
