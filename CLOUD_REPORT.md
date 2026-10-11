# CLOUD_REPORT: postmortem (theseus-m2tiu)

Branch `cloud/20261011-postmortem`. Commits: `3841867` (ledger.window), `d38121d` (theseus postmortem), `b3e60f5` (the
recorder), `71f411a` (execution.opened, the fixture recorded again). The last commit is this report.

## What I found

- Every fact the report needs is a ledger row, but the existing `ledger.tail` could not carry it: it has no scrub and
  no rule for a surface that reads no private text. So I added the read-only method the task allowed, `ledger.window`,
  and left `ledger.tail` as it is.
- The ledger's time index counts a row at the time it was **written**, not at its `at_unix_ms` (so a test that writes
  rows with made-up times must give a window that the write time meets; `ledger.window` then decides by each row's own
  time). The daemon's `execution.interrupted` rows come **before** the `startup.step` rows of the same start (the step
  rows are batched into the last frame), so a restart is begun by whichever of its rows comes first.
- The first start of a store is `startup.step` 1 with `last_position` 0: it is no restart and the report leaves it out.
- Not in the ledger, so not in the report: a tool failure's own text (the first line of its error), and a task
  report's first line. A failed call shows its tool, producer ("class") and duration; a task shows its title, state,
  the end reason and spend. Both are in nodes (`session.history`), which a later step can read for the few failed calls
  and ended tasks of a window; I did not, to keep the report one read of one method.

## What I changed

- `3841867` protocol + core. `theseus-protocol/src/window.rs`: `LedgerWindowParams {since_ms, until_ms?, session_id?,
  after?, limit?}`, `LedgerWindowResult {rows, next?, scope, building, shared, now_ms}`, `POSTMORTEM_KINDS` (42
  kinds). `theseus-core/src/rpc/window.rs`: one page by the kinds' postings (a session's tag when narrowed to it and
  the task sessions below it, found from `Kernel::tasks`), every string in a row through the scrubber, a surface that
  `!reads_private()` must name the session and sees that session's rows and the daemon's; no frame, no session or
  execution lock. A task id (`tsk_x`) names its session (`ses_x`). The dispatch arm shares one line with `ledger.tail`
  (`ledger_reads`). TypeScript regenerated (`cockpit/src/protocol.gen/`, 2 files and the index).
- `d38121d` CLI. `crates/theseus/src/postmortem.rs` (the command, the flags, paging, `--out`, `--rows-out`,
  `--from-rows`), `render/postmortem_facts.rs` (`analyze`: a pure walk over the rows) and `render/postmortem.rs` (the
  `Report`, which `--json` prints, and its markdown); `render/time.rs` gained `local_day_at`, `fmt_hms`, `fmt_day`.
  `--since/--until` take `2h`, `30m`, `1d`, `22:00`, `yesterday 22:00`, `today 06:30` or an RFC 3339 time. The sections
  are in the specified order; the summary is five lines at most (a test holds it); quiet turns fold in runs of 3 or
  more ("… 14 turns, $0.31, nothing notable …"); a long call is over 30 s; `--session`/`--task` group the timeline by
  session. `main.rs` changed by a few lines (the arm, an early return that connects for itself, and `wait_any` moved to
  a function to stay within clippy's 100 lines of `run`).
- `b3e60f5` `crates/theseusd/tests/postmortem_incident.rs`: the incident, scripted on a real daemon (below).
- `71f411a` `execution.opened` joins the kinds (a task's opening), the fixture is recorded again, goldens rewritten.
- AGENTS.md of `theseus`, `theseus-protocol`, `theseus-core`, `theseusd` say what is new. No store format change, no
  config key, no dependency (Cargo.lock untouched), no person's name.

## How I proved it

- **Unit and golden tests** (`TZ=America/Phoenix cargo nextest run -p theseus -p theseus-protocol -p theseus-core -E
  'test(/postmortem/) | binary(postmortem) | test(tests_window) | test(ts::)'`: all pass):
  - `render/postmortem_facts_tests.rs` (9): a restart with a resumed turn (killed: down from the last row, the interrupted
    turn, the spool, a call answered unknown, a late wake, the resumed turn's trigger) and a clean stop; an expired
    question, an answered one and one still open; a fold of quiet turns (and a run of two stays unfolded); spend by
    model and by session summing to the settle rows, with a reservation held; long, failed and cut calls (a 29.999 s
    call is no line); what is open at the end; a narrowed report grouped by session, and an empty one.
  - `tests/postmortem.rs` (7): a scripted daemon (the window read a page at a time from the last position, the default
    12 hours, `--session`/`--task`, `--out`, `--json`, the daemon's refusal said as it is, the index still being built,
    `--rows-out` then `--from-rows`), and the recorded incident: markdown golden, `--json` golden, and "every fact the
    script caused appears as often as it happened, in order".
  - `postmortem_tests.rs` (4): the times. `rpc/tests_window.rs` (7): kinds and times, pages walk each row once, a
    secret is scrubbed, a session narrows, a shared surface must name its session and sees only it, no frame written,
    and a busy day.
  - The read writes no frame: `the_read_writes_no_frame` checks the store's last position and the ledger's length
    around reads from both a private and a shared surface.
- **Planted reverts** (each file put back with `touch`; `git status` clean after): (1) the Restarts section dropped
  from `markdown`: `a_restart_names_what_it_resumed`, `a_narrowed_report_groups_by_session_and_an_empty_one_says_so`,
  `every_fact_the_script_caused_appears_once_in_order`, `the_incidents_markdown_is_its_golden` fail. (2) `FOLD_MIN =
  usize::MAX` (the fold off): `quiet_turns_fold_into_one_line` and both goldens fail. (3) the restart's resumed list
  left empty: `a_restart_names_what_it_resumed` and both goldens fail.
- **Timing** (debug build, `rpc/tests_window.rs::a_busy_days_window_pages_in_under_two_seconds`): 12,000 rows of 4
  kinds in the window, half of them rows the report reads: 6,000 report rows in 13 pages in **0.14 s** (about 23 µs a
  row; the test asserts under 2 s). That is the read at the daemon, not the CLI's analysis. A day with 100,000 report
  rows is about 2.3 s in a debug build; a release build is several times faster, but I did not measure a release
  build, and I had no real 24-hour store here, so "24 hours on a busy store under 2 s" is **extrapolated, not
  measured**: the maintainer should time it on the owner's daemon (below).
- **The keel**: `python3 scripts/keel-guard.py` against this clone's range base reports 5 erosions that are **not
  mine** (`crates/theseus-tui/src/tests.rs`, `crates/theseus-index/src/vectors.rs`, `crates/theseus-protocol/src/
  tests_work_join.rs`, two ceilings in `scripts/long-files.txt`): the guard's base here is `a9ad950`, older than the
  main I cloned (fdd1492), so main's own changes read as findings. With `THESEUS_KEEL_BASE=HEAD` (my changes alone):
  `keel: ok (… 0 findings)`. **Keel findings expected: none.** I added no `#[allow]` or `#[expect]` (two tests carry
  `#[rustfmt::skip]`, which is no lint), raised no ceiling (the protocol's lib.rs is 2,720 of 2,733), removed no test.

## The live check on this VM

The fixture's script, on a scratch daemon with the stand-in model (about 40 s; it leaves the daemon, its state and its
socket in a temp dir the test removes):

```
THESEUS_RECORD_POSTMORTEM=/tmp/pm cargo nextest run -p theseusd -E 'test(an_incident_is_recorded)'
```

That test asks the real `theseus postmortem --since 30m` of the real daemon and asserts the report names every fact;
its output is `/tmp/pm/incident.md` (paths here are the temp dir's; the fixture's are `/srv/theseus`), and the rows
`/tmp/pm/incident.jsonl`. This is that report, as the CLI printed it (UTC):

````markdown
# Postmortem: 2026-10-11 19:25:34 to 2026-10-11 19:55:34

## Summary

- Window 2026-10-11 19:25:34 to 2026-10-11 19:55:34: 10 sessions, 1 task.
- 17 turns run, 0 failed; spent $0.0045 (top: ses_01a12c88664870f5b2511fa9c7109687 $0.0007, ses_01a12c886679726ba0f63d06c431537e $0.0007, ses_01a12c8866df765ea662896cd82adbe1 $0.0007).
- 1 daemon restart; questions to the owner: 2 asked, 1 answered, 1 expired.
- First failure: 19:54:54 · ses_01a12c8855b0729ea92e6e929fa16164 · #141 — tool call `fs.read` failed (inproc:fs.read) after 1 ms.

## Restarts

- The daemon was killed or lost power, started again at 19:55:13 · #472 (down 14.1 s).
  - Turn requeued as interrupted: ses_01a12c88664870f5b2511fa9c7109687 (turn_01a12c88664e76a6a498e439fdefedff).
  - Turn requeued as interrupted: ses_01a12c886679726ba0f63d06c431537e (turn_01a12c8866877351b81b38c3d2dc5194).
  - Wake wak_01a12c8867067431b8140c7a98f15494 of ses_01a12c8866df765ea662896cd82adbe1 ran 4.1 s late at 19:55:13 · #488.

## Timeline

**2026-10-11**

- 19:54:54 · #102 — … 3 turns, $0.0007, nothing notable …
- 19:54:54 · ses_01a12c8855b0729ea92e6e929fa16164 · #113 — turn turn_01a12c8855b5729b853669059f05b4c7 started (owner)
- 19:54:54 · ses_01a12c8855b0729ea92e6e929fa16164 · #141 — tool call `fs.read` failed (inproc:fs.read) after 1 ms
- 19:54:54 · ses_01a12c8855b0729ea92e6e929fa16164 · #159 — turn turn_01a12c8855b5729b853669059f05b4c7 ended in 97 ms, $0.0004; 2 loops, 1 tool calls, stopped: no_tool_calls
- 19:54:54 · ses_01a12c88561e74aa87e8336dfc1810dd · #170 — turn turn_01a12c88562372a5b7368410657f8295 started (owner)
- 19:54:54 · ses_01a12c88561e74aa87e8336dfc1810dd · #192 — asked to approve `fs.write`: create /tmp/.tmpzekJf8/projects/notes.txt (6 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)
- 19:54:54 · ses_01a12c88561e74aa87e8336dfc1810dd · #195 — turn turn_01a12c88562372a5b7368410657f8295 ended in 48 ms, $0.0002; 1 loops, 1 tool calls, stopped: awaiting_confirm
- 19:54:54 · ses_01a12c88565a77b78d50a5217d81c28e · #206 — turn turn_01a12c88565e7469882251a6cfb0c8d6 started (owner)
- 19:54:54 · ses_01a12c88565a77b78d50a5217d81c28e · #228 — asked to approve `fs.write`: create /tmp/.tmpzekJf8/projects/notes.txt (7 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)
- 19:54:54 · ses_01a12c88565a77b78d50a5217d81c28e · #231 — turn turn_01a12c88565e7469882251a6cfb0c8d6 ended in 46 ms, $0.0002; 1 loops, 1 tool calls, stopped: awaiting_confirm
- 19:54:59 · ses_01a12c88565a77b78d50a5217d81c28e · #237 — `fs.write` approved by the CLI from cli after 4.0 s
- 19:54:59 · ses_01a12c88664870f5b2511fa7f44730ca · #266 — turn turn_01a12c88664d76b5be481e392f8dcf42 started (owner)
- 19:54:59 · ses_01a12c88664870f5b2511fa9c7109687 · #269 — turn turn_01a12c88664e76a6a498e439fdefedff started (owner)
- 19:54:59 · ses_01a12c88565a77b78d50a5217d81c28e · #345 — turn turn_01a12c886642706aafb4aa8c9481eadc (harness) ended in 60 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:54:59 · ses_01a12c88664870f5b2511fa7f44730ca · #339 — task tsk_01a12c886679726ba0f63d06c431537e created: Run the long job and report.
- 19:54:59 · ses_01a12c886679726ba0f63d06c431537e · #340 — task opened, limit $25.00
- 19:54:59 · ses_01a12c886679726ba0f63d06c431537e · #357 — turn turn_01a12c8866877351b81b38c3d2dc5194 started (task start)
- 19:54:59 · ses_01a12c88664870f5b2511fa7f44730ca · #398 — turn turn_01a12c88664d76b5be481e392f8dcf42 ended in 103 ms, $0.0004; 2 loops, 1 tool calls, stopped: no_tool_calls
- 19:54:59 · ses_01a12c8866df765ea662896cd82adbe1 · #458 — turn turn_01a12c8866e37399aa992f2ae2783e4e (owner) ended in 79 ms, $0.0004; 2 loops, 1 tool calls, stopped: no_tool_calls
- 19:55:13 · ses_01a12c88664870f5b2511fa9c7109687 · #463 — turn turn_01a12c88664e76a6a498e439fdefedff interrupted by a restart of the daemon
- 19:55:13 · ses_01a12c886679726ba0f63d06c431537e · #465 — turn turn_01a12c8866877351b81b38c3d2dc5194 interrupted by a restart of the daemon
- 19:55:13 · #473 — daemon started again after it was killed
- 19:55:13 · ses_01a12c8866df765ea662896cd82adbe1 · #485 — turn turn_01a12c889e237094b137914a607e9d45 started (wake)
- 19:55:13 · ses_01a12c88664870f5b2511fa9c7109687 · #489 — turn turn_01a12c889e237094b13791499b9fc8e8 started (resumed after a restart)
- 19:55:13 · ses_01a12c886679726ba0f63d06c431537e · #491 — turn turn_01a12c889e25700da680e87c61e1b316 started (resumed after a restart)
- 19:55:13 · ses_01a12c8866df765ea662896cd82adbe1 · #488 — wake wak_01a12c8867067431b8140c7a98f15494 fired 4.1 s late (it came due while the daemon was down)
- 19:55:13 · ses_01a12c886679726ba0f63d06c431537e · #529 — turn turn_01a12c889e25700da680e87c61e1b316 ended in 49 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:13 · ses_01a12c8866df765ea662896cd82adbe1 · #541 — turn turn_01a12c889e237094b137914a607e9d45 ended in 58 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:13 · ses_01a12c88664870f5b2511fa9c7109687 · #547 — turn turn_01a12c889e237094b13791499b9fc8e8 ended in 59 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:20 · ses_01a12c88561e74aa87e8336dfc1810dd · #553 — question about `fs.write` expired unanswered after 25.0 s
- 19:55:20 · ses_01a12c88561e74aa87e8336dfc1810dd · #575 — turn turn_01a12c88ba6773e592ae554d9ce0a3b6 (harness) ended in 47 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:21 · ses_01a12c88664870f5b2511fa9c7109687 · #606 — turn turn_01a12c88bc887419bd4ddb79d559412d (job result) ended in 39 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:32 · ses_01a12c886679726ba0f63d06c431537e · #613 — tool call `proc.run` ran 33.0 s
- 19:55:32 · ses_01a12c886679726ba0f63d06c431537e · #638 — turn turn_01a12c88e7b6741884e1db9ea93573c3 (job result) ended in 44 ms, $0.0002; 1 loops, 0 tool calls, stopped: no_tool_calls
- 19:55:32 · ses_01a12c88664870f5b2511fa7f44730ca · #643 — task exe_01a12c886679726ba0f63d06c431537e ended complete (reported), spent $0.0007 of $25.00
- 19:55:32 · ses_01a12c886679726ba0f63d06c431537e · #645 — task tsk_01a12c886679726ba0f63d06c431537e closed done

## Cost

Total $0.0045.

By session:

- ses_01a12c88664870f5b2511fa9c7109687 — $0.0007 (3 calls)
- ses_01a12c886679726ba0f63d06c431537e — $0.0007 (3 calls)
- ses_01a12c8866df765ea662896cd82adbe1 — $0.0007 (3 calls)
- ses_01a12c8855b0729ea92e6e929fa16164 — $0.0004 (2 calls)
- ses_01a12c88561e74aa87e8336dfc1810dd — $0.0004 (2 calls)
- ses_01a12c88565a77b78d50a5217d81c28e — $0.0004 (2 calls)
- ses_01a12c88664870f5b2511fa7f44730ca — $0.0004 (2 calls)
- ses_01a12c88550275d8968b8ba7f7b95695 — $0.0002 (1 call)
- ses_01a12c8855347063817590df7f915185 — $0.0002 (1 call)
- ses_01a12c88557b7654b1210a7b398d6b62 — $0.0002 (1 call)

By model:

- claude-sonnet-5-5 — $0.0045 (20 calls)

No reservation is held at the window's end.

## Asked and answered

- 19:54:54 · ses_01a12c88561e74aa87e8336dfc1810dd · #192 — `fs.write` (create /tmp/.tmpzekJf8/projects/notes.txt (6 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)): expired unanswered after 25.0 s
- 19:54:54 · ses_01a12c88565a77b78d50a5217d81c28e · #228 — `fs.write` (create /tmp/.tmpzekJf8/projects/notes.txt (7 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)): approved by the CLI from cli after 4.0 s

## Open at the end

Nothing is open.
````

## The live check for the maintainer, on the owner's daemon

```
theseus postmortem --since 12h | less                       # the default window; each section in order
theseus postmortem --since "yesterday 22:00" --until 06:00   # the night, in the machine's zone
theseus postmortem --since 24h --out /tmp/pm.md; time theseus postmortem --since 24h --json >/dev/null
theseus postmortem --session <the end of a session id> --since 6h   # grouped by session, with its tasks
theseus ledger --n 1 >/dev/null                              # nothing written: health's `ledger` count is unchanged
```

Expect: the sections Summary, Restarts, Timeline, Cost, Asked and answered, Open at the end; every line a local time,
a `ses_…` and a `#position`; a `kill -9` of the daemon shows as "killed or lost power" with the turns it requeued; the
`time` of the 24-hour read is the number the brief wants under 2 s.

**To add a second fixture from a real incident on the owner's daemon:**

```
theseus postmortem --since 8h --rows-out incident2.jsonl > /dev/null
```

`--rows-out` keeps the rows the report was made from, and the daemon has already scrubbed every string in them of the
secrets it knows. The rest is the maintainer's: `grep -n` the file for names (`author`, `by`, `via`, handles, paths
under a home directory, a titles' words) and `sed` each to an invented one (`/home/<name>` to `/srv/theseus`,
`discord:<id>` to `discord:1000`), read the file, then `git add` it under
`crates/theseus/tests/fixtures/postmortem/incident2.jsonl` and add a golden pair the way `tests/postmortem.rs` does
(`THESEUS_GOLDEN=write` writes `.md` and `.json`; the render is `theseus postmortem --from-rows incident2.jsonl`, with
`TZ='<-07>7'`). This incident's own rows were recorded by the test, so they hold only invented paths and the author
`owner`.

## What is left, and design choices the owner should hear about

- **A read of a window by a shared place.** `ledger.window` from a surface that does not read private text must name a
  session and gets that session's rows and the daemon's own rows (starts and stops, which carry no session's text). I
  did not add a place check beyond that (that the named session is the caller's place's own): the binding does not call
  this method today. If Discord is to offer `/postmortem`, the binding should name the place's session, and the daemon
  should refuse a session of another place (`Core::places` can say which). That is a design decision for the owner.
- **Held reservations are read from the window's own rows.** A call planned before the window and still open at its end
  is not seen until it settles, and a window that ends in the past shows what was open by the rows it read; the report
  says so only here. "Open at the end" of a default window (end = now) is exact for what began inside it.
- **No `session.history` for first lines** (see above). It would add a read per failed call and ended task.
- **`--task` and `--session` conflict** (clap), and an unresolvable short id fails as the other commands do.
- **The 33 L1 tests fail on this VM, as the brief says.** No other test failed.
- The recorder test runs only with `THESEUS_RECORD_POSTMORTEM` set (it takes 40 s and a 33 s command is the point: the
  report's long-call line is 30 s). Without it the test says so and passes, as `lsp.rs` does with no server. The
  scripted facts are asserted by `tests/postmortem.rs` from the frozen rows on every run.
- Docs to change at review: the spec's Part III (an item for this step: `ledger.window`, the postmortem), the
  protocol chapter's method table (`ledger.window`), `docs/status.md`, and the killer-features plan §4.9 row R23.

## The gate

`THESEUS_KEEL_BASE=HEAD TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the tree of `71f411a` (the report
aside): door, keel (0 findings against HEAD), keel tests, fmt, shape, features, clippy (`-D warnings`, whole workspace),
cockpit, test build, reader rule (15 tests) and the suite ran. The suite: **3,773 tests, 3,740 passed, 33 failed**, all
33 the known L1 failures of this VM (theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox
tests; the brief's root-daemon refusal, theseus-pv6i). Not run by the gate's `--locked-part` because of that failure,
run by me: the protocol-types check (`git diff -- cockpit/src/protocol.gen` clean once added) and the deny phase
(`cargo deny --offline check bans licenses sources`: ok; the advisory database was not fetched here, so advisories were
**not** checked). The benches were not run (`THESEUS_GATE_NO_BENCH=1`; no startup work was added).

I ran the gate with `THESEUS_KEEL_BASE=HEAD` because the clone's own range base makes main's changes read as keel
findings (above); the plain gate stops in its keel phase on those five, none of them in my diff.

This VM's disk allowance (about 39 GB) filled twice during the builds (a cold `--all-targets` debug build and
the gate's second build of every test binary); I deleted `target/debug/incremental` and stale test binaries, nothing
outside the repository.
