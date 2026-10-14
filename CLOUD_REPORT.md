# CLOUD_REPORT: job-follow (theseus-e1hu4)

Branch `cloud/20261013-job-follow`, from main as cloned (`f8aec2d2`, store format 27, `work-board` and
`status-short-board` joined). Code commits: `0e685b90` (the feature) and `9fc13d1b` (a client fix and two
sturdier tests, found under load). This report is the last commit. No store change, no new config key, no new
dependency (`Cargo.lock` and the package locks are untouched).

## What I found

- `core/toolrun/peek.rs` already existed (theseus-n8gk's `job.read` reader, bounded at 64 KiB, but it also counts
  lines up to 8 MiB). `work.peek` must read no more than 64 KB, so I added two bounded readers beside it
  (`newest`: a stat and the last 64 KiB; `last_line`: the last 4 KiB) and left `peek` alone.
- The work board has no timer and writes no frame for what a job prints, so `last_line` cannot ride a frame. A view
  is applied by `position` alone (strictly greater), so a sample, which has its frame's position, would be dropped by
  every client. I made a sample keep its position and take the sample's time (`at_ms`), and `theseus work --watch`
  applies by (position, time). The TUI does not read `work.watch` (only `execution.changed`), so no TUI rule moved;
  the cockpit does not read it either.
- `eta::MIN_RUNS` is 2 ("one run is no usual"), so a job shows `of usual` from its **third** run of a program, not
  its second. The task's live check says the second run; I left the board's rule alone (see Live check).
- Past the output file's head (`[tools] job_output_max_bytes`, 64 MiB less a 4 MiB tail ring) the wrapper keeps the
  newest output in memory until the job ends (`redact.rs`), so the file stops growing: `last_line`, a peek and a
  follow all stop at the head until the job ends. This is the wrapper's design (theseus-gsn9); I did not change it.
- `theseus-sim bench turn` budgets on this main are plain 4, tool 8, batch 14 frames (the task text says 5 and 9);
  all are held.
- A shell alone on this VM filled the disk allowance: `target/debug/incremental` was 21 GB. I deleted it (it is
  rebuildable, inside the repository) and built with `CARGO_INCREMENTAL=0` after.

## What I changed (`0e685b90`, `9fc13d1b`)

Protocol (`theseus-protocol`): new `job_follow.rs`: `work.peek` (`WorkPeekParams`/`WorkPeekResult {id, bytes, lines
(<= 20), at_ms, complete}` plus optional `started_at_ms`, `usual_ms`), `action.output` (`ActionOutputParams {id,
from?}` / `ActionOutputResult {id, offset, bytes, complete}`), the notifications `action.output.chunk {id, offset,
text}` and `action.output.end {id, outcome}`, `action.unfollow`, and `job_words(elapsed, usual)`, the one wording
`job 0:05 of usual 0:31`. `Doing.last_line` (optional, <= 120 characters). Methods, notifications, `Event`s, the
generated TypeScript (8 new files, `Doing.ts`), fixtures `action_output_{chunk,end}.json` and
`work/view_job_running.json`. Types are in the new module; `lib.rs` gained 12 lines.

Core (`theseus-core`):
- `rpc/peek.rs`: the three methods. `work.peek` runs on the blocking pool (stat and <= 64 KiB, scrubbed with the
  session's scrubber, lines cut to 1,000 characters). `action.output` starts a follower task for the connection
  (`SessionBus::follow`; ended by `action.unfollow`, by its end, by the connection closing): it holds the file open
  (so a job that settles and is swept still reads), looks every 100 ms, sends whole lines (a line longer than 32 KiB
  as it is) as chunks with their byte offset, never lags more than 4 MiB (skips ahead; the offset shows it), and
  after the job settles flushes the rest and sends `action.output.end`. Backpressure: a connection whose queue holds
  64 messages gets no more; the follower counts the stream lost (`Outbound::lose`, new, beside `notify`'s own
  drop-at-cap), so the writer sends `events.lost` naming `output:<id>` as the queue drains, and the follower skips to
  the end. A connection that reads no private text is told `no job <id>` (NOT_FOUND, the answer of an id that does
  not exist) for a private session's job.
- `push/last_line.rs`: the sampler (`Core::sample_last_lines_after_serving`, started in theseusd after serving). It
  waits on a `Notify` while no job runs (the push's applier and the seed wake it), and once a second while one does
  reads the last 4 KiB of each running job's file on the blocking pool, scrubs the line, cuts it to 120 characters,
  and tells the work watchers only when it changed. Jobs of a shared place's session are never sampled; a guarded
  `work.watch` scope (`place.rs`) drops the field. The board keeps `running` and `last` maps (`install`/`remove`).
- `toolrun/peek.rs`: `newest`, `last_line` (ANSI escapes and control characters dropped; a `\r` progress bar shows
  its last rewrite). `bus.rs`/`outbound.rs`: the follower table, `backlog()`, `lose()`.

CLI (`theseus`): `theseus peek JOB`, `theseus attach JOB` (`jobout.rs`; a job is named whole or by a unique end of
at least four characters; Ctrl-C asks `action.unfollow` and exits 0, never stopping the job; `events.lost` says so,
peeks again and follows from there; a closed connection exits 1; a re-peek drops the chunks the old follower had in
flight). `theseus work` words a running job `[job 0:05 of usual 0:31]  › <last line>`; `theseus status` (long form)
adds `· job 0:05 of usual 0:31` to a row whose session runs a job (one more read, `work.watch`, best effort).
`run()` in `main.rs` was at clippy's 100 lines: one arm now serves `work`, `peek` and `attach` (`jobout::board`), and
`theseus wait`'s two arms are one helper.

TUI (`theseus-tui`): `follow.rs`: `l` on the session under the cursor finds its running job (`action.list
{execution_id, unsettled}`), peeks, follows from the peek's end; the pane replaces the detail area (header `job 0:05
of usual 0:31 · <name> · esc closes`), `esc` closes and unfollows; `events.lost` and a reconnect peek again. The
sidebar's row words are `tui-rows`'; I touched only the pane, the key, the help line and four match arms in `app.rs`.

Docs: `AGENTS.md` of core, protocol, the CLI and the TUI.

## How I proved it

- Protocol: `cargo nextest run -p theseus-protocol`: 93 passed, 1 skipped (fixtures written with
  `THESEUS_GOLDEN=write`, then read back both ways; `wire.rs`'s fixture count 42 -> 44).
- Core, `rpc/tests_peek.rs` (8 tests, real in-process jobs; `toolrun::peek` has one more): a peek of a running job
  gives its size (= the file's), <= 20 lines and withholds a planted value; a follow from 0 sends `leg 1..12` in
  order at contiguous offsets, then the end `succeeded`, and leaves no follower; unfollow and a closed connection end
  a follower; a slow client hears `events.lost` naming `output:<id>` (both by the connection's cap and by the
  follower's own 64-message limit) and recovers with a peek; a follow from far behind starts at the newest 4 MiB;
  `last_line` under a chatty job (a shell loop printing ~100,000 lines a second for 4 s) arrives 2 to 6 times and
  never twice within 900 ms, scrubbed; Discord, Unnamed and MCP connections are refused a private job (peek and
  follow), a job given to the sampler under a shared session is never sampled, and a guarded scope drops the field.
- CLI: 358 passed (`jobout.rs`: peek words, naming a job, attach order and end, **attach detaches asking
  `work.peek, action.output, action.unfollow` and nothing that stops the job**, a stale chunk is not printed twice;
  `status_tests.rs`: a row with a job; `work.rs`'s tree). TUI: 77 passed (`tests_follow.rs`: `l` -> `action.list`
  -> `work.peek` -> `action.output from 12`, header `job 0:05 of usual 0:31`, chunks in order, the end, `esc` ->
  `action.unfollow`; `events.lost` peeks again; no job; a refusal).
- **Planted reverts** (each file restored with `touch`, `git status` clean after):
  1. scrubber skipped on peek -> `a_peek_of_a_running_job_returns_its_bytes_and_last_lines_scrubbed` fails
     (`tests_peek.rs:246`, the planted value in the lines);
  2. `last_line` unthrottled (the sampler's tick 1 s -> 1 ms) -> `last_line_goes_out_at_most_once_a_second_under_a_chatty_job`
     fails (samples closer than 900 ms, or more than 6);
  3. the follower's 64-message limit removed -> `a_follower_does_not_fill_a_stuck_clients_queue` fails (no
     `events.lost` in 30 s);
  4. the place check removed from `job_for` -> `a_shared_place_reads_no_private_job_and_gets_no_last_line` fails
     (a Discord connection reads the job);
  5. the guarded scope's `last_line = None` removed -> the same test fails at its last assertion;
  6. `attach` detaching without `action.unfollow` -> `attach_detaches_without_cancelling_the_job` fails
     (`left: ["work.peek", "action.output"]`);
  7. the TUI's `esc` not unfollowing -> `l_follows_a_sessions_running_job_and_esc_lets_it_go` times out waiting for
     `action.unfollow`.
- **Under load** (four `sh -c 'while :; do :; done'` at nice 0, the tests at nice 19; loops killed by pid): the first
  loaded run failed two tests, which was two real findings. (a) The slow-client tests slept 2.5 s and assumed the
  niced job had printed enough: they now wait until the daemon counts a loss (`push.status(0).lost`) and the capped
  variant uses a cap of 3. (b) A chunk the old follower queued before the new follow's answer began before the
  peek's end; the daemon is right (the new follow ends the old follower, so every stale chunk precedes its answer),
  but the CLI printed it twice and the TUI showed it twice: both now drop a chunk that ends at or before what they
  have shown and cut an overlap (`9fc13d1b`, with a test each). After the fix, `tests_peek` passed 1 unloaded and 3
  loaded runs (8/8 each; the loaded runs took 35 to 43 s), and the CLI and TUI follow tests passed loaded.
- **Gate.** `scripts/gate.sh` stops in `door`/`keel` on a finding that is not mine (see Keel). I ran its phases by
  hand on the final tree: `cargo fmt --all -- --check` ok; `scripts/shape.sh` ok; `cargo clippy --workspace
  --all-targets -q -- -D warnings` clean; `cockpit` lint, test and build ok; `tests_registry` 15/15; the whole suite
  `cargo nextest run --workspace --no-fail-fast`: **4542 tests, 4509 passed, 33 failed, 47 skipped**: the 33 are the
  known L1 root-daemon set (`theseus-sandbox::contract` x19, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox`
  x13), nothing else; the generated TypeScript is committed (`protocol_types` ok); `theseus-sim bench turn --check
  --runs 5 --burst 0`: plain 4, tool 8, batch 14 frames, syncs 2, all `ok` (the sampler writes no frame, and the
  bench's jobs finish before a second passes); `cargo deny --offline check bans licenses sources` ok. The full
  `cargo deny check` **fails advisories** on rustls RUSTSEC-2026-0346 to 0349 (upgrade to >= 0.23.46) from the advisory
  database fetched at setup: I added no dependency, so this is main's (the maintainer's database may be older).
  Lifecycle, jobs and size phases skipped (`THESEUS_GATE_NO_BENCH`, no release build). The suite ran after the last
  non-test change (the final commit changed tests, the client trims, and docs; I reran the CLI, TUI, protocol and
  `tests_peek` suites and clippy after it, not the whole workspace).

## Live check (a scratch daemon on `theseus-sim fake-model`, state under `/tmp`, stopped by `theseus shutdown`)

Setup: `/tmp/claude-0/live/live.sh` (config: `proc.run = notify`, `proc_sync_secs = 2`, stand-in key `env:`), rule
`run the legs` -> `proc_run sh -c 'for i in $(seq 30); do echo leg $i; sleep 1; done'` with `background: true`. The
exact commands the maintainer can rerun are in "Commands" below. Captures (times as printed):

First run:

```
$ theseus work
◐ run the legs  sh -c for i in ...  (0 need you, 1 working, 0 done, 0 failed)
  ◐ sh -c for i in $(seq 30); do echo leg $i; sleep 1; done  ...  [job 0:04]  › leg 4
$ theseus peek act_01a13c13…
act_01a13c134a3470a1a4dc11bd1afeaa7f · job 0:04 · last 5 lines of 30 B
  leg 1 … leg 5
$ theseus attach act_01a13c13…        # SIGINT after 5 s
act_… · job 0:15 · last 16 lines of 103 B
  leg 1 … leg 16
-- following (Ctrl-C detaches; the job goes on) --
leg 17 … leg 21

-- detached; the job goes on --                    (exit 0)
$ theseus work                                    # 1 s later: [job 0:20]  › leg 20   (the job went on)
```

Third run of the same program (the board offers `usual` after two finished runs, `MIN_RUNS = 2`; the second run
showed `[job 0:06]  › leg 6` with no usual, as designed):

```
$ theseus work     ->  ◐ sh -c … [job 0:05 of usual 0:30]  › leg 5
$ theseus status   ->  ◐ 9c8c14  run the legs  waiting on 1 call · 0:05 · job 0:05 of usual 0:30
$ theseus peek …   ->  act_… · job 0:05 of usual 0:30 · last 6 lines of 36 B
$ theseus --json peek … ->  {"result":{"at_ms":…,"bytes":36,"complete":false,"id":"act_…","lines":[…6…],"started_at_ms":…,"usual_ms":30086}}
```

`theseus attach` of the second run, to the end: `leg 24 … leg 30` then `-- the job ended: succeeded --`.

The TUI in tmux (120x24), `j` to the working row, `l`:

```
 theseus · 3 done                                              20:23 · socket ok
 ◆ done  run the legs         $0.08 │ job 0:23 of usual 0:30 · run the legs · esc closes
 ◆ done  run the legs         $0.08 │ leg 4
 ◐ waiting on 1 call  run the $0.05 │ leg 6   … (the lines to leg 24 at the foot)
…5 s later:                          │ job 0:28 of usual 0:30 · run the legs · esc closes   (leg 9 … leg 20+)
after esc:                           │ no session open: enter opens the one under the cursor
```

## Commands the maintainer should run (a live check that needs nothing of the owner's)

```
cargo build -p theseus -p theseusd -p theseus-tui -p theseus-sim
/tmp/…/live.sh  # or: theseus-sim fake-model --addr 127.0.0.1:PORT --rules rules.json; theseusd --config C --socket S --state-dir D
cd <project>; theseus --socket S ask "run the legs"; theseus --socket S work
theseus --socket S attach <act_… from work>          # shows the legs live; Ctrl-C detaches, the job goes on
theseus --socket S ask "run the legs"  (twice more)  # the third shows `job 0:05 of usual 0:30` in work, status, peek
theseus-tui --socket S   # j to the working row, l: the follow pane with the same words; esc closes
theseus-sim bench turn --check --runs 5 --burst 0    # frames: plain 4, tool 8, batch 14 (unchanged)
```

## What is left or uncertain; choices the owner should hear about

- **`usual` needs two finished runs**, so "the second run shows `of usual 0:30`" is the third in practice; that is the
  board's `MIN_RUNS` (a design choice of work-board), which I did not touch. Lowering it to 1 is one constant.
- **`last_line` stops at the output file's head** (see Found) and is on the job's own view only: a task's or
  conversation's `doing` (whose label is the job's title) does not carry it; a surface that wants it finds the job
  among the task's children (the TUI's `l` does, through `action.list`).
- **A sample's time on the view**: `at_ms` of a sampled job view is the sample's time, not a frame's. A client must
  compare (position, time); `theseus work --watch` does. The cockpit and the TUI do not read the work board yet:
  whoever adds it must use this rule (the protocol AGENTS.md and `Doing.last_line`'s doc say so; the `tui-rows` row
  should know).
- **Scrubbing is per chunk** (whole lines, up to 32 KiB): a multi-line secret block (a PEM) cut by a chunk edge is not
  recognised as one. The same holds at a peek's 64 KiB edge. A value the scrubber knows is withheld wherever it lies
  inside a line.
- **`peek`/`attach` name a job by `act_…` or a unique end**; no handle (`j1`) is accepted, since a handle is a session's.
- **`theseus status`'s long form makes one more read** (`work.watch {limit: 0}`, best effort, 2 s) to word a job's
  time; `--short`, `--watch` and `--json` are unchanged.
- **Constants** (not config keys): follower tick 100 ms, chunk 32 KiB, 16 chunks a look, queue room 64, lag bound
  4 MiB, sampler tick 1 s, peek 64 KiB / 20 lines / 1,000 characters a line, `last_line` 120 characters.
- Docs for the maintainer to write: the spec's Part III item for theseus-e1hu4 and its version line,
  `docs/status.md` (the roadmap row), `docs/design/` (the substrate design's job section, `last_line`, the (position,
  time) rule). I did not touch them.

## Keel findings expected

None from this branch: I raised no cap or ceiling and removed or loosened no test or assertion; one count went up
(`tests/wire.rs`: 42 -> 44 fixtures, two new). `python3 scripts/keel-guard.py` prints, before and after my commits,
the same single finding, which is in the **base** range, not mine:

```
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — the owner, 2026-10-13 23:24: …` on 1c2beb2747: its signature does not verify (%G? is E)
keel: 1 erosions … (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0):
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
```

This clone's `origin/main` is older than the base I was given (`9fa84735` vs `f8aec2d2`), so the range holds a main
commit whose signed ack cannot be verified without the owner's key here. With the right `origin/main` it should
vanish. Because of it the gate's `door`/`keel` phases fail on this VM whatever the diff; the other phases were run by
hand as above.
