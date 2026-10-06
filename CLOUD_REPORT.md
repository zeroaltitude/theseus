# Cloud report: imported-skip (theseus-7087)

Branch `cloud/20261006-imported-skip`, from `main` at 79be321 (store format 23). Started 16:38 UTC,
report written at 19:15 UTC.

Commits:
- `4a2dda6` store, rpc: the whole session lists skip imported sessions by key, unread (theseus-7087)
- `592a9a4` store, rpc: a session page steps over the import's births in one walk (theseus-7087)
- `1be8661` rpc: the imported-lists test checks its pages against one births read, and imports 2,000 (theseus-7087)

**The store's format is unchanged (23).** No stored record, field, or kind changed; both new reads are index walks
over tables the index already keeps (`bykey`, `bybirth`). No new dependency.

## Step 1: the whole list and every whole read skip imported sessions by key (`4a2dda6`)

**Found.** `Core::session_list` → `sessions_by_activity` → `Store::list_sessions` read and decoded every SESSION
record (`latest_of_kind`), and `session_list` then dropped the imported ones. The other whole reads:
`session_page`'s fallback (while the index's shape is built; it decoded and `retain`ed), `compilation_list`'s
current compilations, `confirm_list` (through `sessions_by_activity`), the learning tender's `task_briefs`, and
health's fallback `session_totals`.

**Changed.**
- theseus-store: `Store::latest_of_kind_where(kind, keep: &dyn Fn(&str) -> bool)`: the trait's default filters
  `latest_of_kind`; `WalStore` walks the kind's key table alone (`RedbIndex::positions_of_keys_where`) and reads only
  the kept keys' records (`read_many`). A failed key costs its index row; its record is never `pread`.
- theseus-store: `records_read_here()`, a per-thread count of records read from the log (`Inner::read` and
  `Inner::listed`, i.e. every `get`, `latest_by_key`, and list read), beside `frames_written_here`. It is what the
  tests count. Cost: one thread-local `Cell` increment per record read.
- theseus-core: `Store::live_sessions` = `latest_of_kind_where(SESSION, !import::is_imported)`, and the callers:
  - `sessions_by_activity` (the whole `session.list`, `confirm.list`): live only. `session_list`'s own
    `.filter(imported.is_none())` is gone (the read does it).
  - `session_page`'s fallback: live only.
  - `compilation_list`: live only (an imported session has no `compilation_id`: `SessionRecord::with_id`).
  - `learning/system.rs` `task_briefs`: live only (an imported session has no `task`). The one-line edit there.
  - **Kept whole: health's fallback `session_totals`** (`rpc/methods.rs`, used only while the projection's terms are
    not whole). Its projected answer's `sessions` count is the index's sum over every SESSION key, imported ones
    included, so the fallback must count them too to give the same number. It runs only on a store an older build
    wrote last, until `build_terms` finishes after serving.
  - Test-only callers of `list_sessions` (aws/tests_restore, rpc/tests, mcp/tests) are unchanged.

## Step 2: a page steps over the imported run in one walk (`592a9a4`)

**Found.** `sessions_paged` called `newest_keys(SESSION, cursor, n)` in a loop; each call read the records of all
`n` keys it met (`read_many`), imported or not, and skipped imported ones after. An import's births are the newest,
so a page reaching back past them read every imported record, `n` per step.

**Chosen: the range read, not a key space of their own.** `RedbIndex::keys_by_birth_where` walks `bybirth` once in
one read transaction and steps over a key the predicate fails without looking up its latest position in `bykey`;
`Store::newest_keys_where` reads only the kept keys' records. `newest_keys` is now it with every key kept. This
keeps format 23. Moving imported sessions into their own kind would have meant a format bump and an old-layout
reader, and a change to how `import/write.rs` writes and erases them.

**The answers are unchanged.** `more` (and so the `older` cursor) still counts every key past the page, kept or
not, exactly as the old loop's `(more || !last)` did: a full page whose only older keys are imported still gives a
cursor, and the next page is empty with none. Proved page by page and cursor by cursor against the old loop (below).
`sessions_paged` is now one call and a decode.

**What is left.** The births walk still visits each imported key's `bybirth` row (no record read, no `bykey`
lookup): about 2.5 ms for 21,151 in a release build (the numbers below). The whole list's key walk likewise visits
each imported `bykey` row (about 2 ms). Both are flat in the import's size per row, not per record. If the owner
wants them gone too: the whole list can skip the `ses_ep` prefix as a key range (imported keys are contiguous in key
order, since `p` is no hex digit), with no format change; the page can only skip the run without walking it if
imported births live elsewhere (their own kind or key space), which is a format bump. I did neither: the brief
asks for one read, and these are a few ms at a 3 s poll.

## Step 3: tests

- `theseus-store` `store/tests_keyed.rs` (new):
  - `latest_of_kind_where_reads_only_the_kept_keys_records`: 19 live keys, 600 skipped (`ses_ep…`) in two runs,
    some keys rewritten: the answer equals `latest_of_kind` filtered, and exactly 19 records are read.
  - `newest_keys_where_steps_over_a_skipped_run_in_one_walk`: for n in 1, 2, 3, 5, 7, 20, 1000, every page from the
    newest back equals the old `n`-at-a-time walk's (keys, births, positions) and its cursor, and each page reads
    exactly its own records.
- `theseus-core` `rpc/tests_imported.rs` (new),
  `the_session_lists_read_no_imported_session_and_answer_as_before`: 3 live sessions, an import of 1,000 synthetic
  episodes (built as `import/tests.rs` builds lines, through `write::import_batch`), a session parked on an
  `fs.write` question, a live session, a second import of 1,000 under another tag, 2 more live sessions, then an
  erase of the first tag. At each stage:
  - the whole list equals the old read (every record decoded, imported dropped, sorted by activity), and every page
    of n = 1, 2, 3, 4, 20, 1000 equals the old walk's page and cursor (since `1be8661` computed from one read of
    every birth by the old loop's rule, which `tests_keyed` holds the loop itself to), each live session once;
  - the reads of the whole list, a page of 20, the page past the first import's run, and `confirm.list` are each
    under 60 records, and the second import and the erase add **not one record** to any of them (`[29, 29, 4, 29]`
    before and after);
  - `confirm.list` lists the parked session's question;
  - after the erase the lists still answer as before and 2,007 session keys remain (erase tombstones, never drops).
  (`592a9a4` imported 2 x 1,500; `1be8661` cut it to 2 x 1,000 for the load runs below. The read counts quoted
  in the reverts are from the 1,500 version.)
- The import's existing tests pass unchanged (`import::tests`, 10, including
  `the_session_list_leaves_imported_sessions_out`, `an_erased_tag_leaves_nothing_to_recall`, and the re-import
  refusal in `an_episode_file_imports_once_and_a_second_run_skips_every_episode`), as do `rpc::tests_lists`.

## Proof

- Targeted: `cargo nextest run --workspace -E 'test(tests_keyed) | test(tests_imported) | test(tests_lists) |
  test(/^import::/)'`: 22 passed.
- Planted reverts (each restored with `cp` and `touch`, `git status` clean after):
  1. `sessions_by_activity` back on `list_sessions` + `retain(imported.is_none())`: `tests_imported` fails,
     reads `[3029, 29, 4, 3029]` vs `[1529, 29, 4, 1529]` (whole list and `confirm.list` read every import).
  2. `sessions_paged` back to the `n`-at-a-time loop over `newest_keys`: `tests_imported` fails,
     `[29, 3029, 1504, 29]` vs `[29, 1529, 1504, 29]` (and now also the absolute bound of 60).
  3. The predicate inverted in one caller (the page's `keep` lists only imported sessions): `tests_imported` fails at
     the equal-answers check, `([], None)` vs `(["ses_…"], Some(8))`.
  4. `confirm_list` alone back on the whole decode (`list_sessions` + `retain`): `tests_imported` fails at the
     bound (`confirm.list` read 1,529 records).
  5. In the store, `keys_by_birth_where` without its skip: `newest_keys_where_steps_over_a_skipped_run_in_one_walk`
     fails (a skipped key in the page).
- Under load (AGENTS.md's recipe: `nice -n 19`, four `while :; do :; done` loops at nice 0, killed by their pids):
  - `tests_keyed`, `tests_imported`, `rpc::tests_lists`, `import::` (22 tests), 5 runs on the `592a9a4` test:
    21 of 22 passed in every run; `tests_imported` itself took 100 to 120 s and met nextest's 120 s kill in 3 of
    5 (TIMEOUT, not a failed assertion: unloaded it took 1.7 s, 1.2 s of it the 3,000-session import). That is
    the test's own CPU under starvation, so `1be8661` imports 2,000 and checks pages against one births read.
  - `tests_imported` after `1be8661`: 3 of 3 passed under the same load, 72.7 s, 72.0 s, 72.5 s (1.0 s unloaded).
    It is still the heaviest test of its file under that load; on the owner's 16 cores it should be far from the
    kill, but if the maintainer sees it near 120 s there, cutting the imports to 2 x 500 keeps every check.
  - Planted revert 3 re-run against `1be8661`'s reference: fails at the equal-answers check, `([], None)` vs
    `(["ses_…"], Some(8))`.
- The gate: below.

## FAST: measured (release-thin, this VM)

`main` at 79be321 vs this branch, each a scratch daemon on a fresh state dir (`[discord]`, `[web]`, `[index]` off;
a `file:` key; model endpoint on a closed port). 3 live sessions (`session.open`), then `theseus import openclaw`
of 21,151 synthetic episodes (2 messages each, 20 MB), then 2 more live sessions; then `theseus import erase`. 20
calls each: `session.list {}` and `session.list {"n": 20}` over one raw socket connection, and the CLI's `theseus
sessions` (a process each, connect included).

| | main p50 / max | branch p50 / max |
|---|---|---|
| before the import: whole list (rpc) | 0.1 / 0.4 ms | 0.1 / 0.4 ms |
| before the import: page of 20 (rpc) | 0.1 / 0.2 ms | 0.1 / 0.2 ms |
| before the import: `theseus sessions` | 3.3 / 4.4 ms | 3.0 / 4.2 ms |
| after 21,151 imported: whole list (rpc) | **146.4 / 177.9 ms** | **2.2 / 2.8 ms** |
| after 21,151 imported: page of 20 (rpc) | **46.4 / 47.7 ms** | **2.5 / 3.7 ms** |
| after 21,151 imported: `theseus sessions` | 145.9 / 177.6 ms | 5.2 / 5.9 ms |
| after the erase: whole list (rpc) | 136.9 / 155.1 ms | 2.4 / 3.1 ms |
| after the erase: page of 20 (rpc) | 46.9 / 52.3 ms | 2.7 / 4.0 ms |
| after the erase: `theseus sessions` | 138.5 / 168.2 ms | 6.0 / 7.3 ms |

The import itself: 3.7 s wall on main (1,606 ms in the daemon: 42,302 nodes in 83 frames), 3.5 s on the branch
(1,487 ms); the erase 1.5 s / 1.4 s (16 frames). Every answer held 5 sessions, a page 5 with no cursor, on both.

**Not on the start path or a turn's path.** The reads changed are `session.list` (`rpc/methods.rs`
`session_list_of` → `session_list` / `session_page`), `confirm.list` (`rpc/confirms.rs`), `compilation.list`
(`rpc/methods.rs` `compilation_list`), and the learning tender's nightly run (`learning/system.rs` `task_briefs`).
None runs before the socket answers or inside `TurnRunner::run`. The one change on hot paths everywhere is the
thread-local increment in `Inner::read`/`listed`, a `Cell` add per record read. The lifecycle bench is untouched;
the gate ran with `THESEUS_GATE_NO_BENCH=1` as the brief says.

## The live check (the maintainer's)

1. Episodes in the import's format, invented names only (`episodes.py`):

```python
#!/usr/bin/env python3
"""Write N synthetic episodes (import format 1) as JSON Lines: invented names and text only.
usage: episodes.py N TAG OUT.jsonl"""
import hashlib, json, sys
n, tag, out = int(sys.argv[1]), sys.argv[2], sys.argv[3]
with open(out, "w") as f:
    for i in range(n):
        day = 1 + i % 28
        v = {"format": 1, "import_tag": tag, "episode_id": "ep_%064x" % (0x5EED0000 + i),
             "source": "openclaw-sessions", "agent": None,
             "place": {"kind": "dm", "name": "heron-%d" % (i % 9)},
             "as_of": {"start": "2026-03-%02dT09:00:00Z" % day, "end": "2026-03-%02dT09:20:00Z" % day},
             "labels": {"sensitivity": "personal", "topic": ["heron/count"]},
             "summary": None,
             "messages": [
                 {"idx": 0, "time": "2026-03-%02dT09:00:00Z" % day, "author": "wren", "integrity": "operator",
                  "text": "Heron count %d: twelve at the weir." % i, "unit": "unit-%d-0" % i, "sha256": "ab" * 32},
                 {"idx": 1, "time": "2026-03-%02dT09:01:00Z" % day, "author": "agent:main", "integrity": "agent",
                  "text": "Noted: count %d, twelve at the weir." % i, "unit": "unit-%d-1" % i, "sha256": "cd" * 32}]}
        canon = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        v["hash"] = hashlib.sha256(canon.encode()).hexdigest()
        f.write(json.dumps(v) + "\n")
```

2. A scratch daemon on a fresh state dir, from the install build (`B=~/.local/bin`), in a directory of its own:

```sh
S=$(mktemp -d /tmp/imported-skip.XXXX); cd $S
python3 episodes.py 21151 heron-2026-03 heron.jsonl
echo sk-not-a-key > key && chmod 600 key
cat > config.toml <<EOF
[secrets]
anthropic_api_key = "file:$S/key"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[model]
api_base = "http://127.0.0.1:9"
EOF
$B/theseusd --config config.toml --state-dir state --socket sock 2>d.log & D=$!
for i in 1 2 3; do $B/theseus --socket sock rpc session.open '{}' >/dev/null; done
time $B/theseus --socket sock import openclaw heron.jsonl     # read 21151, imported 21151, rejected 0
t() { for i in $(seq 20); do /usr/bin/time -f %e "$@" >/dev/null; done 2>&1 | sort -n | sed -n '10p;20p'; }
t $B/theseus --socket sock sessions                           # p50 and max
t $B/theseus --socket sock rpc session.list '{"n": 20}'
$B/theseus --socket sock sessions | wc -l                     # 3: no imported session listed
$B/theseus --socket sock import erase --tag heron-2026-03     # 21151 sessions tombstoned
t $B/theseus --socket sock sessions
t $B/theseus --socket sock rpc session.list '{"n": 20}'
$B/theseus --socket sock import openclaw heron.jsonl          # imported 0, rejected 21151: "an erased episode is not imported again"
$B/theseus --socket sock shutdown; wait $D
```

What each should show: `sessions` lists the 3 live sessions only, before and after the erase; each list's timing
after the import stays within a few ms of the CLI's own start (here: `theseus sessions` 5 to 6 ms p50 vs main's
146 ms; a debug build is slower, but the import should no longer show at all in the whole list, and only as a few ms
of index walk in a page). `/usr/bin/time`'s `%e` has 10 ms resolution; the bench above used a socket client
(`bench.py`, the same steps over one raw JSON-RPC connection, in `/tmp/bench` on this VM, not committed) for ms.

## Docs (not edited)

- theseus-core's `AGENTS.md` (the import's paragraph) is updated in `592a9a4`: it said the list skips imported
  sessions by key in `rpc/pages.rs`; it now names `Store::live_sessions` and `newest_keys_where`, and the one read
  kept whole (health's fallback totals).
- Spec / design: wherever Part III's soul-import item (theseus-0lrr.6) says `session.list` leaves imported sessions
  out, add: "Since theseus-7087 the lists never read them: the whole list, `confirm.list`, `compilation.list` and the
  learning tender read the live sessions by key (`Store::live_sessions`, theseus-store's `latest_of_kind_where`), and
  a page walks the births once, stepping over imported keys unread (`newest_keys_where`). After 21,151 imported
  sessions, a release build answers the whole list in 2.2 ms p50 (146 ms before) and a page of 20 in 2.5 ms (46 ms)."
  theseus-store's AGENTS.md could list the two `_where` reads with `keys_ending` as reads that skip by key.

## Uncertain, and for the owner

- The page's cursor rule is kept as it was: a full page whose only older keys are imported (or erased) still returns
  an `older` cursor, and the next page is empty. That is the old answer; a client sees one empty page. Changing it
  (`more` counting only kept keys) is one line in `keys_by_birth_where`, but it changes answers, which the brief
  forbade.
- The residual few ms of index walk (above) grows with the import; a later import of 10x would cost about 10x of it.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each of the three commits (the last at
19:04 to 19:11 UTC on `1be8661`'s tree). fmt, shape, features, clippy,
cockpit, test build and the reader rule pass. The suite: 2,911 tests, 2,878 passed, **33 failed, all the known L1
ones** (theseus-sandbox's contract tests and `spawn_100`, theseusd's `sandbox` tests: a root daemon's L1 job with
no job cgroup, theseus-pv6i). Then by hand: `protocol.gen` unchanged (no protocol type changed), `cargo deny
--offline check`: advisories, bans, licenses, sources ok. The benches were skipped (`THESEUS_GATE_NO_BENCH=1`).

One gate run on the second commit's tree also failed 10 theseusd daemon tests (`job_approval`, `mcp_server`,
`job_latency`, `reaping`, `push`, `outbox`): the VM's disk allowance had filled (`No space left on device` in the
rerun's build; `target/debug/incremental` was 17 GB). I deleted `target/debug/incremental` and stale
`/tmp/theseus-test-*` dirs and ran the gate again from the start: only the 33 known failures. No test's flakiness
is claimed from that run.
