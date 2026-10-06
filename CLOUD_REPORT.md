# CLOUD REPORT: cloud/20261005-judge-turn-cost

The judge's cost on the turn path, measured and then cut (theseus-0j2.8, theseus-289c). Started 03:06 UTC, report
at about 05:55 UTC (deadline 08:06).

| Step | Commit | Subject |
|---|---|---|
| 1 | `e6e30a3` | sim: bench turn --judge measures the judge's cost on a turn's path (theseus-0j2.8) |
| 2 | `f93fbc3` | judge: the sink writes its frames only between turns (theseus-0j2.8) |
| 3 | `8885cea` | judge: no point reads the ladder or the lineage; the warm read does (theseus-289c) |

Base: `c1c63ee` (the task commit) on main `4a44946`, store format 22. No format bump (nothing stored changed). No
new package: Cargo.lock gains one dependency line (`theseus-judge` under `theseus-sim`), no package.

## Step 1: the cost, measured (`e6e30a3`)

### What I built

`theseus-sim bench turn --judge` (`crates/theseus-sim/src/perf/judge.rs`, a new module; perf.rs gains the flag,
`scratch_with`, and the doc lines). Three arms, each its own scratch daemon on the stand-in model:

- `off`: the gate's turn bench (`quiet_config`, the judge off).
- `loop`: the judge on at theseus-judge's fake Jev, started in process as `scratch` starts the stand-in model;
  classify, role and route off, as theseusd's judge test runs it. (The other points stay on: gate, compile,
  categorize, rerank. The name says what is off, not that loop.v1 judges alone.)
- `packs`: every pack as wired. route.v1 is live, so each person's message waits beside its first compile for the
  fake's verdict, scripted to a confident `chat` (0.95): a real switch to chat's profile, which the bench points at
  the stand-in like every provider. The stand-in and the fake make a valid routed turn; the turn's frames are the
  same 5 and 9, with `ledger:route.decided` in the compile's frame. security's `risky` is scripted to 0.02, so no
  notice posts.

Each frame is the judge's (every record a `judge.*` or `pack.*` row, or a `judge.*` META record) or the turn's (checked
against its trace exactly as `bench turn` checks it; the bench fails on a mismatch). `walcount` now labels a META
record by its key (`meta:judge.budget`), which is how the budget's record is told apart. Each judge frame is placed
before a turn's answer, after it (inside the 50 ms window), or between turns. Blobs the store gained are counted,
two syncs each. `--check` judges the off arm against today's budgets; `--record` writes the off arm's columns and
six new ones (`turn_plain_jloop`, `turn_tool_jloop`, `jframes_jloop`, and the `jpacks` three), added to
`history::OTHER` with their names. The issue's nit (perf.rs's doc and the turn header naming only Discord and the
web UI) is fixed.

### What it found (release-thin, `e6e30a3`, 3 runs × 10 turns of each kind per arm, nothing else running)

Wall p50 / p95 in ms; judge frames as before-the-answer · after · between, per run.

| arm | turn | frames | run 1 | run 2 | run 3 | judge frames (runs 1, 2, 3) |
|---|---|---|---|---|---|---|
| off | plain | 5 | 9.7 / 11.5 | 9.7 / 11.6 | 9.7 / 10.5 | — |
| off | tool-call | 9 | 22.7 / 28.7 | 22.6 / 24.3 | 24.3 / 32.2 | — |
| loop | plain | 5 | 10.4 / 13.2 | 9.7 / 12.3 | 10.4 / 12.5 | 0·1·0, 0·1·0, 0·1·0 |
| loop | tool-call | 9 | 26.3 / 27.8 | 23.6 / 26.4 | 25.4 / 28.5 | **1**·2·0, **1**·2·0, **1**·2·0 |
| packs | plain | 5 | 11.5 / 13.6 | 11.1 / 12.5 | 10.5 / 13.4 | **1**·1·0, 0·2·0, **1**·1·0 |
| packs | tool-call | 9 | 26.0 / 28.1 | 24.7 / 27.1 | 26.5 / 30.0 | **1**·3·0, **1**·3·0, **1**·3·0 |

- Over 20 measured turns: loop 5 judge frames (1 after the last turn), 92 blobs, 92 Jev calls; packs 7 frames (1
  after the last), 112 blobs, 112 calls. Shapes: `[ledger:judge.call ×32, meta:judge.budget]` (the sink, full at
  32 rows), `[ledger:judge.call ×24–28, meta:judge.budget]`, and two `[meta:judge.categorize.<session>]` per arm
  (categorize.v1's mark, written in its prepare; see "Left").
- This disk's fdatasync p50 0.14–0.15 ms. The bench's estimate of what the judge added (frames + 2 × blobs, at that
  p50): loop 25.6–28.0 ms over 20 turns (1.28–1.40 ms a turn), packs 31.3–34.2 ms (1.57–1.71 ms a turn).
- **strace** (`strace -f -T -y -e trace=fsync,fdatasync` on each arm's daemon, head build, over the daemon's whole
  life: start, the first turn, 3 warm-ups, 20 measured turns, the drains): off 172 WAL syncs (73.6 ms); loop 191 WAL
  syncs (83.3 ms) and 129 blobs = 258 syncs (99.9 ms: file 63.3, directory 36.6); packs 194 WAL syncs (83.1 ms) and
  153 blobs = 306 syncs (110.5 ms). The index tender's syncs (943–1079, about 340–380 ms) are the same in every arm.
  So the judge adds about 20 WAL frames per arm, and its blobs (each judged state's file and directory) are most of
  its disk cost: about 4–4.6 ms of sync a turn under strace, off the WAL's writer but on the same disk.
- Verdict: judge frames do land before a turn's answer (1–2 per arm per run), and the judge-on plain p50 moves
  (9.7 → 9.7–11.5). Step 2 applies.

### Proof

- `cargo nextest run -p theseus-sim`: 53 passed (2 new: `a_frame_is_the_judges_when_every_record_is`,
  `every_column_the_judge_bench_records_has_a_history_column`).
- Gate (below): green but for the 33 known L1 tests.

## Step 2: cut (`f93fbc3`)

### What I changed

The sink waits for a moment between turns before each frame, through `memory_pass::turns` (`Turns::between`, the
pass's `Timing::default()`: no turn running and none for 500 ms; past 120 s any gap; past 600 s beside a turn), as
consolidation does. The core hands the judge its running turns as it builds (`JudgeService::write_between`, one
line in `rpc/mod.rs` beside `attach`); the sink holds the `Arc<Turns>`, never the service, across the wait, so a stop
never waits on it. Judgments that land meanwhile join the frame (up to 32). A judgment's row and its facts stay in one
frame, said once written; a press still finds an unwritten judgment in `pending` (removed only after the append, as
before). memory_pass/ is unchanged.

- **What now waits longer:** a judgment's `judge.call` row, its sentences, and its `record_judgment` metric, by the
  turns that run after it plus 500 ms (bounded at 120 s / 600 s on a busy daemon). `judge.list` and the learning
  ledger see it that much later. A turn that begins while the sink's append runs waits for that one append, as it
  does for the pass's.
- **Why not ride a turn's or the kernel's next frame:** AGENTS.md says nothing of a judgment rides a turn's frames but
  its mark; it would put one session's judgments into another session's turn, grow the 5/9 frame budgets' frames,
  and tie a judgment's durability to a turn that may never come.
- **The budget's block frame:** `reserve` (judge-tests' area, left alone) still writes a block frame when a
  reservation passes the last block ($0.01), at prepare, not between turns. None landed in a measured window here
  (the fake's prices are small); on the real Jev one lands every cent of shadow spend and can land in a turn.
  Route/rerank's block frames are beside their calls by design (theseus-otny).

### Proof

- `tests_sink_between::a_judgments_frame_waits_for_the_running_turn_to_end`: a turn held running past the sink's
  window; no `judge.call` row until it ends, and the row 500 ms or more after. **Planted** (the wait replaced by
  `None`): it fails, "no judgment's frame while a turn runs". Restored and touched.
- **Bench plant** (debug build, the same plant, two runs): judge frames before an answer came back (before ·
  after): loop tool-call 2·1 and 1·2, packs plain 1·1 twice, packs tool-call 3·1 twice. Unplanted at this commit,
  four debug runs: 0 before any answer.
- Release-thin at the head (`8885cea`), 3 runs × 10:

| arm | turn | run 1 | run 2 | run 3 | judge frames (each run) |
|---|---|---|---|---|---|
| off | plain | 9.5 / 11.9 | 9.2 / 9.9 | 10.4 / 11.7 | — |
| off | tool-call | 23.1 / 25.2 | 27.0 / 30.7 | 25.2 / 28.1 | — |
| loop | plain | 10.4 / 12.3 | 10.6 / 11.5 | 11.5 / 14.3 | 0·1·0 |
| loop | tool-call | 23.0 / 27.6 | 22.9 / 26.6 | 25.5 / 34.1 | 0·1·0 |
| packs | plain | 11.1 / 13.4 | 9.1 / 11.2 | 10.8 / 12.7 | 0·1·0 |
| packs | tool-call | 26.0 / 27.6 | 23.2 / 25.4 | 24.1 / 26.3 | 0·1·0 |

  No judge frame lands before an answer. Every sink frame is after the last measured turn (loop 4 of 6, packs 6 of
  8); the one "after" frame per kind is categorize's META mark. The p95s are within this VM's noise (with 10 runs
  the p95 is the slowest run), so I claim the frames, not a p95 change; the 30-run live check is the place to read
  the p95. Blobs and calls read 112/132 here against 92/112 at step 1: the head's bench adds a first turn (step
  3), and the head bench against step 1's daemon reads 112/132 too, so the arms compare like for like.
- Judge-area suites (below), theseusd's judge test 4/4.

## Step 3: before the warm read, a point reads and writes nothing (`8885cea`)

### What I found (the code against the brief)

As the brief says: `mode_for` → `Ladder::given` → `with` loaded each pack's scope, today's event scopes and the
brake's key, and wrote the missing adoptions, on the asking thread under a std Mutex; `placed` read the ladder and
the lineage the same way. Also: the rollover inside `with` re-read the day (`read_day`); `capped_by_root` reads the
lineage only for a learned name (not a point's case before the read); `pack_list` and the learning loop call
`placed`; `judge.label`'s notices brake asks `mode_for` (so an RPC reached the pre-read rule; see below); and
`route_base` clears a session's move whenever route.v1 answers below canary. On step 1's daemon a turn submitted the
moment the daemon answers had 2–3 `pack.mode` frames before its answer (the bench's new first-turn line).

### What I changed

- `Ladder::given`: before the ladder is loaded, `Ladder::unread`: the wired line under the config, `min` shadow.
  `JudgeService::placed`: the root until `ladder_read()` (ladder and lineage both loaded). Nothing is read or
  written on the asking thread.
- `with`'s load stays for the warm read, the RPCs and the nightly check. `JudgeService::read_ladder` (the warm read:
  `Ladder::read` and `Lineage::read`, nothing written) and `placed_read` (for `pack.list` and the learning loop's
  `gather`). `judge.label` now reads the ladder first (its notices' brake asks what acts).
- `Core::warm_ladder`: the read on the blocking pool, then, only if an adoption is missing, a quiet stretch (500 ms)
  after serving and a moment between turns (`memory_pass::turns`), then the adoptions **in one frame**
  (`Ladder::write_all_in`; before, one frame each).
- Health's pack lines before the read say the pre-read answer: `rerank.v1: shadow (until the ladder is read; wired
  live)`; a pack whose pre-read answer equals its line reads as before.
- `route_base`: while the ladder is unread (and the judge on), a turn runs at its base and the session's move is
  kept for the read ladder, instead of being cleared.
- **Design question, decided: shadow until the read.** A pack that would act (route.v1, rerank.v1, security.v3 as
  wired) judges in shadow until the ladder is read, so the build never acts on a pack the owner (or a rule) rolled
  back. The cost is a moment after serving (the read is a few scope reads on the blocking pool) in which route,
  rerank and v3's notices do not act; the wired-line choice would have acted live on a rolled-back pack in that
  moment. If the warm read cannot read the store, the packs stay in shadow until an RPC loads it (before: the wired
  line, live).
- **Rollover, decided: a new day starts empty and reads nothing.** Every event since midnight in this process landed
  through `land`; the notices' brake already reloads the ladder when it writes its pause (`pause_notices`). No timer.
- AGENTS.md's ladder and lineage lines updated (theseus-core), and theseus-sim's for the bench.
- **Core tests that now load the ladder first** (`tests_judge::warm`, the daemon's warm read at build when the judge
  is on): `tests_judge::rig_on` (so every file that builds through it: tests_judge, tests_inbound, tests_continue,
  tests_judge_surfaces, tests_learning, …), the restart rig in tests_judge, `tests_route::rig_on`,
  `tests_rerank::rig` (tests_rerank, tests_rerank_live, tests_retention), `tests_notices`' core, and
  `tests_security`'s core. Also theseus-discord's gateway rig (`read_ladder`, one line).
  `tests_rerank_live::normalized` leaves out a recalled note's store position (`(as of @<n>)`): the warm read's
  three adoption rows move positions against the judge-off rig.

### Proof

- `tests_ladder_unread.rs`:
  - `before_the_warm_read_a_judged_turn_reads_and_writes_nothing_of_the_ladder`: a store where the owner rolled
    route.v1 back (through `pack.rollback`, after the adoptions); a new core; a turn judged at the inbound (classify,
    role, route), gate (security v1, v3) and loop-end points, with the compile point on (continue.v1 judges only a
    compile whose signal fired; this turn fires none, so its judgment is not required). The ladder and the lineage
    stay unloaded, `reads() == 0`, no `pack.mode` row written, route/rerank/v3 answer shadow, health says so, and
    the turn's `attrs.frames` equals a judge-off core's same turn. After `warm_ladder`: one read, route.v1
    `RolledBack`, rerank.v1 live. A first ask with the ladder's clock a day on: still one read.
  - `the_warm_reads_adoptions_wait_for_a_moment_between_turns`: a fresh store, a turn held running: no adoption
    written; once it ends, all three.
- **Plants:** `given` loading as before: fails, "the ladder is unread". The rollover reading the day again: fails,
  "the new day read nothing" (2 reads, not 1). Both restored and touched; `git status` clean of them.
- Under load (nice 19, four busy loops): the two new files' three tests, 6 runs, all passed.
- Bench, a turn submitted the moment the fresh daemon answers: head, 9 arms over 3 release runs and 6 debug arms:
  0 `pack.mode` frames before its submit, before its answer, or after it; the trace counts 6. Step 1's daemon under
  the same bench: 1·2·0 and 0·3·0 (before submit · before answer · after), twice each.

## The live check (the maintainer's, on the 16-core machine)

1. Build each: `git worktree add /tmp/s1 e6e30a3 && (cd /tmp/s1 && scripts/build.sh --profile release-thin)`, and
   the head the same way. Then, for each:
   `target/release-thin/theseus-sim bench turn --judge --runs 30 --theseusd target/release-thin/theseusd`.
   At `e6e30a3`: judge frames in the "before the answer" column for loop and packs (expect a few per arm), and each
   arm's p95. At the head: 0 before every answer; "after" holds categorize's marks only (about 1 per 10 messages);
   the first-turn line 0·0·0; the judged arms' p95 against the off arm's.
2. The gate's turn bench, unchanged: `target/debug/theseus-sim bench turn --check` (and the join's `--runs 10
   --burst 30`): frames 5 and 9, p50 as on main.
3. A scratch daemon on its own `--config`, `--socket`, `--state-dir`, the judge on with Jev's key (a few cents, with
   the owner's go): start it, run from your own shell `theseus --socket <sock> packs rollback route.v1 --why "live
   check"`, stop it (`theseus --socket <sock> shutdown`), start it again, and ask health as soon as it answers, e.g.
   `until theseus --socket <sock> health --json 2>/dev/null | jq -r '.judge.packs[]'; do :; done`. Expect, if the
   first answer beats the read (a few ms): `route.v1: shadow (until the ladder is read; wired live)`, `rerank.v1:
   shadow (until …)`, `security.v3: shadow (until …)`; then, at the next ask: `route.v1: rolled back (owner: live
   check)` and `rerank.v1: live (owner: decision of 2026-10-04)`. No `pack.mode` row is written by the second start
   (`theseus --socket <sock> ledger --kind pack.mode`: the first start's adoption frame and the rollback only).

## Left, uncertain, and for the owner

- **categorize.v1's mark** (`judge.categorize.<session>`, a META frame written in `prepare_categorize` as a judgment
  is dispatched) lands next to turns: two per arm per bench run, after an answer; in use about one per 10 human
  messages. Not cut: moving it means keeping the mark in memory until the sink writes it in its frame (the
  `deciding` set already guards a second decision). A follow-up if the owner wants it.
- **Blob syncs are the judge's main disk cost** (step 1's strace: 258–306 syncs and 100–110 ms per arm, against
  about 20 extra WAL frames). They do not take the WAL's writer, but they share the disk with a turn's frames. A
  follow-up could stage every judged state's blob (as theseus-otny stages route/rerank's) and write them in the
  sink's between-turns moment, with one directory sync per batch.
- The budget's block frame (`reserve`, judge-tests' area) is still written at prepare; see step 2.
- notice.rs's health line (`notices: on` while `!is_loaded()`) says "on" before the read, while v3 judges in shadow
  then. A few ms; notice.rs is judge-reads' file, so I left it.
- Edits outside my own files, each small: `turn/route_step.rs` (route_base's pre-read case, 4 lines);
  `rpc/learning.rs` (`judge.label` reads the ladder first); `learning/propose.rs` (`placed_read`); the test rigs
  named above; `crates/theseus-discord/src/tests_gateway.rs` (one warm read in the rig).
- `bench turn --judge`'s first turn adds one Jev call per measured turn in the judged arms (92→112, 112→132), the
  same with step 1's daemon; I did not trace which pack it moves (likely categorize's or continue's trigger, since
  the first turn is another session). It does not change the comparison.
- Docs for the maintainer: the spec's Part III item and docs/status.md; docs/design/m5-judgment.md §2.7 says the
  ladder is read "at the first read after serving … or the first judgment" (now: never by a judgment, and shadow
  before the read); docs/design/m5-judgment.md §2.5 (the sink) could say its frames are written between turns.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (with `CARGO_INCREMENTAL=0`; see below), at each step:

- Step 1 (`e6e30a3`): fmt, shape, features, clippy, cockpit, test build ok; suite 2832 run, 2799 passed, 33 failed:
  exactly the known L1 set (theseus-sandbox's contract tests and `spawn_100`, theseusd's sandbox tests). The phases
  after the suite run by hand: protocol types unchanged, nothing compiled under the lock, `bench turn --check --runs
  5 --burst 0` 5/9 ok, `cargo deny --offline check` ok.
- Step 2 (`f93fbc3`): the same, suite 2833 run, 33 failed (the same L1 set); after-suite phases ok.
- Step 3 (`8885cea`): suite 2835 run, 2801 passed, 34 failed: the 33 L1 tests and
  `theseus-core learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy` (assertion
  "the pool thread took the idle thread's policy", left 0, right 5). Not on the flaky list and in no code this
  branch touches; it passes alone 3/3 and fails 3 of 15 under the load recipe, so it is a load-sensitive test of
  tokio's blocking pool (cause not traced). After-suite phases ok.
  - An earlier step-3 gate run failed `theseus-discord tests_gateway::a_jev_notice_goes_to_the_owners_dm_and_a_press_there_labels_it`
    (its rig never did the warm read): fixed in the commit.
  - Another run failed about 100 job tests at once: the disk allowance was full (916 MB free; duplicate debug
    artifacts after switching to `CARGO_INCREMENTAL=0`). I deleted the stale large artifacts in `target/debug/deps`
    and `target/debug/incremental` (all rebuildable) and reran.
- Judge-area suites at the head (`TZ=America/Phoenix`): 140 tests (tests_ladder, ladder, lineage, learning::tender,
  tests_learn*, tests_judge*, tests_continue, tests_notices, tests_route*, tests_rerank*, tests_sink_between,
  tests_security, tests_inbound, telemetry::tests_judge) all passed; theseusd `--test judge` 4/4; theseus-sim 53/53.
- No timing test from the brief's list failed in these runs.
