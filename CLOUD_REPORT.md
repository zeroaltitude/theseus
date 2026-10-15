# CLOUD_REPORT: work-digest round 2 (theseus-qkoi3)

Branch `cloud/20261013-work-digest-r2`. Commits: merge `f9c8e920` (round 1's work, a plain merge into today's main),
fix `db08386f`. Store format untouched (43; no field, no kind).

## The product bug: what I found

"Told once" failed across a restart because the ring and the ledger told a wake from two different facts.

- The ring's wake line came from the work board's *notice* for the wake. That notice is made when the due scan queues
  the conversation's turn (`views::due_wake`, deliberately "not a frame later when its turn takes the wake").
- The ledger path read the `wake.fired` row, which the turn writes when it takes the wake, a later frame (543 against
  549 in the reviewer's run).
- So (2): a client told at the notice's position had `to` = 543; after a restart the ledger path, asked from 543, told
  the timer again from the row at 549. And (1): a digest asked between the two frames, or a restart between them,
  had the notice told by the ring but no row yet in the ledger. The reviewer's "kind index lags" is this: `ledger.tail`
  showed the row at 549 because it was written after the restart, not because the page lagged. I found no index lag.
  `ledger_page` is `None` while the shape is built and the code falls back to the tail scan (8,000 rows); when shaped,
  the tags are put in the same transaction as the rows (`put_shaped`). I did not change either.
- The cursor was not at fault: the digest's `to` is the board's position, and the daemon keeps none per surface.

## What I changed (`db08386f`)

- `push/work.rs`, `push/work/digest.rs`: the board keeps its last 1,024 `wake.fired` rows with their own positions
  (`fired`, `fired_since`), and its ring "reach" (`complete`) also stops at the oldest kept row. O(1) per wake row,
  no frame, no store change.
- `rpc/digest.rs`: both paths word a wake from that one row with one function (`raw_of`, via `fired_raws` on the ring
  path); the ring's `Why::Woke` notices are no longer read (and the `woken_twice` dedupe went with them). The scope
  filter is one function (`in_scope`) for both paths.
- Join fixes (also in the merge): protocol `lib.rs` back to its 2,733 ceiling (`pub use work_digest::*; pub use
  work_watch::*;`, `fold`/`summary` renamed `fold_digest`/`digest_summary`, the method on the existing line); the
  Discord `/digest` is registered in `runtime/slash.rs` now (the commands moved there), count test 20 own + join/leave
  + tasks = 26; the CLI digest arm goes through `long_cmd` (renamed `long`) and `status::run` loses a helper
  (`mark_shown`) so both stay within clippy's 100 lines; the CLI test's stand-in daemon answers `status.short` and
  `work.watch`, which main's `status` reads now, and accepts several connections.

## How I proved it

- `theseusd::digest what_happened_while_a_client_was_away_is_told_once_from_where_it_left` (the row's own test):
  10 runs in a row, `--retries 0`: 10 of 10 green (with the rest of the row's 38 tests in the same invocation, see below).
  The same test on the pre-fix code: its one run before the fix passed here, so I did not reproduce the reviewer's 1-in-3
  on this VM, and say so; the fix rests on the cause above, the two new tests and the planted reverts.
- New daemon test `a_stop_between_telling_and_the_next_arrival_tells_nothing_twice` (tests/digest.rs): tell, then kill
  the daemon, restart: nothing from `to`, the same lines from where the client left (from the ledger); then a clean
  `shutdown`, restart, the same. Planted revert (the ring tells `Why::Woke` notices again, `fired_raws` off): it FAILED
  6 of 6 (a 40 s wait for exactly one woke line, which the ring then said twice); restored, `touch`ed, `git status`
  clean: green 5 of 5.
- New core test `tests_work::digest::a_wake_is_told_the_same_from_every_position_by_both_sources`: from every position
  of the scenario the ring and the ledger tell the same wakes. Planted revert (same): it FAILED ("from position 0",
  and `the_ring_and_the_ledger_agree_on_the_same_window` with it).
- The row's tests, run 10 times: `nextest -p theseusd -p theseus -p theseus-core -p theseus-protocol -p theseus-tui
  -p theseus-discord -E 'binary(digest) | test(tests_work::digest) | test(work_digest) | test(digest_)'`:
  38 tests, 10 runs, 38 of 38 each, `--retries 0`.
- `cargo fmt --check` ok, `cargo clippy --workspace --all-targets -- -D warnings` clean, `cargo deny --offline check`
  ok (advisories, bans, licenses, sources), shape ok, cockpit lint (warnings only, none new), `npm run test` 435 of
  435, `npm run build` ok, protocol's TypeScript regenerated (no diff after the merge's own).

## Review points, one by one

- Two races (ring vs ledger position; ledger lag): fixed as above; the lag was the second race's face, not an index
  lag. If you still see a ledger page missing a row after a restart, it is a store issue I did not find.
- Join fix (lib.rs ceiling, herdr helper, run()'s length): done; `own()` went with main's own socket computation,
  `run()` stays within 100 by the digest arm sharing `long`.
- Merge (Discord runtime/voice count test, protocol method list, TUI app.rs, CLI main.rs): resolved by hand against
  today's main; `runtime.rs` shrank (the commands moved to `slash.rs`); the TUI keeps main's `snapshot_read` and takes
  the row's look-at-the-board bookkeeping inside it.
- Noted by the reviewer (welcome's last sight in memory; a failing task covered by the mapping only): unchanged.
- Token budget: nothing added to any always-loaded set, no tool or schema; the digest is an RPC and surfaces.
- No budget, test, or ceiling loosened.

## Live check for the maintainer

1. Scratch daemon on a fresh state dir (the row's test does it): `cargo nextest run -p theseusd --test digest
   --retries 0` (2 tests, about 2 s each).
2. By hand: `theseus digest --since-seen` after five tasks, one failing, one asking and answered, a timer set;
   `theseusd` killed with SIGKILL; restart; the same command shows each line once; `theseus digest --since 10m`
   twice in a row (the second, `--since-seen`, shows nothing).

## What is left or uncertain

- The cockpit's recorded fixture `cockpit/test/fixtures/digest.json` was recorded when the ring told a wake by its
  own view (kind `wake`); the daemon now tells it as the ledger does (the conversation's or task's work ref). The
  cockpit's tests read the fixture, not a daemon, so they pass; re-record with `THESEUS_RECORD_DIGEST=<file>` at
  review if you want the exact shape.
- A wake that has no `wake.fired` row (none that I know of) is told by neither path now; before, the ring told it.
- Docs to change at review: the core AGENTS.md's `work.digest` paragraph (a wake is told from its `wake.fired` row, the
  board keeps the last 1,024) and the spec Part III item for this row.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` stops in the keel phase on this VM, before any build, on
commits already in main: it refuses two `Keel:` acks (238ecead06 and 1c2beb2747) because their signatures do not verify
here (`%G?` is `E`: this VM holds no signer keys), so `crates/theseus-sim/src/lifecycle.rs`'s "seed" budget reads as
raised. That is not this branch's change; I loosened nothing. So I ran the phases after it by hand: fmt, shape,
clippy, deny, cockpit (lint, test, build), and the full suite
(`TZ=America/Phoenix nextest run --workspace --no-fail-fast`: 5,967 tests, 5,932 passed, 35 failed on the first run).
The 35: the 33 known L1 failures here (theseus-sandbox's contract tests `clause_01..12`, `spawn_100`,
`the_jobs_bench_l1_row`, and theseusd's sandbox tests: the root daemon's L1 refusal, theseus-pv6i); one flake under
load, `theseus-core tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`
(not this row; passes alone; its `every 1m` series' cap assertion at line 532 is clock-dependent); and one of this
row's, `theseus::digest status_ends_with_one_line_when_something_happened_since_the_last_look`, which was the CLI
test's stand-in daemon answering `{}` to `status.short` and `work.watch`, fixed in `db08386f` (now 4 of 4). The tests
that passed or failed at the first suite run were not re-run in full after that fix; the row's tests were, 10 times.
The disk allowance ran out twice on this VM (a 21 GB debug target); I built with `CARGO_PROFILE_DEV_DEBUG=0` after a
`cargo clean`.
