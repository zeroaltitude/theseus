# CLOUD REPORT: cloud/20261005-memory-tests

Five follow-ups from M6's memory reviews, all in crates/theseus-core: theseus-e21m, theseus-6fn.14, theseus-x875,
theseus-6fn.9, theseus-6fn.8. Cloned at 4a449460 (the task commit 4c8eede on top), store format 22, unchanged here.
Started 03:07 UTC; the report was written at the time on its last commit.

| Step | Issue | Commit |
| --- | --- | --- |
| 1. A search's own build never paces | theseus-e21m | e752f83 |
| 2. A search during the warm build answers `building` at once | theseus-6fn.14 | 1a5f95e |
| 3. A recalled source read by position after a restart | theseus-x875 | 455a890 |
| 4. The summary follows a one-message override | theseus-6fn.9 | fee24fc |
| 5. The summary reserved on the estimate's upper bound | theseus-6fn.8 | 81119b2 |

No store format bump, no protocol type, no config key, no new dependency, no Python. Nothing in turn.rs, recall.rs,
memory_pass/mod.rs, stub.rs, or `hit_of`; recall/render.rs was touched only by a plant, restored (git status clean
after each restore).

## What the code says, against the brief

Every claim in the brief's "What the code says" held as written, with one addition. The brief says the
x875 test runs "both turns in one Core, so its second render hits the cache". I confirmed it: main's version of
`the_next_request_begins_with_the_previous_requests_bytes` passes with the step 3 plant in (`source()` reading
`store.get_node` alone).

One detail on step 2: `kestrel`'s rig sets `recall_deadline_ms` to its maximum, 5,000 ms. So with the plant, the
search there answers `deadline` at 5.0 s, not at `SEARCH_DEADLINE`'s 2 s. A daemon on the default config, as in the
live check, answers at 2 s.

## Step 1: theseus-e21m, a search's own build never paces (e752f83)

**Found.** `Ask::run`'s `build(&self.store, false)` was held by nothing. `PACES` is thread-local, and the search's
build runs on the blocking pool.

**Changed.** I chose the count over the namespace. `Adjacent` has `paces: AtomicU64`, which every build adds to
before each pace waits (so a pace still waiting is seen), with `Adjacent::paces()`. The build's log line also names
its own `paces` beside `waited_ms`. New test: `tests_activation_search::a_searchs_own_build_never_paces`. It runs
`memory_search` under `+activation` on kestrel's store plus `PAGE + 1` nodes, with the projection unbuilt. The
search answers `ran`, B is admitted, the projection holds more than `PAGE + 1` nodes, and `paces() == 0`. A warm
build of the same store counts 1, so the counter is shown to count. It runs on any VM.

**Plant.** `build(&self.store, true)` in `Ask::run`:
```
panicked at crates/theseus-core/src/tests_activation_search.rs:45:5:
assertion `left == right` failed: a search's own build never paces
  left: 1
 right: 0
```

## Step 2: theseus-6fn.14, a search during the warm build answers `building` at once (1a5f95e)

**Changed.** In `Ask::run`, a search (`build: true`) that finds the projection unbuilt checks `building()` first. If
the warm build is running, it returns at once with the reason `WARM` ("the adjacency projection's warm build is
running, paced by the machine's pressure; a search does not wait for it"). The manifest's activation row carries
that as `why`, with outcome `building`.

I chose `building()` over `try_lock`. Only `warm` sets `building`, and it sets it before it spawns the build that
takes the lock. A turn's refresh, or another search's own unpaced build, holds the lock only as long as a fold takes,
and a search still waits for those, as it should.

One edit outside `Ask::run` and `Adjacent`: `activated`'s match reads the new reason as `building`
(`why == "building" || why == WARM`). The module docs and theseus-core's AGENTS.md (the `+activation` paragraph) say
the rule.

**Residual race (for the owner).** If `warm()` is first called between a search's `building()` check and its
`lock()`, a matter of microseconds, the search can still queue behind the warm build and answer `deadline`. Closing
it fully would mean the warm build folding outside the lock and installing at the end. That allows two builds at
once, so I left it.

**Test.** `tests_activation_search::a_search_during_the_warm_build_answers_building_at_once`. It runs as the pace
test's namespaced test runs: `unshare -rm`, fake pressure files bound over `/proc/pressure` saying IO 55 %. I
extracted that runner into a shared `tests_activation_pace::namespaced(test, marker)`, and
`a_clean_stop_ends_the_warm_builds_waits` now uses it too; its inner body is unchanged.

Namespaces work on this VM: the test ran, not `skipped`. Inside, kestrel's store gets `2 × PAGE + 1` more nodes,
and `warm()` starts. The test waits until `paces() ≥ 1`, which means the build is in its first pace. A search under
`+activation` must then answer `building`, with the reason, in under **1 s** (`AT_ONCE`), and the projection must
still be unbuilt. Then `stop_began()`; the build ends, and a search answers `ran` with B admitted and the whole
store folded.

Why 1 s: it is half of `memory.search`'s smallest deadline (2 s), and a fifth of this rig's (5 s). A search queued
on the lock answers only at its deadline, so it can never pass. Measured: 1.8 ms unloaded, 52 to 90 ms under load.

**Plant.** The check removed (`if false && …`):
```
SEARCH-INNER a search during the warm build answered deadline in 5.00244958s
assertion `left == right` failed: RecallActivation { outcome: "deadline", why: Some("the spread did not finish in the 4998 ms left of recall's deadline"), … }
  left: "deadline"
 right: "building"
```

## Step 3: theseus-x875, a recalled source read by position after a restart (455a890)

**Changed.** In `the_next_request_begins_with_the_previous_requests_bytes`, the second turn runs on a second Core
built over the same store and model (`restarted`: `Parts::for_tests` with `(*c.cfg).clone()`, the same `Store`
handle, and the same stand-in index). I chose a fresh Core over clearing the cache. It is what a restart is, and
clearing the cache needs a test hook on `Memory`'s private `sources`, in recall.rs, which soul-import is editing. The
prefix bytes are asserted as before.

One caveat: the second Core shares the first's `Store` handle, and so its heat cache (`node_cache`). That cache is
keyed by WAL position, so it serves the record at a position, never "the newest by id". It doesn't mask the plant.

**Plant.** `source()` reading `store.get_node(&r.node_id)` alone:
```
panicked at crates/theseus-core/src/tests_recall_node.rs:238:5:
assertion `left == right` failed: the second request does not begin with the first's bytes
```
main's version of the test passes with the same plant: the cache masked it, as the issue said.

## Step 4: theseus-6fn.9, the summary follows a one-message override (fee24fc)

**Changed (test only; the code was right).** `tests_compaction::summary_profile_session_follows_a_one_message_override`.
New helpers: `turn_on` (a turn with `resolve_target(live, None, Some(provider), Some(model))`, as `turn.submit`'s
`provider` and `model` do), `session_rig`, and `is_summary`. The rig uses `summary_profile = "session"` and gives
glm-5.3-flash the session model's window (40,000) and output cap (2,000), so a turn on either rings at the same
point.

A probe rig finds how many turns ring (K). The real rig runs K−1 turns on the session's model, then turn K with an
override to `zai`/glm-5.3-flash. The compile is `compaction`. One summary call goes to glm, carrying `turn0`, and
the session's provider gets none. The `Summary` node has profile `<live>`, model glm-5.3-flash, glm's text, and a
header ending `written by <live> on glm-5.3-flash]`. The `context.compacted` row's `model` is glm-5.3-flash. The
next turn, with no override, goes to claude-sonnet-5-5, and glm is asked nothing more.

**Plant.** `own` resolved from the session's own profile
(`self.resolve_target(&t.target.profile, None, None, None)`):
```
panicked at crates/theseus-core/src/tests_compaction.rs:808:5:
assertion `left == right` failed: one summary call, to glm
  left: 0
 right: 1
```

## Step 5: theseus-6fn.8, the summary reserved on the estimate's upper bound (81119b2)

**Changed.** `plan_summary` now reserves `price.reserve_micros(max_tokens, est.upper)`, with a comment saying why.
The fake provider gets `Scripted::BilledBy { usage: fn(&ProviderRequest) -> Usage, then }`: a bill computed from
the request it answers. It is a small addition to the test-only fake in provider.rs.

The test is `tests_compaction::the_summary_is_reserved_on_the_estimates_upper_bound`. glm bills each summary call
for 25 % more input than `compiler::estimate(request, glm's bytes_per_token).tokens`, and for output at the
request's `max_tokens` (4,096). The test checks the row's `input_tokens` and `summary_tokens` equal that bill. It
reads `settled_micros` and `reserved_micros` from the `context.compacted` row, as `settled_as_reserved` does, and
requires settled ≤ reserved, the kernel's action holding the same reservation, state `Succeeded`.

Numbers, on glm-5.3-flash ($0.15 in, $0.50 out a million):

| | input | output | micro-dollars |
| --- | --- | --- | --- |
| billed | 3,614 | 4,096 | settled 2,591 |
| reserved on `upper` | ≈ 4,048 | 4,096 | 2,656 |
| reserved on `tokens` (main) | ≈ 2,891 | 4,096 | 2,482 |

**Plant.** `est.tokens` again:
```
billed 3614 in and 4096 out: settled 2591 micros, reserved 2482
panicked at crates/theseus-core/src/tests_compaction.rs:894:5
```

**Should the turn's own call take `upper`?** I changed nothing in turn.rs. turn.rs:2144 reserves
`reserve_micros(target.max_tokens, compiled.est_tokens)`. The difference is smaller there, for two reasons. First,
`est_tokens` is mostly `counted`: the provider's own count of the last request, which has no margin to add.
`upper − tokens` is 0.4 × the newly estimated tail only. Second, the output term is the profile's whole cap: 128,000
on Sonnet 5.5, $1.28 at $10 a million.

Worked numbers, Sonnet 5.5 ($2 in, $10 out a million):
- An append turn, 60k counted plus 10k new estimated. `tokens` reserves 70,000 × $2/M + $1.28 = $1.42. `upper` adds
  4,000 × $2/M = $0.008, about 0.6 %.
- A first turn after a restart, or a recompile with nothing counted, 100k all estimated. `upper` adds
  40,000 × $2/M = $0.08 to a $1.48 reservation, about 5 %.

It only matters if a turn writes nearly all of its 128k output cap while the provider counts its input well over
the estimate. A provider counting 25 % over on that 100k turn costs $0.05 more than reserved, at full output.

My recommendation: take `upper` there too, for the same reason as here (money is a gate, reserved at its worst
case). It costs under 1 % on an ordinary append and about 5 % on a cold turn, and needs a one-line change at
turn.rs:2144 and :2223 (`input_micros`). Its golden and money tests may move; I didn't run them against it.

## Proof, offline

- **The new and edited tests, unloaded:** 7 of 7 pass. These are the two in tests_activation_search, both
  tests_activation_pace tests, the two new tests_compaction tests, and the edited recall-node test.
- **Families:** `tests_activation` (all 16, including arm, pace and search), `tests_compaction` (11), and recall,
  recall-node and retention, in the gate's full suite: all passed.
- **After every restore, on the head's tree:** `tests_activation*`, `tests_compaction`, `tests_recall*` and
  `tests_retention`, 50 tests, all pass.
- **Each plant:** fails as quoted above, and passes once restored and `touch`ed. git status was clean after each
  restore.
- **Under load, 5 runs:** each test at `nice -n 19`, beside four busy processes at nice 0, on 4 cores. I used
  `yes > /dev/null` processes, not `sh -c 'while :; do :; done'`: this environment's safety check refused the
  `sh -c` line, a false positive for a removal. Each `yes` holds a core at 100 % the same way. They were killed by
  their own pids.

| Test (5 runs under load) | Result | Durations |
| --- | --- | --- |
| `a_search_during_the_warm_build_answers_building_at_once` | 5/5 pass | search answered `building` in 54, 53, 90, 52, 52 ms; the warm build ended 3.39, 3.45, 3.63, 3.40, 1.56 s after the stop began; whole test 29 to 32 s |
| `a_searchs_own_build_never_paces` | 5/5 pass | 21 to 23 s |
| `summary_profile_session_follows_a_one_message_override` | 5/5 pass | 16.6 to 17.0 s |
| `the_summary_is_reserved_on_the_estimates_upper_bound` | 5/5 pass | 7.3 to 7.6 s; settled 2,591 against 2,656 each time |
| `the_next_request_begins_with_the_previous_requests_bytes` | 5/5 pass | 4.7 to 4.9 s |
| `the_warm_build_paces_between_its_pages_and_a_refresh_never_does` | 5/5 pass | 33 to 39 s |
| `a_clean_stop_ends_the_warm_builds_waits` (main's test) | **0/5** | see below |

**Finding: `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` fails under load.** It is a timing test,
not on the flaky list and not on the brief's list. All 5 runs under load failed: the build ended 5.08, 5.26, 5.14,
5.01, 5.17 s after it began, against a bound of 1.5 s + 3 × `LOOK_EVERY` = 4.5 s. Unloaded it passes; the gate's
suite passed it.

Cause: the bound counts the walk's own time after the stop, not only the waits. Under this load, folding
2 × PAGE + 1 nodes in a debug build takes about 3.4 s with no wait in it. My step 2 test measures the same fold after
`stop_began()` at 3.39 to 3.63 s. So the stop is not waiting on a pace; the work itself overruns a bound written as
three looks.

My edit to that test only moved its outer runner into `namespaced`; `stopped_inside` is byte-identical. To be sure,
I ran main's own versions of the test, activation.rs and lib.rs under the same load. See "main's run" below.

A fix, not made here because it is outside these issues: bound the build by the waits it skipped, not by wall
time. For example, assert on `waited` from the build's log, or on the count of paces that returned at once after the
stop. Or allow the measured fold time: time an unpaced build of the same store first, and bound by that plus
`LOOK_EVERY`.

**main's run.** I checked out 4c8eede's activation.rs, tests_activation_pace.rs and lib.rs in place, built them
unloaded (pass: the build ended 2.07 s after it began), then ran them under the same load 3 times. All 3 failed:
5.28, 5.14, 5.17 s. So the failure is main's, not this branch's. My files were restored and `touch`ed, and git
status was clean.

A first attempt at this run compiled at nice 19 under the busy processes, printed nothing for 30 minutes, and was
stopped at the tool's time limit. Its busy processes were gone when I checked their pids. The rerun compiled first,
unloaded.

## The live check (the maintainer's)

A scratch daemon: Discord and the web off, the stand-in model, and a synthetic store. theseusd runs inside
`unshare -rm` with `/proc/pressure` faked busy, as tests_activation_pace.rs fakes it. Paths are examples:

```sh
S=/tmp/mt-live; mkdir -p $S/psi
printf 'some avg10=1.00 avg60=0.00 avg300=0.00 total=1\n'  > $S/psi/cpu
printf 'some avg10=55.00 avg60=0.00 avg300=0.00 total=1\n' > $S/psi/io
theseus-sim synth-store --dir $S/state --sessions 10000   # --dir as the lifecycle bench passes it (check `--help`)
theseus-sim fake-model --rules … &                          # the stand-in model; point [providers] at it
# $S/theseus.toml: state_dir = $S/state, discord and web off, [memory] mode = "canary", arm = "+activation"
unshare -rm sh -c "mount --bind $S/psi /proc/pressure && exec theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state" &
```

1. While the warm build waits (no "adjacency projection is built" line in the log yet):
   `theseus --socket $S/sock memory search --arm +activation "<a word the store holds>"`. It should answer at once,
   with activation `building` and the why "the adjacency projection's warm build is running, paced by the machine's
   pressure; a search does not wait for it". On main it answered `deadline` after 2 s.
2. Stop, then set `arm = "baseline"` (no warm build) and start again the same way. The same search should answer
   activation `ran` in under a second, and the daemon's log line "memory: the adjacency projection is built" should
   say `waited_ms=0` and `paces=0`.
3. Two profiles (say `sonnet` and `glm`), a small `context_window` on the session model's catalog row so a session
   compacts, `[memory] summary_profile = "session"` (the default). Send a few long messages, then
   `theseus --socket $S/sock ask -m <the other profile's model> "<a long message>"` on the turn that rings. The
   `context.compacted` row (`theseus ledger --kind context.compacted`, or the cockpit) should name that model, with
   `reserved_micros` ≥ `settled_micros`. The session's next `ask` without `-m` should run on its own model.

Stop it with `theseus --socket $S/sock shutdown`.

## Left, uncertain, and for the owner

- **Step 2's race window** (above): a `warm()` called between a search's check and its lock can still queue that
  search. It is microseconds wide; closing it needs the warm build to fold outside the lock.
- **`Adjacent::paces`** is a total over the projection's life. Its only reader outside tests is the build's log
  line, which logs its own build's count. If health should show it, that is a later step.
- **The turn's own reservation:** recommended `upper` above, with numbers; turn.rs left alone.
- **Docs for the maintainer:**
  - Part III's items for these five issues.
  - In the spec's §2.7 or the 32b wire-in text: a search answers `building` while the warm build runs.
  - In the 30c compaction text: the summary is reserved on the estimate's upper bound.
  - docs/status.md's line for these.
  - theseus-core's AGENTS.md is updated in 1a5f95e.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the whole change (the tree of 81119b2): fmt, shape,
features, clippy, cockpit and the test build passed.

The suite ran 2,834 tests: 2,801 passed and 33 failed, 21 skipped. The 33 are exactly the known L1 failures (the VM
runs as root, theseus-pv6i): 20 in theseus-sandbox (its contract tests and its bench's `spawn_100`) and 13 in
theseusd's `sandbox` tests. No other test failed. Under the gate's own run, `tests_output`'s golden passed with
`TZ=America/Phoenix`, and no timing test on the brief's list failed.

I ran the phases after the suite myself: protocol types ok; the turn bench `frames_plain` 5 of 5 and `frames_tool`
9 of 9, ok; `cargo deny --offline check`: advisories, bans, licenses and sources ok. The lifecycle and jobs benches
were skipped by `THESEUS_GATE_NO_BENCH`.

Each intermediate commit (e752f83, 1a5f95e, 455a890, fee24fc) passed fmt, `clippy -p theseus-core --all-targets
-D warnings`, and its own family's tests. The gate itself ran on the final tree, which is byte-identical to 81119b2's
(checked file by file).
