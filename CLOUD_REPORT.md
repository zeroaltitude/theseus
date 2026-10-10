# CLOUD REPORT: h2h-bench (theseus-7gir.13, theseus-4w1h)

Branch `cloud/20261010-h2h-bench`, from `main` at 2fd1f654 (store format 26, unchanged: no stored record changes).
Started 20:15 UTC on 2026-10-10. 4-core VM, 15 GB, root, no sccache.

Commits: `ef0b197` (the stand-in, the counted rows, the overhead's split, the speed wall's words), `912e66d` (the
stand-in's steps for Claude Code's merged turns; the counted rows' warning), `269df74` (`bench/h2h/`), then this report.

## The steps

### 1. The stand-in in Rust (`theseus-sim fake-model`), paced and logged

**Found.** The stand-in sent every SSE event in one write and closed each connection (`connection: close`), so
no first-token row could be measured. It matched a rule on the last user text only, which no harness's multi-step
turn fits.

**Changed** (`ef0b197`, and the fixes in `912e66d`):
- `crates/theseus-sim/src/fake_model/serve.rs`: HTTP/1.1 keep-alive, chunked, `TCP_NODELAY`, each connection on its
  own thread. Pacing (`--ttfb-ms`, `--chunks`, `--chunk-ms`, or a rule's own `ttfb_ms`/`chunks`/`chunk_ms`): the
  first byte that long after the request arrived, then the answer's deltas one write each, that far apart (a text's
  first chunk always holds its first word, so a marker there is on screen with the first text). Unpaced, an answer
  is one write, as before. `"stream": false` gets the message whole; `count_tokens` is answered. `--log`: one JSON
  line a request, times on `CLOCK_MONOTONIC` (`arrival_ns`, `first_byte_ns`, `last_byte_ns`), `conn`/`conn_req`
  (keep-alive reuse), sizes, tools, model, `side` (no tools), `stream`, `rule`, `step`, `opening`, `marker`, `chunks`.
- `fake_model/steps.rs`: a rule's `steps` script a whole turn, matched on the turn's opening user text; `{marker}`
  in a step's strings is the word after `marker=`. **Choice: one Rust stand-in serves both harnesses; no Python
  stand-in.**
- The live check against Claude Code 2.1.296 found its requests merged by role: a new prompt lands in the user
  message that holds earlier prompts and results, a turn's own results can sit *before* its prompt, and a `system`
  role message appears among the messages. Reading the messages, the stand-in looped (200 identical requests for
  step 0). Fixed in commit 2: the opening is the last plain text block (reminders never count), and with a marker
  the step is the requests the rule has answered for that marker (structure stays the fallback without a marker).

**Proved.** `cargo nextest run -p theseus-sim`: 72 passed (was 63 before the branch). New tests:
`fake_model::tests_paced::a_paced_answer_keeps_its_first_byte_and_spacing_on_a_kept_connection` (first byte at
120 ms within [115, 170) ms; each chunk never before its time after the first byte and within 40 ms of it; two
requests on one connection, `conn_req` 0 then 1; the log's fields), `a_stepped_rule_scripts_a_turn_and_the_log_says_each_step`,
`a_marked_turn_steps_by_the_requests_it_was_answered`, `deltas_are_cut_into_the_chunks_asked_for`, and
`steps::tests` (three, Claude Code's merged shapes among them). The first version of the spacing test judged the
gap between two chunks as the client read them, and failed once in five suite runs (7.6 ms: a late read followed
by an on-time one); it now judges each chunk against the stand-in's schedule (a reader is late, never early), 0
failures in 8 suite runs since, 3 of them beside a release build. The legacy test `a_held_rule_...` now sends
`connection: close` (the stand-in keeps connections alive), its assertions unchanged.

### 2. The counted rows in the gate (`bench turn --check`)

**Changed** (`crates/theseus-sim/src/perf/counted.rs`, `ef0b197`): the turn bench, which the gate runs in every lane
(`turn` phase), now judges, beside the frames, counts and not times (D-6):
- `syncs_first_byte`: the WAL frames from a warm turn's submit to the stand-in's first byte, counted by the stand-in
  itself (a `Watch` hook reads the WAL as the request arrives and again just before the first byte is written).
  **Today 3, all before the request leaves** (`[execution.queued…]`, `[turn.started, node]`, `[context.compiled…
  action.dispatched]`); budget `SYNCS_BEFORE_FIRST_BYTE = 3.0`, a lower count passes (turn-one-frame and
  tool-loop-frames lower it at their joins).
- `first_request_kb`: a fresh session's first request **with the default tools**: a second scratch daemon of the
  default config (only the stand-in, a key from the environment, its paths). **33.7 KB (27 tools) against 40 KB.**
  The bench's own config is the template's every section (AWS, GitHub, MCP): its first request is **51.0 KB**,
  printed and in `--json`, not judged.
- `deltas_gathered`: a paced stream of 8 chunks reaches the client as 8 `model.delta` notifications (0 missing).
- `deltas_held`: in lockstep (the stand-in holds each next chunk until the client has the last, at most 5 s), each
  chunk reaches the client alone (0 missing).
- Recorded, not judged: a delta's way from the stand-in's write to the client (debug: p50 0.8 ms, max 3.3 ms).
The counted rows run before the plain and tool kinds and are printed at once, so a later check that bails leaves
them standing. The headroom warning no longer reads them as ms (`912e66d`, `history::tests::the_counted_rows_never_warn_at_their_budget`).

**Planted reverts** (each restored with `cp` of the saved file and `touch`; `git status` clean after each):
- An extra sync before the first byte: `self.store.put_meta("plant.extra_sync", &1u8)` before the model call in
  `turn.rs`. Caught: `syncs before the first byte: [4, 4, 4] … budget 3`. (The trace-against-WAL check then bails
  too: `a plain turn's trace counts Some(5) frames, and the WAL holds 6 … [meta:plant.extra_sync]`.)
- A delta held behind a timer: `on_delta` gathering text and telling it at most every 60 ms. Caught by both rows:
  `deltas: 3 of 8 chunks reached the client; in lockstep 1 of 8 alone` → `deltas_gathered: 5 … MISSED`,
  `deltas_held: 7 … MISSED`; the recorded delta's way read p50 50.7 ms, max 101.1 ms.

**Keel findings expected:** none. `python3 scripts/keel-guard.py` → `keel: ok (… 0 findings acked)` before each
commit. No budget, cap or ceiling moved; the new counts are new checks.

### 3. The harness overhead per turn (theseus-4w1h)

**Definition chosen: everything but the model and the tools** — the turn's time less its model's calls and its
tools' runs, the disk's commits included. Why: it is what a person waits for that is neither the model nor their
own work, and the commits are the harness's choice to make the turn durable; the speed wall already computed it
that way (`costOf`), so the README follows the wall rather than the other way round.

**Changed:** `crates/theseus-sim/src/perf/overhead.rs` splits each measured turn's trace exactly as the speed wall
does (commits = `store` spans, compiles = `compile`, admission = `lock`, the rest), printed by `bench turn` and in
`--json` (`plain.overhead`, `tool.overhead`). `cockpit/src/views/Speed.tsx`: the wall's header comment and the dial's
source now say the definition ("each the turn less its model and tools, the disk's commits included; the pointer
leaves the commits out").

**Measured** (release-thin build of this branch, `bench turn --runs 10 --burst 0`, load 0.43 → 0.44, this VM):
plain turn **p50 13.1 ms, p95 21.1 ms**: commits 3.3, compiles 3.8, admission 1.5, the rest 4.1 (part medians; the
turn 16.3, the model 3.0). Tool-call turn p50 27.1 ms (commits 8.1, compiles 8.1, admission 1.8, rest 7.2). Debug:
plain p50 52.8 ms (compiles 30.0). **Over the README's 5 ms budget.**

**Doc to change (the maintainer's):** README.md, "Fast is a contract", the row `| Harness overhead per turn | under 5
ms | to be measured | |` → `| Harness overhead per turn (the turn less its model and tools, commits included) |
under 5 ms | about 13 ms on a release build (commits 3, compiles 4, admission 1.5, the rest 4) | over |`, with the
owner's machine's number from `target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd
--runs 10` in place of this VM's.

### 4. `bench/h2h/`: the drivers, the report generator, and their tests

**Changed** (`269df74`; written with a helper agent in this session, then reviewed and fixed against live runs):
`standin.py` (rules per arm, process, log reader), `pty.py` (120x40 pty driver; transcript with `ESC[nC` as spaces;
**a screen grid**, added after the TUI's input line and replies were found split by cursor moves; terminal queries
answered; `/proc` sampler), `oneshot.py`, `joins.py`, `claude_code.py`, `theseus.py`, `run.py`, `report.py`,
`README.md`, `fixtures/`, six test files.

**Defects found and fixed in the live check:**
- The Theseus arm's socket path passed 108 bytes (`path must be shorter than SUN_LEN`): the daemon now lives in a
  short `/tmp/h2h-*` dir, removed at exit.
- With the state dir long, the daemon's spool notify socket did not bind and **a job's completion waited for the
  1 s heartbeat** (`WARN notify socket unavailable; heartbeat only`): T4 shell read 1,055 ms; with a short state dir,
  20 ms. **Worth the owner's word:** a long state dir silently makes every `proc.run` up to a second slower, said only
  in the log; health could name it (theseus-core, not touched here).
- The TUI's input-line marker `" > "` never matched (drawn by a cursor move); now `>` after the keypress.

**Proved.** `python3 -m unittest discover -s bench/h2h -p 'test_*.py'` → `Ran 46 tests … OK` (the parsers, the ANSI
strip and stamping, the grid, the joins of the stand-in's log against screen stamps, the one-shot join by time,
the report generator on the fixture run).

## The live check on this VM

Release-thin build of the branch (`scripts/build.sh --profile release-thin`, 17 min 39 s), stand-in the branch's
debug `theseus-sim` (its latest steps fix; same code). **Claude Code 2.1.296 is installed here**, so both halves ran.

Theseus alone, every row, 5 runs (`run.py --theseus-only --rows T1,T2,T3,T4,T5,T6,T7,task3,first_request_bytes
--runs 5 --long-turns 20 --idle-secs 10`), load 0.87 → 0.16, 100 samples, 0 failures; medians:
T1 42.1 ms · T2 10.9 (one-shot 13.2) · T3 1.1 (one-shot 176.3) · T4 read 11.0, shell 20.8, task3's read 12.1,
edit 14.2, shell 23.9 · T5 resume 79.9 (one-shot to the request 15.0) · T6 10 ms CPU (one-shot 7.8) · T7 idle CPU
0.1 %, RSS 49.1 MB · task3 2,003 ms (one-shot 1,967) · first request 34,527 bytes. Daemon starts: 22 to 34 ms.
T8 (beside a `dd … conv=fsync` loop, 3 runs, load 0.12 → 0.34): T1 42.6 · T2 27.1 · T3 1.1 · T4 read 28.3,
shell 103.2.

Both arms, 3 runs (`run.py --rows T1,T2,T3,T4,T6,task3,first_request_bytes --runs 3`), load 0.31 → 0.50, 96
samples, 0 failures; medians, Theseus / Claude Code: T1 42.5 / 685 ms · T2 10.1 / 202 (one-shot 15.3 / 654) ·
T3 0.97 / 260 (one-shot 176 / 201) · T4 read 12.4 / 130, shell 20.3 / 155, edit 18.0 / 228 · T6 CPU 10 / 1,110 ms
(one-shot 8.0 / 917) · task3 1,995 / 2,427 (one-shot 1,984 / 2,989) · first request 33.7 / 74.0 KB (one-shot
33.7 / 61.7). `report.py` on that run wrote the .md, .json and light/dark SVGs to a scratch dir (not committed).

**Finding:** Theseus's one-shot T3 is 176 ms, the whole stream (8 × 25 ms): `theseus ask` prints the reply only when
it ends, so a one-shot shows no first text until the last byte. Interactive T3 is 1 ms.

## The full run, for the maintainer (the owner's machine, pinned Claude Code)

```bash
scripts/build.sh --profile release-thin
# Claude Code 2.1.296 on PATH (or --claude PATH); every row, both arms, 10 runs, A/B interleaved:
python3 bench/h2h/run.py --bin-dir target/release-thin --runs 10 --long-turns 50 --idle-secs 30 --out /tmp/h2h
python3 bench/h2h/report.py /tmp/h2h/run.json          # docs/benchmarks/<run date>-head-to-head-speed.md, .json, img/
# The counted rows and the overhead's split, release:
target/release-thin/theseus-sim bench turn --theseusd target/release-thin/theseusd --runs 10 --check
```
Expect: `run.json` with no `failures`; Theseus's T1 tens of ms, T3 interactive about 1 ms, T4 tens of ms; Claude Code's
rows in hundreds of ms as above; the counted rows `syncs_first_byte 3`, `first_request_kb` about 33.7, both deltas
rows 0 missing; the overhead line per kind.

## Left, uncertain, for the owner

- The first-request bar holds for the default tools (33.7 KB) but not for a config with every section on (51 KB:
  AWS, GitHub, MCP tools). Which config the README's promise means is the owner's call.
- The harness overhead is 13 ms on a release build here, against a 5 ms budget; the compiles and the commits are
  most of it.
- T5's long session is made by running `--long-turns` one-shot turns through the stand-in for both arms (no shared
  synthetic store for Claude Code); the one-shot resume adds a turn each run.
- The Claude Code arm's scratch config keys (onboarding, bypass accepted, auto mode off) were read from the binary
  and work on 2.1.296; another version may need others (`--any-claude-version`).
- **F10's pty driver** does not exist on `main` as cloned, so there is nothing to compare; when it joins, `pty.py`'s
  `Pty`/`Screen`/`Transcript` are the natural shared module.
- The paced stand-in judges a chunk within 40 ms of its schedule in its test: generous for a debug suite under load,
  tight enough to tell 25 ms spacing from none.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`, see below):

- **Before `ef0b197`:** keel ok, fmt, shape, features, clippy, cockpit (lint, test, build), test build, reader rule
  ok; suite **3,656 passed, 33 failed**: exactly the known L1 set (theseus-sandbox's 20 contract tests and
  `spawn_100`, theseusd's 12 `sandbox` tests: a root daemon's job without a job cgroup, theseus-pv6i). The phases
  after it by hand: protocol types ok; `theseus-sim bench turn --check --runs 5 --burst 0` exit 0 (frames 5 and 9,
  syncs 3, 33.7 KB, deltas 0 and 0 missing); `cargo deny --offline check` ok.
- **Before `912e66d` and `269df74` (the final tree):** the same phases ok; suite **3,658 passed, 33 failed**, the
  same known L1 set; by hand: protocol types ok, the turn bench exit 0 with every counted row ok, deny ok; `keel: ok
  (… 0 findings acked)`; `bench/h2h`'s suite 46 OK.
- **A failure outside the lists, in the gate between those two** (not counted green; the gate was rerun):
  `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`, `the live wrapper's job
  runs on: {"dispatched":2,"outcome_unknown":3,"succeeded":11}`. That gate ran out of disk (see below). Rerun alone
  three times with the disk free: passed, **failed with the same assertion**, passed. So it is a flake of its own and
  not this branch's (no theseusd code or test changed here). Cause, read from the test (`crates/theseusd/tests/gone_jobs.rs`
  around line 350): it waits until `dispatched == 1`, then reads the actions again and asserts 1; its own comment
  says the late result's woken turn has a model call "dispatched for a moment", which can land between the two
  reads (2). The fix is to assert on the value the wait saw, or to wait for the turn's end; it should be filed and
  fixed with a planted revert, or listed as flaky.
- **The disk.** This VM's writable allowance ran out twice: once inside clippy (the first gate failed in clippy with
  no finding; clippy alone was clean) and once inside a suite. The debug target reached 26 GB with incremental
  artifacts beside non-incremental ones. I deleted `target/debug/incremental`, the release target once its binaries
  were copied out, and stale duplicate artifacts in `target/debug/deps` (older hashes of the same crate; cargo
  rebuilt what it needed), and ran the later gates with `CARGO_INCREMENTAL=0`. Nothing outside the repository and
  /tmp was touched.
- Not run here: the lifecycle and L1 jobs benches (`THESEUS_GATE_NO_BENCH=1`, a lane's gate). Python under bench/
  is outside the gate; its suite ran before the commit (46 OK).
