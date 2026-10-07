# CLOUD_REPORT: imported-walk (theseus-26jo, theseus-ve34)

Branch `cloud/20261006-imported-walk`, from main at 57f265f2 (store format 23). Started 01:11 UTC, report at
about 04:00 UTC. Seven commits on top of the task commit, each gated (below). No store-format bump, no index
table changed, no dependency added, no protocol type changed (protocol.gen untouched).

| Step | Commit | Subject |
|---|---|---|
| 1 | 2816d96d | store: count the index rows the key walks and the births walk visit (theseus-26jo) |
| 2 | ec6818c4 | store: the whole session list steps past the import's key run without visiting it (theseus-26jo) |
| 3 | eebd203b | store: the births walk tests a key before it allocates or looks it up (theseus-26jo) |
| 4 | 38bb33b5 | rpc: compilation.list {session_id} marks current from that session's record alone (theseus-26jo) |
| 5 | 70717a68 | rpc: test the pages of a store whose import came before every live session (theseus-ve34) |
| 6 | f6f9eea9 | rpc: count compilation.list's reads past a second import (theseus-ve34) |
| FAST | 38df51df | rpc: an ignored measure of the session lists past a full import (theseus-26jo) |

## What the code says, against the brief

- `import::SESSION_PREFIX` is `ses_ep`, `is_imported` is `starts_with(SESSION_PREFIX)`, and `session_id_of` writes
  `ses_ep` and the episode's 64 hex digits: as the brief says. The run in key order is after every `ses_e<hex>` and
  before `ses_eq`/`ses_f`.
- **R2 as the brief states it was already not the code on main.** main's `keys_by_birth_where` already tested `keep`
  before its `bykey` lookup; what it paid per imported birth was the row and an allocation
  (`from_utf8_lossy(..).into_owned()`), not a lookup. So step 3's saving is the allocation alone, and the R2 plant is
  a regression to a worse state than main's, which the new tests now catch.
- The brief's main-side numbers hold roughly here (4-core VM): whole list 2.5 ms p50 release (brief: about 2 ms),
  8 to 10 ms debug (brief: 12 to 20); a page of 20 3.5 ms release (brief: 2.5), 9 to 11 ms debug (brief: 14 to 32).

## Step 1: a count of index rows visited (2816d96d)

**Found.** No count of index work existed beside `records_read_here`, so a walk over imported rows was invisible to
the tests (they counted records read).

**Changed.** `theseus_store::index_rows_here()` (store.rs, beside `records_read_here`, exported from lib.rs), fed by
`count_rows` in index.rs: each row `keys_of_kind`, `keys_with_prefix` and the key walk yield, each row the births
walk meets (including the one that tells it more remain), each lookup of one key (`latest_position`, and the births
walk's lookup of a kept key).

**Proof.** tests_keyed `the_index_rows_a_walk_visits_are_counted_on_its_thread`: 619 rows for a 619-key walk, 1 for
a lookup, 7 for a page of 3 (3 rows, 3 lookups, the row that says more). theseus-store suite 84/84 at that commit.

## Step 2: the whole list skips the run (ec6818c4)

**Changed.**
- `RedbIndex::positions_of_keys_except(kind, skip)`: two range reads of `bykey`, `kind..kind+skip` and
  `past_prefix(kind+skip)..kind+1` (`past_prefix` raises the last byte below 0xff by one and drops what follows:
  `ses_ep` -> `ses_eq`), so no row of the run is visited.
- `Store::latest_of_kind_except(kind, skip)`: the default reads every record of the kind and filters by
  `!starts_with(skip)` (as the old default did); `WalStore` overrides it with the walk.
- `Store::live_sessions` (core store.rs) passes `import::SESSION_PREFIX`.
- **Retired** `Store::latest_of_kind_where` and `RedbIndex::positions_of_keys_where`: `live_sessions` was their one
  caller (grep of the workspace). tests_keyed's test of it moved to the new method.
- theseus-core AGENTS.md: the one sentence on the lists' walk names `latest_of_kind_except` (and, at step 4, one
  session's `compilation.list`).

**Tests.** tests_keyed `latest_of_kind_except_reads_and_visits_only_the_kept_keys` (19 records, 19 rows, out of 619
keys) and `latest_of_kind_except_keeps_every_key_outside_the_run` (keys `ses_e`, `ses_eo`, `ses_ep`, `ses_ep0`,
`ses_epz`, `ses_eq`, `ses_f`, `zÿ`; skips `ses_ep`, `ses_e`, `ses_eq`, `""`, `x`, `zÿ`, `ÿ`, each equal to a
filter). tests_imported's `list_rows`: the index rows of the whole list, `confirm.list` and `compilation.list` are
**[6, 6, 5]** after the first import, and the same after the second import and after the erase.

**Plant.** `live_sessions` back on `latest_of_kind_where` (the predicate walk): **[1006, 1006, 1005]**, and the
test fails at its bound.

## Step 3: the births walk tests before it allocates (eebd203b)

**Changed.** `keys_by_birth_where` tests `keep` on the borrowed `Cow` from `from_utf8_lossy`, and allocates
(`into_owned`) only a kept key, after its lookup. The walk over the import's births stays (skipping them unvisited
needs their births in a key space of their own: a format change, not made). learning/system.rs's task-brief walk
gains it with no edit.

**Tests.** tests_keyed `a_page_past_a_skipped_run_looks_up_only_its_kept_keys`: a page of 8 past a run of 300
visits 309 birth rows and 8 lookups, **317** exactly. tests_imported: the page past the import's 1,000 births visits
fewer than 1,060 rows.

**Plant R2** (a `bykey` lookup before the skip): tests_keyed sees **625** against 317, tests_imported **2,009**
against its bound; both fail. (The step-1 count test fails too: 10 against 7.)

**What the allocation saved, measured** (the ignored measure, 21,151 imported, page of 20, which crosses the run;
M = ec6818c4, before step 3; B = this branch; run M B B M):
- debug: M 10.79 / 10.57 ms p50, B 8.49 / 9.27 ms p50: about 15 % off the page.
- release (opt-level 3, LTO off for the test build): M 2.96 / 2.80 ms, B 3.08 / 3.01 ms: **no gain measurable** in
  release; the difference is inside the run-to-run noise (A's own two runs differ by up to 2 ms in debug). The
  allocator is cheap next to redb's row decode; the change is kept because it is free and removes a per-row
  allocation from a walk that grows with every import.

## Step 4: compilation.list {session_id} (38bb33b5)

**Changed.** rpc/methods.rs `compilation_list` only: with `session_id`, `current` comes from
`store.get_session(sid)` alone; without, from `live_sessions` as before. A session's `compilation_id` is only ever
set by its own turn (turn.rs:3317, `session.compilation_id = Some(c.id)` of the compile it just ran), so the
session's own record is the whole answer. An imported id reads its own record, which has no compilation.

**Test.** tests_imported: the parked session's list has exactly one `current`, and reads the scan of its scope plus
one record. **Plant** (the session branch back on `live_sessions`): 27 reads against 23; fails.

**The whole `compilation.list`: would reading only the sessions its page names be cheaper?** Yes, in the usual
case. The page is at most `n` compilations (default 50, at most 500) from `recent_compilations`, naming at most `n`
distinct sessions, usually a few active ones; reading those by key costs one lookup and one record each, against
every live session's record now. It is cheaper whenever the live sessions outnumber the page's distinct sessions,
which is the normal state of a store with history. Not done here (the brief asked only whether); a follow-up of a
few lines in `compilation_list`: collect the page's `session_id`s into a set and `get_session` each.

`confirm.list`'s read left as it is.

## Step 5: the import-first pages (70717a68)

`the_pages_of_a_store_whose_import_came_first_answer_as_before`: 300 imported, then 3 live; every page of n = 1,
2, 3, 4, 5, 20 and 1000, cursor by cursor, equals `page_as_before`, and each live session is seen once; a full page
of 3 keeps its cursor and the next page is empty.

**Plant R3** (`out.len() == limit` moved after `keep`): fails at `n 1 from Some(609)`: the walk answered no cursor,
`page_as_before` one at 608. theseus-store's suite (86) and the older tests_imported test both pass under R3, as
the brief found: this test is the one that guards it.

## Step 6: compilation.list's reads (f6f9eea9)

`list_reads` now counts `compilation.list` whole and of the parked session: **[29, 29, 4, 29, 6, 23]** after the
first import (whole list, page of 20, page past the run, confirm.list, compilation.list, compilation.list of one),
the same after the second import and after the erase. **Plant R4** (`compilation_list` back on `list_sessions`):
the whole compilation.list reads **1,006** records and the test fails.

## FAST

Ignored test `rpc::tests_imported::the_session_lists_past_a_full_import_timed` (38df51df): 3 live sessions, 21,151
imported, 2 live; 20 calls each after a warm one. A = 2816d96d (main plus the row counter only, so main's walks),
M = ec6818c4, B = this branch's code at f6f9eea9 (38df51df adds only the measure). Run in the order A M B B M A per
build:

| build | variant | whole list p50 / max | rows, records | page of 20 p50 / max | rows, records |
|---|---|---|---|---|---|
| debug | A | 9.86 / 12.29 ms; 7.84 / 8.17 ms | 21,156, 5 | 11.48 / 13.16; 9.38 / 10.29 ms | 21,161, 5 |
| debug | M | 0.04 / 0.10; 0.06 / 0.14 ms | 5, 5 | 10.79 / 13.20; 10.57 / 12.95 ms | 21,161, 5 |
| debug | B | 0.05 / 0.07; 0.04 / 0.07 ms | 5, 5 | 8.49 / 11.86; 9.27 / 11.77 ms | 21,161, 5 |
| release | A | 2.51 / 3.24; 2.51 / 3.61 ms | 21,156, 5 | 3.62 / 4.21; 3.51 / 3.82 ms | 21,161, 5 |
| release | M | 0.01 / 0.01; 0.01 / 0.03 ms | 5, 5 | 2.96 / 3.19; 2.80 / 2.93 ms | 21,161, 5 |
| release | B | 0.01 / 0.02; 0.01 / 0.04 ms | 5, 5 | 3.08 / 3.62; 3.01 / 3.35 ms | 21,161, 5 |

The whole list (and with it `confirm.list` and `compilation.list`) goes from 21,156 index rows to 5, from 2.5 ms to
0.01 ms release and 8 to 10 ms to 0.05 ms debug. A page that crosses the run still visits each imported birth row:
that is the format change the brief leaves out. (Release here is `cargo test --release` with
`profile.release.lto=false`, `codegen-units=16` passed by `--config`, since a fat-LTO test build was too slow and
large for this VM; the crates are opt-level 3 either way.)

**Nothing here is on the start path or a turn's.** The changed code runs from `Store::live_sessions` (called by
rpc/methods.rs `sessions_by_activity` for `session.list {}`, `session_page`'s fallback, `compilation_list`;
rpc/confirms.rs `confirm_list`; learning/system.rs's task-brief fallback), `keys_by_birth_where` (rpc/pages.rs
`sessions_paged`; learning/system.rs's task-brief walk, a background tender), and `compilation_list`: all RPC reads
or the learning tender. The kernel's startup frame, the open, and a turn's path (turn.rs, compile, toolrun) call
none of them. The turn bench's frames (5 plain, 9 with a tool) are unchanged at every commit.

## Live check (the maintainer's)

I ran this once here on a scratch daemon of this branch's debug build; the maintainer should run it on the
install build, against main's too.

Episodes generator (invented names and text; writes the import's format with its canonical hash):

```python
#!/usr/bin/env python3
"""episodes.py N [TAG]: write N synthetic episodes in the import's format (invented names and text)."""
import hashlib, json, sys

n, tag = int(sys.argv[1]), sys.argv[2] if len(sys.argv) > 2 else "tern-2026-04"
for i in range(n):
    v = {
        "format": 1, "import_tag": tag, "episode_id": "ep_%064x" % (0x7E440000 + i),
        "source": "openclaw-sessions", "agent": None,
        "place": {"kind": "dm", "name": "tern-%d" % (i % 9)},
        "as_of": {"start": "2026-04-03T09:00:00Z", "end": "2026-04-03T09:20:00Z"},
        "labels": {"sensitivity": "personal", "topic": ["tern/count"]},
        "summary": None,
        "messages": [{"idx": 0, "time": "2026-04-03T09:00:00Z", "author": "wren",
                      "integrity": "operator", "text": "Tern count %d: forty on the north spit." % i,
                      "unit": "unit-%d" % i, "sha256": "ab" * 32}],
    }
    canon = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    v["hash"] = hashlib.sha256(canon.encode()).hexdigest()
    print(json.dumps(v))
```

Timer (`time20.sh LABEL CMD...`: 20 runs, p50 and max wall time):

```bash
#!/bin/bash
label=$1; shift
for i in $(seq 1 20); do
  s=$(date +%s%N); "$@" > /dev/null; e=$(date +%s%N)
  echo $(( (e - s) / 1000 ))
done | sort -n | awk -v l="$label" '{a[NR]=$1} END {printf "%s: p50 %.1f ms, max %.1f ms\n", l, a[10]/1000, a[20]/1000}'
```

Commands (a fresh state dir; web and Discord off; a stand-in key so the config loads):

```bash
D=$(mktemp -d /tmp/walk-XXXX); B=~/.local/bin   # or the build's target dir
echo "sk-stand-in-not-a-key" > $D/key && chmod 600 $D/key
printf '[web]\nenabled = false\n\n[discord]\nenabled = false\n\n[secrets]\nanthropic_api_key = "file:%s/key"\n' $D > $D/theseus.toml
$B/theseusd --config $D/theseus.toml --socket $D/sock --state-dir $D/state > $D/daemon.log 2>&1 & echo $! > $D/pid
until [ -S $D/sock ]; do sleep 0.2; done
for i in 1 2 3; do $B/theseus --socket $D/sock sessions open; done
./time20.sh "sessions (before)"        $B/theseus --socket $D/sock sessions
./time20.sh "session.list {} (before)" $B/theseus --socket $D/sock rpc session.list '{}'
./time20.sh "session.list n 20 (before)" $B/theseus --socket $D/sock rpc session.list '{"n": 20}'
python3 episodes.py 21151 > $D/episodes.jsonl
$B/theseus --socket $D/sock import openclaw $D/episodes.jsonl
for i in 1 2; do $B/theseus --socket $D/sock sessions open; done
./time20.sh "sessions (after)"         $B/theseus --socket $D/sock sessions
./time20.sh "session.list {} (after)"  $B/theseus --socket $D/sock rpc session.list '{}'
./time20.sh "session.list n 20 (after)" $B/theseus --socket $D/sock rpc session.list '{"n": 20}'
$B/theseus --socket $D/sock sessions | wc -l                        # 5: no imported session listed
SID=$($B/theseus --socket $D/sock sessions | head -1 | cut -f1)
$B/theseus --socket $D/sock rpc compilation.list "{\"session_id\": \"$SID\"}"
$B/theseus --socket $D/sock rpc compilation.list '{}'
$B/theseus --socket $D/sock shutdown; while kill -0 $(cat $D/pid) 2>/dev/null; do sleep 0.1; done
```

What it showed here (debug build of this branch; each CLI call includes the process spawn, about 7 ms; `rpc
health` 10.4 ms p50):
- the import: `read 21151, imported 21151, skipped 0, rejected 0 (21151 nodes in 83 frames, 4770 ms)`;
- `sessions` 7.2 ms p50 before and 7.2 ms after; `session.list {}` 7.4 ms after: the whole list costs nothing
  more after the import (on main it should rise by about 2.5 ms release, 8 to 10 ms debug, per the table);
- `session.list {"n": 20}` 7.1 ms before, 19.2 ms after (debug): the page still walks the 21,151 births, as
  expected; release should add about 3 ms;
- `sessions` lists the 5 live sessions; a page of 20 gives the 5 and `older: null`;
- `compilation.list` (whole and of one session) answers `{"compilations": []}`, since no turn ran (no model);
  with a model, a session's list should show its latest compilation `current: true`.

## Proof, offline

- theseus-store's suite: 86/86 at the head (84 at step 1).
- theseus-core's suite, `rpc::`, `import::`, `learning::` included, ran in every gate (`TZ=America/Phoenix`).
- Under load (AGENTS.md's recipe; the `sh -c` busy loop was refused by this environment, so the four loops were
  `yes > /dev/null` at nice 0, the tests at `nice -n 19`): tests_imported and tests_keyed, 7 tests, 3 runs, all
  passed (about 62 s each run).
- Each plant restored with `cp` from a saved copy and `touch`, then `git status` checked clean of it.

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` before each commit. Every run
failed only in the suite, on the 33 known L1 tests (theseus-sandbox's contract tests and `spawn_100`, theseusd's
sandbox tests: a root daemon with no job cgroup, theseus-pv6i). The phases after the suite were then run by hand
each time: protocol types unchanged; `theseus-sim bench turn --check --runs 5 --burst 0` (5 and 9 frames, ok); and
`cargo deny --offline check` (advisories, bans, licenses, sources ok). Benches skipped, as a lane's gate.
At the last commit (38df51df): 3030 tests run, 2997 passed, 33 failed (the L1 set), 25 skipped.

Other things met:
- **The disk allowance.** The second gate's first run failed 111 tests and 2 timeouts, every one a job refused by
  the disk floor ("the disk under the state dir has 133 MB free, below the floor of 1,024 MB"): the debug build's
  incremental cache had grown to 18 GB. I deleted `target/debug/incremental` and ran with `CARGO_INCREMENTAL=0`
  from then on; the rerun was the 33 alone. Not a finding about the code.
- **theseus-1g8j** (`learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`) failed
  once, in step 3's gate, and passed alone on rerun: the listed flake.
- No negative assertion failed.

## Left, uncertain, and for the owner

- **A page still walks the import's births.** Skipping them unvisited needs the import's births in a key space of
  their own (a store-format change), as the brief says; the page of 20 is 3 ms release and 9 ms debug after 21,151.
- **The whole `compilation.list`** could read only the sessions its page names (step 4): a small follow-up.
- **`latest_of_kind_where` is retired.** If another branch in flight calls it (none on main does), it moves to
  `latest_of_kind_except`, or the predicate walk comes back for it.
- `index_rows_here` counts only the walks the lists use (the key table's walks and lookups, and the births walk),
  not every index read (terms, pages, locations). That is the count the tests needed; widen it if another reader
  wants it.
- **Docs the maintainer may want** (I edited none): theseus-store's AGENTS.md "Tests" could name `index_rows_here`
  beside `records_read_here`; Part III's item for this step and `docs/status.md` should say the whole list no
  longer grows with an import and the page does, by one birth row per imported session.
