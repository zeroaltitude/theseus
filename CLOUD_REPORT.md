# CLOUD REPORT: health-o1 (theseus-id8d)

Branch `cloud/20261010-health-o1`, from `main` at 2fd1f654 (store format 26). No format bump: no stored record gained
a field. Two projections are renamed, so every store builds their terms again once, after serving:
`terms.kernel.1` became `terms.kernel.2` (the new `ot` term), and `projection.core.2` became `projection.core.3`.

## The measurements, on this VM (4 cores, release-thin)

The store is `theseus-sim synth-store --sessions 200000` (1.6 GB). `theseus-sim bench lifecycle --phases
cold,seed,health --runs 5`, the same harness (this branch's theseus-sim) against each daemon, alternated in two
rounds. Both rounds ran while the gate built and ran beside them, so the absolute numbers are loaded. The quiet run is
the one taken before the gate started.

| | main 2fd1f654 (2 loaded rounds) | this branch (2 loaded rounds) | this branch (quiet) |
|---|---|---|---|
| seed, first `executions.watch` p50 | 418 / 386 ms | 5.3 / 4.1 ms | 3.8 ms (p95 5.1) |
| health, own connection each, p50 | 916 / 916 ms | 0.8 / 0.7 ms | 0.7 ms (p95 1.0) |
| cold start to first health p50 | 1082 / 1299 ms | 83 / 149 ms | 69 ms |

The daemon's log on the quiet run says `push: board seeded elapsed_us≈15000 executions=1000`. The seed meets both
targets: the 250 ms budget and the 50 ms target. Health meets its 5 ms target.

**Not this change: the cold start misses its 50 ms budget at 200,000 sessions, on main and on this branch.** On both,
the daemon's own clock puts about 61 to 124 ms in `core`, almost all of it in `day_ceiling` (daily-ceiling). Read
before serving, its cost grows with the store. On main, health then adds about 900 ms on top. That makes it a FAST
defect on main, outside this row's files. I left it alone and suggest a row of its own: `day_ceiling` from a kept sum,
or read after serving.

## Steps

### 1. Health from counters: `0a29238`
- **Found:** health's parked-tasks count read every open execution and decoded each one to find the tasks
  (`parked.rs`). A store of parked conversations paid one record read per conversation on every call.
- **Changed:** the kernel's terms keep `ot` on a task that has not ended. `Kernel::open_tasks` reads only those, and
  health uses it. The board (step 3) keeps its view and question counts as entries change, so health counts nothing.
  Health's fields and words are unchanged.
- **Tests** (`rpc/tests_health_counts.rs`):
  - `health_counts_equal_a_recount_after_mixed_changes_and_a_restart`: opened, retired, cancelled, crashed, then a
    restart. Each count equals a full recount.
  - `health_reads_no_record_of_an_open_conversation`: a health answer reads the same 5 records with 200 more open
    conversations.
- **Planted revert:** health back to `open_executions()`. `health_reads_no_record_of_an_open_conversation` failed:
  `left: 205, right: 5`.

### 2. The tender's status cached for 1 s: `7584a2d`
- **Found:** every health call opened a new connection to the tender and read its status (`tender.rs`).
- **Changed:** `tender/status_cache.rs` keeps one status read, which serves every caller for `STATUS_EVERY` (1 s).
  Callers do not stack reads. When the tender does not answer, health gives the last status it read. Past
  `STALE_AFTER` (3 min), the index line leads with `stale: its status is 3 min old, and its socket did not answer
  (...)`, where before it showed only an age (theseus-uazd). The status stays one struct, so index-memory can add
  fields to the same read.
- **Tests** (`tests_status_cache.rs`): `health_calls_inside_a_second_make_one_status_read`,
  `one_read_serves_a_second_and_stale_is_minutes`, `a_tender_that_does_not_answer_gives_its_last_status_marked_stale`,
  and the unit `an_old_status_says_stale_plainly`. `tests_tender.rs`'s supervisor keeps a zero cache window, so its
  tests still ask the tender every call.
- **Planted revert:** the cache's `fresh()` always missed. Two tests failed:
  - `health_calls_inside_a_second_make_one_status_read`: `left: 7, right: 1`.
  - `a_tender_that_does_not_answer_gives_its_last_status_marked_stale`: `left: 3, right: 2`.

### 3. The push's seed by terms: `d958e54`
- **Found:** the seed read every action ever and every execution (`push.rs`).
- **Changed:** the seed now reads, by the store's terms:
  - the actions not settled;
  - the executions that need you or work: an active state, a due time or wakes, or a task not ended;
  - the parents of those;
  - the 1,000 most recently written executions (`push/hot.rs`).

  The rest stay cold:
  - `executions.watch`'s `total` still counts them (`Kernel::count_executions`).
  - `session.wait` loads a cold session's execution when it asks (`Push::view_or_load`).
  - A frame that touches a cold execution loads it.

  The board is indexed by session.
- **Tests** (`push/tests/seed.rs`):
  - `the_seed_by_terms_holds_what_every_action_ever_gives`: property-style, over many random seeds of mixed stores.
    The seed by terms gives the same views as the seed from every action.
  - `the_kept_counts_equal_a_recount_after_mixed_frames`.
  - `a_board_left_cold_answers_as_the_full_read`.
  - `the_seed_reads_no_settled_action_and_no_cold_execution`: 300 parked conversations and 400 settled calls. The seed
    reads 41 records, against a limit of 60; reading every record would be 700.
- **Planted revert:** the seed back to `actions_at()` (every action).
  `the_seed_reads_no_settled_action_and_no_cold_execution` failed: it held all 300 executions where 10 were asked.
  `the_seed_by_terms_holds_what_every_action_ever_gives` passed under the revert, as it should, because it is the
  equivalence oracle.

### 4. The bench: `ec1ff08`
- `bench lifecycle` has a `seed` budget of 250 ms and a new `health` phase: 5 calls after the seed, each on its own
  connection, budget 5 ms. Both are set for `--sessions 200000`. history.rs's CSV gains the health columns. Its test's
  assertions are rewritten for the new column, not removed.
- `crates/theseus-core/AGENTS.md`: the seed's reads and the status cache, in the push's and the tender's entries.

## Proof

- Targeted run, `cargo nextest run` over theseus-core's push, tests_push, tests_health_counts, tests_status_cache,
  tests_tender, session_wait and executions_watch, and theseus-kernel's terms tests: **53 passed, 0 failed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `python3 scripts/keel-guard.py --base 2fd1f654`: `keel: ok (2fd1f65..the working tree; 22 files changed, 0 findings acked)`.
- Each step was checked alone (`cargo check -p theseus-core --tests` with the later steps stashed) before its commit.
- No runs under load: the task asked for none for these tests. The two A/B rounds above ran beside the gate.

## Live check for the maintainer

```
theseus-sim synth-store --dir /tmp/s200k --sessions 200000
theseus-sim bench lifecycle --theseusd target/release-thin/theseusd --store /tmp/s200k --phases cold,seed,health --runs 10
```

The seed and health rows should be well under 250 ms and 5 ms; on this VM they were 4 ms and 0.7 ms. With
`main`'s theseusd, the same command gives a seed of about 400 ms and health of about 900 ms. The cold row misses on
both, for the `day_ceiling` reason above.

On a scratch daemon of a copy of a real store: `theseus --socket <s> health` should give the same fields and counts as
main's build. With its tender stopped for more than 3 minutes, the index line should start `stale:`.

## Left, uncertain, or for the owner

- **Cold executions.** An execution parked on input and older than the 1,000 written last is not on the board until a
  frame touches it or `session.wait` asks. `executions.watch` lists only the board, though its `total` counts all.
  Every surface lists by recency and caps at 200, so nothing visible changes. A client that pages deep into the board
  would not see the cold ones. The paging belongs to cli-pages (`session.list` and `execution.list`, which read the
  store, not the board).
- **`RECENT_WALK`** caps the records walked to find the recent ones at 64,000. A store whose recent executions were
  each rewritten many times finds fewer than 1,000.
- **`day_ceiling` at start** (above) is the one FAST miss at 200,000 sessions.
- **Docs for the maintainer:**
  - spec §9: the seed's 250 ms and health's 5 ms budgets at 200,000 sessions;
  - Part III: this item;
  - docs/status.md: the row.

## Keel findings expected

None. The history.rs test is a rewritten assertion, not a removed one. The new budgets are additions.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` over the whole tree, before the last commit:
- Before the suite, all green: keel 0 findings, fmt, shape, features, clippy, cockpit, test build, and the reader
  rule (15 passed).
- **Suite:** 3,692 run, 3,659 passed, **33 failed**, 44 skipped. The 33 are exactly the known L1 failures on this
  root VM (theseus-pv6i):
  - 19 in theseus-sandbox's contract tests;
  - its bench's `spawn_100`;
  - 13 in theseusd::sandbox.

  Nothing else failed, and no timing test flaked.
- **The phases after the suite, run by hand, all pass:**
  - protocol types: unchanged;
  - `theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames and tool 9 frames, both at budget;
  - `cargo deny --offline check`: advisories, bans, licenses and sources ok.

  The lifecycle and jobs benches are skipped under NO_BENCH. The release lifecycle runs above stand in for them.

So the gate counts green by the task's rule.
