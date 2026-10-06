# Cloud report: the judge's reads cut to their answers (theseus-wse2, theseus-b8e2, theseus-e1ei, theseus-cf5c)

Branch `cloud/20261005-judge-reads`, from main at 4a449460 (store format 22, unchanged). Started 03:06 UTC,
report at about 04:35 UTC.

| Step | Commit |
|---|---|
| wse2: judge.list newest first | 8043656 |
| b8e2: the brake's day from today's rows | e2ff85f |
| e1ei: the continuation's window held | a64f1ea |
| cf5c: the rules read only what can still change | e02c30c |

## Records read per read, before and after

These are the in-test counts of records each read decodes, scan then page.

| Read | Store | Before (scan) | After |
|---|---|---|---|
| `judge.list`, default (50) | 10,000 judgments over 30 days, 5 packs, 3 sessions, labels | 11,429 | 51 |
| `judge.list --session` | same | 11,429 | 51 |
| `judge.list --pack security` (id) | same | 4,572 | 51 |
| `judge.list --pack security.v3` (version) | same | 4,572 | 101 |
| `judge.list -n 7` | same | 11,429 | 8 |
| `judge.list -n 500` | same | 11,429 | 501 |
| `judge.list` pack+session+since+20 | same | 2,286 | 21 |
| `judge.list --pack loop.v2` (matches nothing) | same | 2,286 | 2,000 (every loop row; see Uncertain) |
| brake's day (`read_today`) | 5,000 security judgments over 30 days, 250 notices, 100 noise labels | 5,363 | 362 in-test; in production only today's notices and labels (see below) |
| learning run, sessions read by the rules | 200 old judged sessions and 1 open | 201 (first run) | 1 (second run, a day later); 0 (third) |

The brake's 362 counts every notice and label row in the test store. A core test can't set the store's frame clock
(theseus-store's `test_clock` is crate-private), so every row written in a test carries today's frame time and the
page from midnight reads them all. What the page saves is in the scan's 5,000 judgments, which it never decodes. The
midnight cut is shown by the second brake test: read as of tomorrow, the page decodes 0 records.

## wse2: judge.list newest first (8043656)

**Found.** The code matched the brief. `judge_list` scanned each scope from position 0 (all 14 embedded packs' scopes
when no pack is given; 10 distinct ids), decoded every record, and only then cut to `limit`. The index counts a kind
(`COUNTS`) and a scope (`SCOPECOUNTS`) without decoding. It does not count a tag (`k:judge.call`, `ks:…`), and
nothing counts per pack or version. So `matched` can't be exact without decoding.

**Changed.**
- `rpc/judge.rs` pages back through `ledger_page` with `before` cursors:
  - It reads the `k:judge.call` tag, or `ks:judge.call\u{1}<session>` when a session is given.
  - `since` is the page's `since_ms`, and the row-time check is kept.
  - Only rows whose `scope` is a listed scope are decoded.
  - It stops once `limit` rows pass the filter and one more matches.
- When the page answers `None` (the index's shape is being built), it scans as before (`judge_list_scanned`).
  `judge_list_read(p, paged)` returns the cost for the tests.
- **The protocol changed.** `JudgeListResult.matched` is exact when the read reached the start of history.
  Otherwise it is a floor: the limit plus the one older match that proves there are more. A new field,
  `more: bool` (serde default `false`, so an older daemon's exact answer reads as exact), says which.
  - Its doc is updated and `cockpit/src/protocol.gen/JudgeListResult.ts` is regenerated.
  - The cockpit shows "N of M+" when `more`.
- The CLI's `cmd.rs` is untouched, as the brief asked. With `more` it still prints
  `(20 of 21 judgments in …; --n shows more)`, where 21 is the floor. **What the CLI should print:**
  `(the newest 20; more match in judge:loop; --n shows more)` when `r.more`, and the exact
  `(20 of M judgments …)` only when `!r.more`.
- The surfaces test's limit-1 listing now expects `matched` 2 with `more` (it expected 3).
- The AGENTS.md line for `judge.list` is updated.

**Proved.**
- `tests_judge_reads::judge_list_pages_from_the_newest_and_answers_as_the_scan_did` covers 11 filters (none, pack id,
  version, a pack nothing has, a pack no build embeds, session, since, limit 7, limit 500, all together, fewer than
  the limit).
  - For each, the page gives the same judgments as the scan, by position and id, in the same order.
  - `matched` is exact wherever the scan matched no more than the limit, and `limit + 1` with `more` otherwise.
  - Decode counts are asserted: 51 for the default and for a session, 8 for limit 7.
- **Planted revert:** the page's filter without its version clause (only in the paged path, so the scan still
  filters). It fails this test ("version: the same judgments, in order") and
  `tests_judge_surfaces::judge_list_filters_by_pack_session_and_time`.
- The commit message says the scan decoded 11,430; the measured figure is 11,429.

## b8e2: the brake's day from today's rows (e2ff85f)

**Found.** As the brief said: `read_today` scanned all of `judge:security` once a run. The pause row is keyed by its
day (`paused_key`), and `brakes_today` already read it by that key.

**Changed.** `judge/notice.rs`: `read_day(store, today, now_ms, paged)`.
- The pause is read by its key.
- Today's `tool.notified` and `judge.label` rows come from a page over both kind tags with
  `since_ms = local_midnight(now)`, newest first, decoding only rows scoped `judge:security`.
- Each row is still counted by its own time, through the same `count` rules as before.
- When the page answers `None`, it scans as before.
- `brake_day` now takes `now`, so the page's midnight is the brake's own (skewable) clock.
- **The dated position mark** the issue suggested was not needed: the page serves wherever the index's shape is
  built, and the scan covers the rest.

**Proved.** Two tests in `tests_judge_reads.rs`:
- `the_brakes_day_is_todays_rows_as_the_scan_counted_them`. The store holds 5,000 judgments, earlier days' notices,
  noise labels and pause, and today's 4 notices, 2 noise labels and pause. It also holds the near misses that must
  not count:
  - a notice and a noise label 1 ms before midnight;
  - a policy's notice;
  - a system's noise label;
  - a noise label on v1;
  - a judge's notice in `judge:loop`.

  The page's `Day` equals the scan's: 4 notices, 2 noise, today's pause. The page starts at `local_midnight(now)`.
- `the_brakes_page_begins_at_its_midnight`: read as of tomorrow, the day equals the scan's (empty) and 0 records are
  decoded.
- **Planted revert:** the page from yesterday's midnight (`local_midnight(now) - 24 h`). Both tests fail: the first
  on `since_ms`, the second with 361 records decoded.

## e1ei: the continuation's window held (a64f1ea)

`tests_learning::a_continuation_counts_only_inside_its_window` writes "go on" through the store at three times
(`created_at_ms` set by the test), each relative to the judged turn's last node:
- 10 min + 1 ms after it, which takes no label;
- 9 min after it, which is labelled;
- exactly 10 min after it, which is labelled because the rule is `<=`.

**Planted revert:** `after <= CONTINUATION_MS &&` removed. The test fails, with the late judgment labelled. The
existing `loops_system_labels_…` test still passes under the plant, so it never held the window.

## cf5c: the rules read only what can still change (e02c30c)

**Found** (differences from the brief):
- The security rule does not read only the scope. It reads each judgment's kernel action (`self.kernel.action(call)`)
  every run, which grows with history.
- The route rule reads only the scope, but for each routed judgment it walks the whole scope looking for the next
  one, which is quadratic in memory.
- `loop_labels` never reads `now`. The classify rule's "over" reads `now` (an hour after the turn's last node).
- How long a call can wait before it expires: `[kernel] confirm_ttl_secs`, 900 s by default (`expire_questions`,
  `planned_at_ms + confirm_ttl_ms`). A budget question never expires, but security judges tool calls, not budget
  questions.

**Changed.**
- The run's `learning.last_run` META value gains `through`: the store's last position, taken before the scopes are
  read.
- The next run builds `system::Cut { through, at_ms }` from it. A judgment is closed, and none of its reads happen,
  when both of these hold:
  - its row position is at or before `through`;
  - its time plus its window (`open_ms`) plus `MARGIN_MS` (1 h) is before the last run's time.
- The windows (`open_ms`):

  | Pack | Window |
  |---|---|
  | loop, task class | 24 h (false completion) |
  | loop, other | 10 min (continuation) |
  | classify | 24 h (see Uncertain) |
  | security | `confirm_ttl_ms` |
  | route | 10 min |
  | rerank, others | not cut (no window) |

- **Cut per pack, by the rules that apply, and why each:**
  - loop and classify are cut because they read whole sessions.
  - security is cut because it reads an action per judgment.
  - route is cut because of its scope walk per routed judgment.
  - rerank keeps its own path.
- With no mark, or a mark without `through` (every mark written before this build, and `tests_prove`'s), the run
  walks everything, as today.
- The false-completion rule's task briefs now page sessions newest first through the index's births
  (`newest_keys(SESSION)`). It stops at sessions created before the earliest open task judgment less the margin, and
  falls back to `list_sessions` while the index has no births.
- **Log line:** `run_learning_read` logs `learning: the rules read what can still change` with `sessions_read`,
  `actions_read`, `tasks_read`, `judgments_closed` and `first`. The tender's `learning: the report ran` line gains
  `sessions_read` and `judgments_closed`.
- `run_learning` keeps its signature. `system_labels(pack, scope, now)` stays as a test-only wrapper because
  `tests_route` calls it.

**Store format.** No bump: `through` is a key inside the existing `learning.last_run` META value, read as optional.
The AGENTS.md rule speaks of fields of stored records, so **the maintainer should confirm** this reading.

**Proved.** `tests_learning::a_second_run_reads_only_what_can_still_change`:
- The store holds 200 old judged sessions (every tenth with a "go on" inside its window) and one judgment written
  5 minutes before the first run, whose "go on" is written after it.
- The first run reads 201 sessions and writes 20 labels.
- The second run, a day later, reads 1 session, leaves 200 closed, and writes 1 label (the open judgment's
  continuation). In total there are 21 labels, none written twice.
- A third run reads 0 sessions and writes 0 labels.

**Planted revert:** the cut removed (`if false && closed(seen)`). The test fails: "only the judgment still inside its
window", 201 against 1.

## Suites, under load, and the gate

- The judge, notices, learning, ladder, prove, route, security, categorize, replay, audit, backfill and learn-loop
  suites, and `tests_registry`: 172/172 passed on the final tree.
- theseus-core's suite as a whole ran in each gate (below).
- Each new test (and the changed surfaces test) under load: three runs at `nice -n 19` beside four busy loops at
  nice 0, started and killed by their pids. 6/6 passed each run; the 10,000-judgment test is slow under that load
  (about 70 s).
  - The busy loops were `yes > /dev/null`: this environment refused the brief's `sh -c 'while :; do :; done'` form.
  - A first loaded attempt spent its load on a rebuild. Its first run passed under load; its next two ran after I
    stopped the loops, and I count them as unloaded.
- **The gate** (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`) ran before each of the four commits.
  - Each time it failed only in the suite.
  - Commits 2, 3 and 4: exactly 33 failures, all L1 (theseus-sandbox's contract tests and `spawn_100`, theseusd's
    `sandbox` tests), the known root-without-cgroup refusal (theseus-pv6i).
  - Commit 1's run also failed seven load-timing tests, none in code this branch touches:
    - `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy` (policy 0, not 5);
    - three `telemetry::tests_resumed` tests (the telemetry3 lane's resumed spans);
    - `theseusd::job_latency::a_quick_jobs_result_comes_by_the_drains_word_not_a_poll`;
    - `theseusd::mcp_server::a_jobs_process_that_opens_a_session_through_mcp_passes_on_its_hold`;
    - `theseusd::job_approval::a_job_that_kills_its_wrapper_leaves_an_orphan_that_cannot_answer` ("no orphan in
      30 s", a positive wait that timed out, not a negative assertion).

    All seven passed alone at once, and none failed in the next three gates.
  - After each suite I ran the phases that follow it myself: protocol types clean; the turn bench 5 frames plain and
    9 with a tool, within budget; `cargo deny --offline check` ok; the benches skipped (NO_BENCH).
  - fmt, shape, features, clippy and the cockpit build passed in every gate.
- No dependency was added; Cargo.lock and package-lock.json are unchanged.

## Live check for the maintainer

Use a copy of a stopped state dir with months of judgments, a scratch daemon on each build (main's and this
branch's), and Discord and the web off.

1. Run `theseus --socket <sock> judge log -n 20`, then the same with `--pack loop`, `--pack loop.v1` and
   `--session <id>`.
   - The judgment lines should be identical on both builds.
   - Where more matched, main prints `(20 of M judgments in …)` with M exact; this branch prints `(20 of 21 …)`.
     That is the floor, until cmd.rs reads `more`.
   - `--json` shows `"more": true` there.
2. Run `theseus --socket <sock> judge report` twice.
   - The daemon's log shows `learning: the rules read what can still change` for each run.
   - The first run of this build has `first=true` (main's mark has no `through`) and reads every judged session.
   - The second has `first=false`, `judgments_closed` about the history's size, and `sessions_read` only for
     judgments from the last day or so.
   - The second report's numbers equal the first's, unless a judgment inside its window took a label in between.
3. Open the cockpit's Judgment section for two minutes and run `pidstat -p <pid> 1`.
   - Expect the CPU per 4 s poll to fall from a scan of every judge scope to a few dozen decodes.
   - The header reads "500 of 501+ …" when more match.

## Left and uncertain

- **A rare pack or version filter walks the whole tag.** The tags carry kind and session, not pack, so `--pack
  loop.v2` (nothing matches) decodes every `judge:loop` row: 2,000 of 10,000. A pack id filter skips other scopes'
  rows without decoding them, but still steps through their postings.
  - The fix needs the store: a `judge.call` row tagged by its scope, or a reverse read of a scope. That is a
    theseus-store change, so I made none.
- **A future-stamped row.** The page's `since` uses the ledger's clock (frame times, monotone). A row whose own
  `at_unix_ms` is later than its frame's time would be dropped by a page from after its frame, where the scan kept
  it. The writers stamp rows before they commit, so this should not happen in production; hand-written test rows
  could do it.
- **Classify's window (24 h)** bounds a turn's length. A classified turn still running more than about 25 h after its
  message, across a run, would never take its label. TURN_OVER_MS (1 h) is measured from the turn's last node, which
  isn't known without reading the session.
- **The task briefs' floor** assumes a task's session is created at most an hour before its first node (they are
  written in one frame).
- **The 1-hour margin** is my choice, covering a late sink row, a clock step and a run's own length.

## Docs the maintainer should write

- Part III: one item for these four issues.
- `docs/status.md`: "judge.list, the brake and the learning rules read what each answer needs".
- The design or spec text for `judge.list` (§2.13): `matched` is a floor when `more`.
- crates/theseus-core/AGENTS.md is updated in the commits.
