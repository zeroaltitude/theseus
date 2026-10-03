# Cloud report (not for main)

Branch `cloud/20261003-ledger-reads`, from `main` at `a59b7c1`.
Task: reads that don't grow with history — theseus-vm3n.5, theseus-96w2, theseus-tphr.

Started 20:00 UTC, 2026-10-03. 4-core VM, running as root, no sccache.

## Commits (oldest first)

- `8f6866a` store, ledger: `ledger.tail` reads through the index's tags, time, and cursors; counts in one row (theseus-vm3n.5)
- `dd53c05` ledger: a page and its total are one snapshot; the history-check test comes off the flaky list (theseus-tphr)
- `3b6d848` store, rpc: the polled lists read their page through the index — `action.list`, `node.list`, `session.history`, `session.list { n, before }` (theseus-96w2)
- `ab710b6` sim: `synth-store --ledger-rows` adds a long history behind the parked sessions (theseus-vm3n.5)

The report commit follows as the branch's last.

---

## theseus-vm3n.5 — `ledger.tail` through the index; counts in O(1)

### What I found

Every read the task named walked history. `ledger.tail` with a `kind` or
`session_id` filter read `n × 50` rows and filtered them; health's
`ledger_rows`, the lists' `total`s, and the kernel's per-state counts each
walked a per-kind range of the index that grows with history.

### What I changed (`8f6866a`)

The index (`crates/theseus-store/src/index.rs`) keeps, in the one redb
transaction that already indexes each frame:

- **counts**: records per kind (`counts`), keys per kind (`keycounts`),
  records per scope (`scopecounts`), and keys per term (`termcounts`, beside
  the projection's `terms`). `count_of_kind`, `count_keys`, `count_in_scope`,
  and `count_by_terms` now read one row. A count is bumped only for a record
  `bykind` did not already hold, because an open replays the WAL past the
  checkpoint into an index that may already hold some of it (the `fresh`
  flags in `apply`/`apply_bulk`).
- **a clock per kind** (`clock`) — the newest frame time its records have had
  — and **`bytime`**: (kind, minute) → the first position whose clock is in
  that minute. A frame whose time stepped back takes the clock's time, so the
  clock, and `bytime` with it, only grows with position, and a window of time
  is one contiguous stretch of positions. **A clock step back is handled**
  this way: a row written while the clock was ahead of the host counts at the
  clock's time (so a window never splits around it), while its own
  `at_unix_ms` is unchanged and is what the row reports.
- **tags** (`tagged`): a ledger row's kind (`k:`), session (`s:`), and the two
  together (`ks:`), each with the row's position (`pages::tags_of`).

`Store::page` (`crates/theseus-store/src/pages.rs`, `Page`/`PageOut`) answers
one page of a kind through these: any of some tags, a window (`since_ms`,
`until_ms`), `after` (oldest first, as the ledger walk always has) or `before`
(a page back from the newest), with the kind's count **from the same read
transaction as the page** (this is also the theseus-tphr fix). `first_at`
finds a window's first position in O(log n): the first `bytime` minute at or
after the time, then at most that minute's records.

`ledger.tail` (`crates/theseus-core/src/rpc/methods.rs`) reads through
`Store::page`, with three new optional params — `before`, `since_ms`,
`until_ms` — and one new result field, `older` (the next `before`). Every
current call's params and answer are unchanged; a filtered `after`-page's
`next` is now exact (absent at the end, where before it was set whenever the
page was full).

**Shape change, rebuilt from the WAL.** The index's shape mark is
`index.shape.3`, set at each checkpoint. An index an older build wrote has no
such mark at its checkpoint; the open (`shape_or_empty`) drops only the new
tables, still reads **only the WAL's tail**, and `WalStore::build_shape`
rebuilds them after serving (`Core::build_store_shape`, beside the existing
`build_store_terms`), a stretch of records at a time, then one transaction
that counts every table (`recount`). Until the shape is whole, the counts
walk and a filtered `ledger.tail` scans (`ledger_tail_scanned`), exactly as
before. An index with no checkpoint at all is emptied and replayed whole —
the store's existing rule.

### How I proved it

Store tests (`crates/theseus-store/src/store/tests_pages.rs`), all pass:
- `each_count_is_kept_with_every_append_and_equals_a_walk` — 300 mixed
  batches, counts checked against a walk every 37 batches, after a reopen that
  replays the tail, and after a bulk rebuild.
- `a_filtered_page_equals_the_scans_answer` — 400 randomized pages (kind,
  session, `after`/`before`, `limit`) each equal to a scan-then-filter.
- `a_window_keeps_its_bounds_and_a_clock_that_steps_back_does_not_split_it`
  — inclusive bounds to the ms across minutes; a clock stepped back 4 minutes
  then forward; the window stays one stretch and each row still reports its
  own time; the same after a WAL rebuild.
- `cursor_pages_under_concurrent_writes_have_no_duplicate_or_gap` — paging
  back by `before` and forward by `after` while a second thread appends.
- `each_terms_count_follows_its_keys_and_equals_a_walk` — `termcounts`
  against a walk through reopen and bulk rebuild.
- `an_index_of_another_shape_is_built_after_serving_and_then_answers_whole`
  — an older-shape index (its new tables deleted, no shape mark); the open
  reads only the tail, counts walk and a page by tag is `None`, then
  `build_shape` with appends between its stretches makes everything equal a
  walk; the next checkpoint marks it; the history-check mark survives.

Core test `crates/theseus-core/src/rpc/tests_ledger.rs`:
- `ledger_tail_through_the_index_answers_as_the_scan_did` — 300 randomized
  `ledger.tail` calls (renamed kinds included) equal to the old scan's filter,
  with exact `total`, `next`, and `older`.
- `ledger_tail_pages_back_by_before_and_keeps_to_a_window`.

**Planted reverts** (store, each caught then restored, `git status` clean):
- a `turn.ended` row's kind tag not written → `a_filtered_page…` and the
  older-shape test fail.
- a multi-record batch bumping its kind count by one instead of its length →
  `each_count…` and the older-shape test fail (e.g. 21 vs 28).

### Measurements

Synthetic store: 10,000 parked sessions + 470,000 history ledger rows
(587,000 records, 155 MB WAL), `release-thin`. BASE = `main` a59b7c1, NEW =
this branch. p50/p95 over 12 runs, warmed; VmHWM is the daemon's peak over the
run.

| call | BASE p50/p95 ms | NEW p50/p95 ms | resp | BASE/NEW peak RSS |
|---|---|---|---|---|
| `action.list {n:500}` | 165 / 206 | **3.8 / 6.2** | 166 KB | 625 / 300 MB |
| `ledger.tail {n:2000, kind}` | 147 / 191 | **4.1 / 6.4** | 182 KB | 685 / 300 MB |
| `ledger.tail {n:400, kind}` | 78 / 102 | **1.8 / 2.4** | 72 KB | |
| `ledger.tail {n:1, kind}` | 40 / 50 | **0.1 / 0.3** | 0.1 KB | |
| `node.list {kind, n:2000}` | 96 / 120 | **0.1 / 0.2** | 0.1 KB | |
| `health {}` | 46 / 56 | **0.5 / 0.6** | 6 KB | |
| `ledger.tail {n:400, session_id}` | 83 / 90 | **0.7 / 0.9** | 9.2 KB | |

(`session.list {}` and `execution.list {}` with no params are unchanged —
~200 ms, 5+ MB — by design; their paginated forms are under theseus-96w2.)

**FAST.** Plain turn, `bench turn --runs 10 --burst 10`, quiet disk:
- BASE: 5 frames, p50 23–26 ms, the 5 frames ~0.9 ms of it.
- NEW: 5 frames, p50 26–28 ms, the 5 frames ~0.7–0.8 ms of it.
The frame count is held at 5 by the gate's `turn --check` (it passed on every
gate). The extra index puts per row ride in the frame's existing redb
transaction and add no measurable commit time.

---

## theseus-96w2 — the polled lists read a page, not every record

### What I changed (`3b6d848`)

The index keeps each key's **first position** (`born`: kind,key → position;
`bybirth`: kind,position → key), so the newest keys of a kind by when each was
first written are a range read (`RedbIndex::keys_by_birth`,
`Store::newest_keys`). An action's first record is its plan; a session's its
open. `tags_of` also now tags a node (its body kind and session) and an action
(its execution, `x:`). All of this is in the `index.shape.3` shape, so an
older build's index is built after serving as above.

Chosen among the task's options, with the measurements below:
- **`action.list`**: the newest `n` by birth, or an execution's by its `x:`
  tag; each action in its latest state, sorted by `planned_at_ms` as before;
  `total` is the action-key count (one row). (`crates/theseus-core/src/rpc/pages.rs`)
- **`node.list`** with a kind, a session, or both: the tag's newest `n`.
- **`session.history`** with `n`: the session's newest `n` nodes — unless a
  question waits in the session, whose card reads the gate's record on its
  call's node wherever that is, so then the whole session is read, as before.
- **`session.list`** gains optional `n` and `before` (and the answer `older`):
  the newest `n` sessions by when each was opened, paged back. With neither,
  every session by activity, as today. `session.list { ids }` is unchanged.

**`execution.list` I left as it was** — `self.kernel.executions()`. It reads
every execution and takes no params; bounding it well needs a cursor shape and
a decision about ordering (by state? by activity?) that the push board already
covers for the live set, and it was the one list the task marked "choose with
measurements, and say why". Its unpaginated cost is the same order as
`session.list {}` (~180 ms, 5 MB at 10k). I judged a good `execution.list`
page its own follow-up rather than a guess here; flagging it for the
maintainer. No web client is changed, so nothing regresses.

While the shape is built after serving, each list falls back to the read it
replaced.

### How I proved it

`crates/theseus-core/src/rpc/tests_lists.rs` (all pass):
- `action_list_through_the_index_answers_as_every_action_read_did` — 300
  frames of plans with older actions changing state, 60 randomized queries
  (every `n`, with/without execution) equal to `kernel.actions()` sorted and
  truncated, same `total`.
- `node_list_and_history_through_the_index_answer_as_the_scan_did` — 60
  queries of `node.list` and `session.history` equal to the session scan.
- `session_list_pages_back_from_the_newest_by_when_each_was_opened` — paging
  back by `older` while sessions are opened between pages, each session once,
  newest first; without `n`, every session.

Store: `births_agree` in `tests_pages.rs` checks each key's birth against its
first position in a walk, and the newest-by-birth page, across reopen, bulk
rebuild, and the older-shape build.

**Planted revert**: a new session's key not born → the session-list test and
the store's `each_count…` fail. Caught, restored, `git status` clean.

### Measurements (same store)

| call | BASE p50/p95 ms | NEW p50/p95 ms | resp |
|---|---|---|---|
| `session.list {n:200}` | 205 / 234* | **3.8 / 5.7** | 109 KB |
| `session.list {n:50}` | 197 / 244* | **1.3 / 1.5** | 27 KB |
| `action.list {n:500, execution_id}` | 141 / 193 | **0.9 / 1.1** | 16 KB |
| `node.list {session_id, n:100}` | 2.2 / 2.9 | **0.3 / 0.4** | 1.4 KB |

(*BASE ignores the unknown `n` and returns every session, 5.4 MB.)

---

## theseus-tphr — a page and its total are one snapshot

### What I found and changed (`dd53c05`)

`the_history_check_after_serving_finds_a_corrupt_frame_and_says_so` compared
health's `refused_records` with `total - shown` from a `ledger.tail` whose
rows and `total` were two separate reads; a row written after serving (a
check, the tender) between them counted once more (5 vs 6 under load). Since
theseus-vm3n.5's commit, `ledger.tail` takes rows and `total` from one
`Store::page` read transaction (and, while the shape is built, counts in the
same scan). This commit adds the test holding that property and removes the
daemon test from `.config/nextest.toml`'s flaky list.

### How I proved it

`a_pages_total_is_counted_in_the_same_snapshot_as_its_rows` reads the whole
ledger while a writer appends 800 rows; every answer's `total` must equal the
rows shown.

**Planted revert**: `total` read as a second `ledger_len()` → fails on every
run (read N: 6 vs 5, the issue's shape).

**Under load** (`nice -n 19` test binary beside four `nice 0` busy loops, load
average 6–7 on 4 cores), 20 runs each:
- `a_pages_total_is_counted_in_the_same_snapshot_as_its_rows`: **20/20 pass**;
  with the revert planted: **0/20 pass** (20/20 fail).
- `the_history_check_after_serving_finds_a_corrupt_frame_and_says_so`:
  **20/20 pass** (and still 20/20 with the planted revert, since it reads
  through the fixed path too; the unit test is the sharp witness).

---

## The gate

`THESEUS_GATE_LOCK=inner THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the branch
tip (`ab710b6`): fmt, shape, clippy (`-D warnings`), reader rule, protocol
types, deny (offline), web/cockpit lint+test+build, and `web dist` all pass.
The turn bench holds the plain turn at **5 frames**.

The suite fails on exactly the two cases the task lists as known on this VM,
and nothing else:
- `theseus-core tests_output::the_cores_output_matches_its_golden` — the
  one-byte request-size drift from a duration's digit (theseus-6a7o), all
  three tries.
- `theseus-sandbox::contract clause_09_limits` — root is exempt from
  `RLIMIT_NPROC` here (theseus-pv6i).

Per the task, I ran the gate's post-suite phases (`machine_checks` tail plus
`deny`/`web`/`cockpit`/`web dist`) by hand after each suite; all green. So each
commit counts green.

## Left / uncertain

- **`execution.list`** is unchanged (above). It is the remaining list that
  reads every record; I recommend a follow-up that pages it, once its order is
  decided.
- **The cockpit/web half** (bounding history to a window, then the push) is
  explicitly later work; no web client was changed, and all current calls keep
  their params and answers. `session.list`'s new `n`/`before` and `older`, and
  `ledger.tail`'s `before`/`since_ms`/`until_ms`, are the server surface the
  cockpit's half will use.
- **Docs**: I did not touch the spec, `docs/status.md`, or `docs/design/`. For
  the maintainer to record: Part III item for this step; the new index tables
  (`counts`, `keycounts`, `scopecounts`, `termcounts`, `clock`, `bytime`,
  `tagged`, `born`, `bybirth`) and the `index.shape.3` shape in
  `theseus-store/AGENTS.md`; the `ledger.tail` and `session.list` new params.
  `MANIFEST_FORMAT` and `kinds::SCHEMAS` are untouched (the index is a
  rebuildable projection; no record layout changed).
